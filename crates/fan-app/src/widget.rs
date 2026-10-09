//! GUI-only, bounded display data. This module never talks to the helper or SMC.
use fan_core::Snapshot;
use objc2_foundation::{NSBundle, NSFileManager, NSString};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const GROUP: &str = "JFC5CWT3V6.com.local.fan-control.readings";
const PERIOD: Duration = Duration::from_secs(30);
const HISTORY_SECONDS: f64 = 3600.;

/// Explicit cross-process signing diagnostic; does not touch the production
/// snapshot, configuration, helper or hardware. Run only for approved candidates.
pub fn check_container() -> Result<(), String> {
    let directory = NSFileManager::defaultManager()
        .containerURLForSecurityApplicationGroupIdentifier(&NSString::from_str(GROUP))
        .and_then(|url| url.path())
        .ok_or("App Group container unavailable")?;
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    );
    let bytes = serde_json::to_vec(&serde_json::json!({"nonce": nonce}))
        .map_err(|error| error.to_string())?;
    crate::storage::atomic_save(
        &PathBuf::from(directory.to_string()).join("fan-control-widget/container-check.json"),
        &bytes,
    )
    .map_err(|error| error.to_string())?;
    println!("App shared container wrote: {nonce}");
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
struct Point {
    sampled_at: f64,
    temperature: Option<f64>,
}
#[derive(Debug, Serialize)]
struct Fan {
    id: u8,
    name: String,
    rpm: Option<f64>,
    min_rpm: Option<f64>,
    max_rpm: Option<f64>,
}
#[derive(Debug, Serialize)]
struct DisplaySnapshot {
    schema_version: u32,
    written_at: f64,
    sampled_at: Option<f64>,
    state: &'static str,
    temperature: Option<f64>,
    fan_count: Option<u8>,
    fans: Vec<Fan>,
    history: Vec<Point>,
}
fn finite_rpm(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v >= 0. && *v <= 100_000.)
}
fn timestamp(now: f64, age: f64) -> Option<f64> {
    (now.is_finite() && age.is_finite() && age >= 0. && age <= now).then_some(now - age)
}
fn display(
    snapshot: &Snapshot,
    age: f64,
    now: f64,
    state: &'static str,
    history: Vec<Point>,
) -> DisplaySnapshot {
    let sampled_at = timestamp(now, age).filter(|_| {
        snapshot.fan_count.is_some() || !snapshot.sensors.is_empty() || !snapshot.fans.is_empty()
    });
    let temperature = snapshot.average_temperature(fan_core::SensorGroup::Cpu);
    DisplaySnapshot {
        schema_version: 1,
        written_at: now,
        sampled_at,
        state,
        temperature,
        fan_count: snapshot
            .fan_count
            .filter(|n| n.is_finite() && *n >= 0. && *n <= 255. && n.fract() == 0.)
            .map(|n| n as u8),
        fans: snapshot
            .fans
            .iter()
            .take(8)
            .map(|fan| Fan {
                id: fan.id,
                name: fan.name.chars().take(64).collect(),
                rpm: finite_rpm(fan.current_rpm),
                min_rpm: finite_rpm(fan.min_rpm),
                max_rpm: finite_rpm(fan.max_rpm),
            })
            .collect(),
        history,
    }
}

pub struct Publisher {
    path: PathBuf,
    history: Vec<Point>,
    last_write: Option<Instant>,
    last_state: &'static str,
    last_reload: Option<Instant>,
}
impl Publisher {
    /// Demo, bare binaries and helper entry points must never publish production data.
    pub fn new(demo: bool) -> Option<Self> {
        if demo || NSBundle::mainBundle().bundleIdentifier()?.to_string() != "com.local.fan-control"
        {
            return None;
        }
        let directory = NSFileManager::defaultManager()
            .containerURLForSecurityApplicationGroupIdentifier(&NSString::from_str(GROUP))?
            .path()?;
        Some(Self {
            path: PathBuf::from(directory.to_string()).join("fan-control-widget/snapshot-v1.json"),
            history: Vec::new(),
            last_write: None,
            last_state: "unknown",
            last_reload: None,
        })
    }
    pub fn publish(&mut self, snapshot: &Snapshot, age: f64, state: &'static str) {
        if self.last_state == state && self.last_write.is_some_and(|last| last.elapsed() < PERIOD) {
            return;
        }
        let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return;
        };
        let mut data = display(snapshot, age, now.as_secs_f64(), state, Vec::new());
        if let Some(sampled_at) = data.sampled_at {
            if state == "running"
                && age <= 120.
                && self
                    .history
                    .last()
                    .is_none_or(|p| sampled_at - p.sampled_at >= 25.)
            {
                self.history.push(Point {
                    sampled_at,
                    temperature: data.temperature,
                });
            }
        }
        // Clock rollback and wake start a new history; never draw across missing samples.
        self.history.retain(|p| {
            p.sampled_at <= data.written_at && data.written_at - p.sampled_at <= HISTORY_SECONDS
        });
        if state == "sleeping" || state == "starting" {
            self.history.clear();
        }
        if self.history.len() > 120 {
            self.history.drain(..self.history.len() - 120);
        }
        data.history = self.history.clone();
        let changed = self.last_state != state;
        self.last_state = state;
        self.last_write = Some(Instant::now());
        let result = serde_json::to_vec(&data)
            .map_err(std::io::Error::other)
            .and_then(|bytes| crate::storage::atomic_save(&self.path, &bytes));
        if let Err(error) = result {
            eprintln!("Widget snapshot unavailable: {error}");
            return;
        }
        if changed
            || self
                .last_reload
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(900))
        {
            reload();
            self.last_reload = Some(Instant::now());
        }
    }
}

fn reload() {
    // Loaded only by the GUI; the independently installed root executable has no
    // dynamic dependency on this bundled Swift bridge and never opens it.
    let Some(path) = NSBundle::mainBundle().privateFrameworksPath() else {
        return;
    };
    let Ok(path) = std::ffi::CString::new(format!("{path}/FanControlWidgetBridge.dylib")) else {
        return;
    };
    static HANDLE: OnceLock<usize> = OnceLock::new();
    unsafe {
        let handle = *HANDLE.get_or_init(|| {
            libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) as usize
        }) as *mut libc::c_void;
        if handle.is_null() {
            return;
        }
        let symbol = libc::dlsym(handle, c"fan_control_reload_widgets".as_ptr());
        if !symbol.is_null() {
            let invoke: unsafe extern "C" fn() = std::mem::transmute(symbol);
            invoke();
        }
        // WidgetKit may enqueue work from Swift. Retain this library for process life.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_conversion_does_not_change_control_clock() {
        assert_eq!(timestamp(1000., 25.), Some(975.));
        assert_eq!(timestamp(1000., f64::NAN), None);
        assert_eq!(timestamp(1000., -1.), None);
    }
    #[test]
    fn absent_data_is_unknown_not_zero() {
        let mut source = Snapshot {
            sampled_at: 5.,
            fan_count: None,
            fans: vec![],
            sensors: vec![],
            thermal_pressure: fan_core::ThermalPressure::Nominal,
            cpu_utilization_percent: None,
            machine_id: None,
        };
        let data = display(&source, 5., 1000., "running", vec![]);
        assert_eq!(data.sampled_at, None);
        assert_eq!(data.temperature, None);
        assert_eq!(data.fan_count, None);
        source.fan_count = Some(0.);
        assert_eq!(
            display(&source, 5., 1000., "running", vec![]).fan_count,
            Some(0)
        );
        assert_eq!(source.sampled_at, 5.);
    }
    #[test]
    fn invalid_rpm_is_null_but_measured_stop_is_zero() {
        assert_eq!(finite_rpm(Some(0.)), Some(0.));
        assert_eq!(finite_rpm(Some(-1.)), None);
        assert_eq!(finite_rpm(Some(f64::INFINITY)), None);
    }
}
