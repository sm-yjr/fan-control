//! Read-only whole-machine CPU utilization from public Mach CPU tick counters.
//! No GPU utilization is inferred: a missing GPU measurement must stay unknown.
use crate::{Error, Result};
use std::time::{Duration, Instant};

const MAX_SAMPLE_GAP: Duration = Duration::from_secs(60);
const IDLE_INDEX: usize = 2;

/// Owns a Mach host send right and a fresh pair of CPU tick samples.
pub struct CpuLoadMonitor {
    host: native::Host,
    sampler: TickSampler,
}
impl CpuLoadMonitor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            host: native::Host::open()?,
            sampler: TickSampler::default(),
        })
    }
    /// Initial, stale, failed, reset, or zero-delta samples return `None`.
    /// A real idle interval returns `Some(0.0)`; a busy interval can reach 100.
    pub fn sample(&mut self) -> Option<f64> {
        self.sampler.observe(self.host.ticks().ok(), Instant::now())
    }
    /// Discard a baseline after sleep or another interruption.
    pub fn reset(&mut self) {
        self.sampler.previous = None;
    }
}

#[derive(Default)]
struct TickSampler {
    previous: Option<([u32; 4], Instant)>,
}
impl TickSampler {
    fn observe(&mut self, ticks: Option<[u32; 4]>, now: Instant) -> Option<f64> {
        let Some(ticks) = ticks else {
            self.previous = None;
            return None;
        };
        let (previous, previous_at) = self.previous.replace((ticks, now))?;
        let elapsed = now.checked_duration_since(previous_at)?;
        if elapsed.is_zero() || elapsed > MAX_SAMPLE_GAP {
            return None;
        }
        let mut delta = [0u64; 4];
        for (index, (current, previous)) in ticks.into_iter().zip(previous).enumerate() {
            let change = current.wrapping_sub(previous);
            // A small u32 rollover is valid; a backwards/reset counter produces
            // an impossible half-range jump for this bounded sample interval.
            if change >= (1 << 31) {
                return None;
            }
            delta[index] = u64::from(change);
        }
        let total: u64 = delta.iter().sum();
        if total == 0 {
            return None;
        }
        let busy = total - delta[IDLE_INDEX];
        let percent = busy as f64 / total as f64 * 100.0;
        percent.is_finite().then_some(percent.clamp(0.0, 100.0))
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    // SDK mach/host_info.h + mach/machine.h; Apple XNU exposes the same ABI:
    // https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/host_info.h
    const HOST_CPU_LOAD_INFO: libc::c_int = 3;
    #[repr(C)]
    #[derive(Default)]
    struct HostCpuLoadInfo {
        cpu_ticks: [u32; 4],
    }
    extern "C" {
        fn mach_host_self() -> u32;
        fn host_statistics(
            host: u32,
            flavor: libc::c_int,
            output: *mut libc::c_int,
            count: *mut u32,
        ) -> libc::c_int;
        fn mach_port_deallocate(task: u32, name: u32) -> libc::c_int;
        static mach_task_self_: u32;
    }
    pub struct Host(u32);
    impl Host {
        pub fn open() -> Result<Self> {
            let port = unsafe { mach_host_self() };
            if port == 0 {
                return Err(Error("Mach host port unavailable".into()));
            }
            Ok(Self(port))
        }
        pub fn ticks(&self) -> Result<[u32; 4]> {
            let mut data = HostCpuLoadInfo::default();
            let mut count = 4;
            let status = unsafe {
                host_statistics(
                    self.0,
                    HOST_CPU_LOAD_INFO,
                    &mut data as *mut _ as _,
                    &mut count,
                )
            };
            if status != 0 || count != 4 {
                return Err(Error(format!(
                    "Mach CPU statistics unavailable: status={status} count={count}"
                )));
            }
            Ok(data.cpu_ticks)
        }
    }
    impl Drop for Host {
        fn drop(&mut self) {
            unsafe {
                mach_port_deallocate(mach_task_self_, self.0);
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn cpu_info_matches_sdk_integer_count_layout() {
            assert_eq!(
                std::mem::size_of::<HostCpuLoadInfo>(),
                4 * std::mem::size_of::<libc::c_int>()
            );
            assert_eq!(
                std::mem::align_of::<HostCpuLoadInfo>(),
                std::mem::align_of::<libc::c_int>()
            );
        }
    }
}
#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub struct Host;
    impl Host {
        pub fn open() -> Result<Self> {
            Err(Error("Mach CPU statistics require macOS".into()))
        }
        pub fn ticks(&self) -> Result<[u32; 4]> {
            Err(Error("Mach CPU statistics require macOS".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialization_idle_busy_and_wrapping_are_bounded() {
        let now = Instant::now();
        let mut sampler = TickSampler::default();
        assert_eq!(sampler.observe(Some([10, 20, 30, 40]), now), None);
        assert_eq!(
            sampler.observe(Some([20, 30, 50, 40]), now + Duration::from_secs(1)),
            Some(50.0)
        );
        assert_eq!(
            sampler.observe(Some([20, 30, 60, 40]), now + Duration::from_secs(2)),
            Some(0.0)
        );
        assert_eq!(
            sampler.observe(Some([30, 30, 60, 40]), now + Duration::from_secs(3)),
            Some(100.0)
        );
        sampler = TickSampler::default();
        sampler.observe(Some([u32::MAX - 4, 0, 100, 0]), now);
        assert_eq!(
            sampler.observe(Some([5, 0, 110, 0]), now + Duration::from_secs(1)),
            Some(50.0)
        );
    }
    #[test]
    fn faults_and_time_anomalies_do_not_fabricate_load() {
        let now = Instant::now();
        let mut sampler = TickSampler::default();
        sampler.observe(Some([100, 100, 100, 100]), now);
        assert_eq!(
            sampler.observe(Some([100, 100, 100, 100]), now + Duration::from_secs(1)),
            None
        );
        assert_eq!(sampler.observe(None, now + Duration::from_secs(2)), None);
        assert_eq!(
            sampler.observe(Some([110, 100, 110, 100]), now + Duration::from_secs(3)),
            None
        );
        assert_eq!(
            sampler.observe(Some([120, 100, 120, 100]), now + Duration::from_secs(3)),
            None
        );
        assert_eq!(sampler.observe(Some([130, 100, 130, 100]), now), None);
        assert_eq!(
            sampler.observe(
                Some([140, 100, 140, 100]),
                now + MAX_SAMPLE_GAP + Duration::from_secs(1)
            ),
            None
        );
        assert_eq!(
            sampler.observe(Some([0; 4]), now + MAX_SAMPLE_GAP + Duration::from_secs(2)),
            None
        );
    }
    #[test]
    #[ignore = "opt-in, read-only physical CPU telemetry probe"]
    fn native_cpu_readonly_probe() {
        let mut monitor = CpuLoadMonitor::new().unwrap();
        assert_eq!(monitor.sample(), None);
        std::thread::sleep(Duration::from_millis(250));
        let sample = monitor
            .sample()
            .expect("two native tick samples should have a nonzero delta");
        assert!((0.0..=100.0).contains(&sample));
        println!("read-only CPU utilization: {sample:.2}%");
        monitor.reset();
        assert_eq!(monitor.sample(), None);
    }
}
