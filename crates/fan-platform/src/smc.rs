use crate::{Error, Result};
#[path = "smc_sensors.rs"]
mod sensors;
use sensors::{sensor_name, SiliconGeneration};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub type SharedSmc = Arc<Mutex<Smc>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FanMode {
    #[default]
    Automatic,
    Forced,
}
impl Serialize for FanMode {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_u8(if *self == Self::Forced { 1 } else { 0 })
    }
}
impl<'de> Deserialize<'de> for FanMode {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            0 => Ok(Self::Automatic),
            1 => Ok(Self::Forced),
            _ => Err(serde::de::Error::custom("fan mode must be 0 or 1")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensorGroup {
    Cpu,
    Gpu,
    System,
    Other,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawSensor {
    pub key: String,
    pub name: String,
    pub group: SensorGroup,
    pub value: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawFan {
    pub id: u8,
    pub name: String,
    pub min_rpm: f64,
    pub max_rpm: f64,
    pub max_rpm_known: bool,
    pub current_rpm: f64,
    pub current_rpm_known: bool,
    pub mode: FanMode,
    pub mode_known: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RawSnapshot {
    pub fans: Vec<RawFan>,
    pub sensors: Vec<RawSensor>,
    pub captured_at_ms: u64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct KeyValue {
    pub data_type: String,
    pub size: u32,
    pub bytes: [u8; 32],
}
impl KeyValue {
    pub fn numeric(&self) -> Option<f64> {
        if self.size == 0 || self.size > 32 {
            return None;
        }
        let b = &self.bytes;
        let u = u16::from_be_bytes([b[0], b[1]]) as f64;
        let signed = i16::from_be_bytes([b[0], b[1]]) as f64;
        let value = match self.data_type.as_str() {
            "ui8 " if self.size >= 1 => b[0] as f64,
            "ui16" if self.size >= 2 => u,
            "ui32" if self.size >= 4 => u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64,
            "flt " if self.size >= 4 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            "fpe2" if self.size >= 2 => u / 4.0,
            // SMC 'sp' numbers are signed fixed point, including negative temperatures.
            t if t.starts_with("sp") && t.len() == 4 && self.size >= 2 => {
                let fractional = t.as_bytes()[3];
                let shift = (fractional as char).to_digit(16)?;
                signed / (1u32 << shift) as f64
            }
            t if t.starts_with("fp") && t.len() == 4 && self.size >= 2 => {
                let shift = (t.as_bytes()[3] as char).to_digit(16)?;
                u / (1u32 << shift) as f64
            }
            _ => return None,
        };
        value.is_finite().then_some(value)
    }
    fn set_rpm(&mut self, rpm: u16) -> Result<()> {
        match self.data_type.as_str() {
            "flt " if self.size == 4 => {
                self.bytes[..4].copy_from_slice(&(rpm as f32).to_le_bytes())
            }
            "fpe2" if self.size == 2 && rpm <= 16_383 => {
                self.bytes[..2].copy_from_slice(&(rpm << 2).to_be_bytes())
            }
            _ => {
                return Err(Error(format!(
                    "unsupported target encoding {}",
                    self.data_type
                )))
            }
        }
        Ok(())
    }
}

pub trait SmcTransport: Send {
    fn read(&mut self, key: &str) -> Result<KeyValue>;
    /// `None` means confirmed key absence; transport/read failures remain errors.
    fn read_optional(&mut self, key: &str) -> Result<Option<KeyValue>> {
        self.read(key).map(Some)
    }
    fn write(&mut self, key: &str, value: &KeyValue) -> Result<()>;
    fn keys(&mut self) -> Result<Vec<String>>;
}

/// Every hardware access is serialized by callers using `SharedSmc`.
/// Direct mode-write verifications on models without `Ftst`, about one second.
const DIRECT_UNLOCK_ATTEMPTS: usize = 20;

pub struct Smc {
    transport: Box<dyn SmcTransport>,
    forced: HashSet<u8>,
    known_fans: HashSet<u8>,
    unlock_attempts: HashMap<u8, Instant>,
    sensor_keys: Vec<(String, String, SensorGroup)>,
    silicon_generation: SiliconGeneration,
    retry_delay: Duration,
    cancellation_check: Option<Box<dyn Fn() -> bool + Send>>,
}
impl Smc {
    pub fn open() -> Result<Self> {
        let mut smc = Self::with_transport(Box::new(native::Connection::open()?));
        smc.silicon_generation = SiliconGeneration::read();
        Ok(smc)
    }
    pub fn with_transport(transport: Box<dyn SmcTransport>) -> Self {
        Self {
            transport,
            forced: HashSet::new(),
            known_fans: HashSet::new(),
            unlock_attempts: HashMap::new(),
            sensor_keys: Vec::new(),
            silicon_generation: SiliconGeneration::Unknown,
            retry_delay: Duration::from_millis(50),
            cancellation_check: None,
        }
    }
    pub fn into_shared(self) -> SharedSmc {
        Arc::new(Mutex::new(self))
    }
    /// Cancellation stops custom writes while automatic hand-back remains allowed.
    pub fn set_cancellation_check(&mut self, check: impl Fn() -> bool + Send + 'static) {
        self.cancellation_check = Some(Box::new(check));
    }
    fn check_control_active(&self) -> Result<()> {
        if self
            .cancellation_check
            .as_ref()
            .is_some_and(|check| check())
        {
            Err(Error("control cancelled".into()))
        } else {
            Ok(())
        }
    }
    fn wait_control_delay(&self, delay: Duration) -> Result<()> {
        let deadline = Instant::now() + delay;
        loop {
            self.check_control_active()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(20)));
        }
    }
    pub fn read_value(&mut self, key: &str) -> Result<f64> {
        self.transport
            .read(key)?
            .numeric()
            .ok_or_else(|| Error(format!("invalid numeric SMC value {key}")))
    }
    /// Read-only diagnostic access; all production writes remain validated above.
    pub fn read_key(&mut self, key: &str) -> Result<KeyValue> {
        self.transport.read(key)
    }
    fn fan_count(&mut self) -> Result<u8> {
        let count = valid_fan_count(self.read_value("FNum").ok())
            .ok_or_else(|| Error("invalid live fan count".into()))?;
        self.known_fans.extend(0..count);
        Ok(count)
    }
    fn check_id(&mut self, id: i64) -> Result<u8> {
        let count = self.fan_count()?;
        if id < 0 || id >= i64::from(count) {
            return Err(Error("invalid fan id".into()));
        }
        Ok(id as u8)
    }
    fn mode_key(&mut self, id: u8) -> Result<String> {
        for suffix in ["md", "Md"] {
            let key = format!("F{id}{suffix}");
            if self.transport.read_optional(&key)?.is_some() {
                return Ok(key);
            }
        }
        Err(Error(format!("fan {id} mode register unavailable")))
    }
    fn read_mode(&mut self, id: u8) -> Result<FanMode> {
        let key = self.mode_key(id)?;
        let raw = self.transport.read(&key)?;
        if raw.size != 1 || raw.data_type != "ui8 " {
            return Err(Error("unexpected fan mode encoding".into()));
        }
        match raw.bytes[0] {
            // 3 is thermalmonitord's system-controlled mode. Never treat it as forced.
            0 | 3 => Ok(FanMode::Automatic),
            1 => Ok(FanMode::Forced),
            _ => Err(Error("unknown hardware fan mode".into())),
        }
    }
    fn write_verified(&mut self, key: &str, value: &KeyValue, attempts: usize) -> Result<()> {
        self.write_verified_policy(key, value, attempts, false)
    }
    fn write_verified_control(
        &mut self,
        key: &str,
        value: &KeyValue,
        attempts: usize,
    ) -> Result<()> {
        self.write_verified_policy(key, value, attempts, true)
    }
    fn write_verified_policy(
        &mut self,
        key: &str,
        value: &KeyValue,
        attempts: usize,
        cancellable: bool,
    ) -> Result<()> {
        if value.size == 0 || value.size > 32 {
            return Err(Error("invalid write byte count".into()));
        }
        let mut last = Error(format!("write {key} failed"));
        for attempt in 0..attempts {
            if cancellable {
                self.check_control_active()?;
            }
            match self
                .transport
                .write(key, value)
                .and_then(|()| self.transport.read(key))
            {
                Ok(readback)
                    if readback.size == value.size
                        && readback.bytes[..value.size as usize]
                            == value.bytes[..value.size as usize] =>
                {
                    return Ok(())
                }
                Ok(_) => last = Error(format!("write {key} readback mismatch")),
                Err(error) => last = error,
            }
            if attempt + 1 < attempts {
                if cancellable {
                    self.wait_control_delay(self.retry_delay)?;
                } else {
                    std::thread::sleep(self.retry_delay);
                }
            }
        }
        Err(last)
    }
    fn unlock(&mut self, id: u8) -> Result<()> {
        let key = self.mode_key(id)?;
        let mut value = self.transport.read(&key)?;
        if value.size != 1 || value.data_type != "ui8 " {
            return Err(Error("invalid mode encoding".into()));
        }
        value.bytes[0] = 1;
        if self.write_verified_control(&key, &value, 1).is_ok() {
            return Ok(());
        }
        // Models without the test key unlock only through the direct mode
        // write, and firmware can take a moment to report forced mode.
        let Some(mut test) = self.transport.read_optional("Ftst")? else {
            return self.write_verified_control(&key, &value, DIRECT_UNLOCK_ATTEMPTS);
        };
        if test.size != 1 || test.data_type != "ui8 " {
            return Err(Error("invalid Ftst size".into()));
        }
        if test.bytes[0] != 1 {
            test.bytes[0] = 1;
            self.write_verified_control("Ftst", &test, 40)?;
            if !self.retry_delay.is_zero() {
                self.wait_control_delay(Duration::from_secs(3))?;
            }
        }
        self.write_verified_control(&key, &value, 120)
    }
    pub fn set_fan_mode(&mut self, id: i64, mode: FanMode) -> Result<()> {
        let id = self.check_id(id)?;
        if mode == FanMode::Forced {
            let result = (|| {
                self.check_control_active()?;
                // Never enter forced mode with a stale, out-of-range target.
                let min = self.read_value(&format!("F{id}Mn"))?;
                let max = self.read_value(&format!("F{id}Mx"))?;
                let target = self.read_value(&format!("F{id}Tg"))?;
                if !valid_bounds(min, max) || target < min || target > max {
                    return Err(Error("unsafe existing fan target; set RPM instead".into()));
                }
                self.unlock(id)
            })();
            if result.is_ok() {
                self.forced.insert(id);
            } else {
                let _ = self.automatic(id);
            }
            return result;
        }
        self.automatic(id)
    }
    fn automatic(&mut self, id: u8) -> Result<()> {
        let key = self.mode_key(id)?;
        let mut value = self.transport.read(&key)?;
        if value.size != 1 || value.data_type != "ui8 " {
            return Err(Error("invalid mode encoding".into()));
        }
        let initial_mode = value.bytes[0];
        if initial_mode != 0 && initial_mode != 3 {
            value.bytes[0] = 0;
            let mut result = Err(Error("automatic mode write failed".into()));
            for attempt in 0..10 {
                result = self.transport.write(&key, &value).and_then(|()| {
                    if self.read_mode(id)? == FanMode::Automatic {
                        Ok(())
                    } else {
                        Err(Error("fan remained forced".into()))
                    }
                });
                if result.is_ok() {
                    break;
                }
                if attempt < 9 {
                    std::thread::sleep(self.retry_delay);
                }
            }
            result?;
        }
        if self.read_mode(id)? != FanMode::Automatic {
            return Err(Error("fan remained forced".into()));
        }
        self.forced.remove(&id);
        // System mode already owns its target and may reject writes. Preserve it.
        if initial_mode == 3 {
            return self.release_test_if_all_automatic();
        }
        // A failed target clear must never undo the verified automatic hand-back.
        let target_key = format!("F{id}Tg");
        if let Ok(mut target) = self.transport.read(&target_key) {
            if let Err(error) = target
                .set_rpm(0)
                .and_then(|()| self.write_verified(&target_key, &target, 10))
            {
                eprintln!("[FanControlHelper] fan {id} is verified automatic; optional target cleanup failed: {error}");
            }
        }
        self.release_test_if_all_automatic()
    }
    fn release_test_if_all_automatic(&mut self) -> Result<()> {
        let count = self.fan_count()?;
        for id in 0..count {
            if self.read_mode(id)? != FanMode::Automatic {
                return Ok(());
            }
        }
        if let Some(mut value) = self.transport.read_optional("Ftst")? {
            if value.size != 1 || value.data_type != "ui8 " {
                return Err(Error("invalid Ftst encoding".into()));
            }
            if value.bytes[0] != 0 {
                value.bytes[0] = 0;
                self.write_verified("Ftst", &value, 10)?;
            }
        }
        Ok(())
    }
    pub fn set_fan_rpm(&mut self, id: i64, rpm: i64) -> Result<u16> {
        let id = self.check_id(id)?;
        let result = self.set_rpm_inner(id, rpm);
        if let Err(error) = &result {
            if let Err(fallback) = self.automatic(id) {
                return Err(Error(format!(
                    "{error}; automatic fallback failed: {fallback}"
                )));
            }
        }
        result
    }
    fn set_rpm_inner(&mut self, id: u8, rpm: i64) -> Result<u16> {
        self.check_control_active()?;
        let min = self.read_value(&format!("F{id}Mn"))?;
        let max = self.read_value(&format!("F{id}Mx"))?;
        let rpm = validated_rpm(rpm, min, max)
            .ok_or_else(|| Error("invalid RPM or live hardware bounds".into()))?;
        let hardware_mode = self.read_mode(id)?;
        if hardware_mode != FanMode::Forced {
            if self.forced.remove(&id) {
                self.unlock_attempts.remove(&id);
            }
            if self
                .unlock_attempts
                .get(&id)
                .is_some_and(|last| last.elapsed() < Duration::from_secs(30))
            {
                return Err(Error(
                    "fan unlock cooling down; system control retained".into(),
                ));
            }
            self.unlock_attempts.insert(id, Instant::now());
            self.unlock(id)?;
        }
        self.forced.insert(id);
        let key = format!("F{id}Tg");
        let mut value = self.transport.read(&key)?;
        value.set_rpm(rpm)?;
        self.write_verified_control(&key, &value, 10)?;
        if self.read_mode(id)? != FanMode::Forced {
            return Err(Error("hardware reset fan mode during write".into()));
        }
        self.check_control_active()?;
        Ok(rpm)
    }
    pub fn reset_all(&mut self) -> Result<()> {
        let mut errors = Vec::new();
        let live_count = self.fan_count();
        if let Err(error) = &live_count {
            errors.push(error.to_string());
        }
        let mut ids: Vec<u8> = self.known_fans.union(&self.forced).copied().collect();
        ids.sort();
        // Per-fan mode verification is required even when Ftst is available.
        for &id in &ids {
            if let Err(error) = self.automatic(id) {
                errors.push(format!("fan {id}: {error}"));
            }
        }
        match self.transport.read_optional("Ftst") {
            Ok(Some(mut value)) if value.size == 1 && value.data_type == "ui8 " => {
                if value.bytes[0] != 0 {
                    value.bytes[0] = 0;
                    if let Err(error) = self.write_verified("Ftst", &value, 10) {
                        errors.push(error.to_string());
                    }
                }
            }
            Ok(Some(_)) => errors.push("invalid Ftst encoding".into()),
            Ok(None) => {}
            Err(error) => errors.push(format!("Ftst verification failed: {error}")),
        }
        // The global Ftst cleanup may change firmware state; verify every
        // previously established fan again before attesting final hand-back.
        for id in ids {
            match self.read_mode(id) {
                Ok(FanMode::Automatic) => {}
                Ok(FanMode::Forced) => errors.push(format!("fan {id} is still forced after reset")),
                Err(error) => {
                    errors.push(format!("fan {id} final mode verification failed: {error}"))
                }
            }
        }
        self.unlock_attempts.clear();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error(errors.join("; ")))
        }
    }
    pub fn discover_snapshot(&mut self) -> Result<RawSnapshot> {
        let mut keys = self.transport.keys()?;
        keys.sort();
        keys.dedup();
        self.sensor_keys = keys
            .into_iter()
            .filter(|key| key.starts_with('T'))
            .map(|key| {
                let (name, group) = sensor_name(&key, self.silicon_generation);
                (key, name, group)
            })
            .collect();
        self.refresh_snapshot()
    }
    pub fn refresh_snapshot(&mut self) -> Result<RawSnapshot> {
        let mut snapshot = RawSnapshot {
            captured_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            ..Default::default()
        };
        let count = self.fan_count()?;
        for id in 0..count {
            let min = self.read_value(&format!("F{id}Mn")).ok();
            let max = self.read_value(&format!("F{id}Mx")).ok();
            let known = min
                .zip(max)
                .is_some_and(|(min, max)| valid_bounds(min, max));
            let lower = min
                .filter(|x| x.is_finite() && *x >= 0.0 && *x <= 16_383.0)
                .unwrap_or(0.0);
            let upper = if known { max.unwrap() } else { lower + 1.0 };
            let current = self
                .read_value(&format!("F{id}Ac"))
                .ok()
                .filter(|value| *value >= 0.0 && *value <= 16_383.0);
            let mode = self.read_mode(id).ok();
            let name = self
                .transport
                .read(&format!("F{id}ID"))
                .ok()
                .filter(|v| v.data_type == "{fds" && v.size >= 16)
                .map(|v| {
                    String::from_utf8_lossy(&v.bytes[4..16])
                        .trim_matches([' ', '\0'])
                        .to_owned()
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if count == 2 {
                        if id == 0 {
                            "左侧风扇".into()
                        } else {
                            "右侧风扇".into()
                        }
                    } else {
                        format!("风扇 {}", id + 1)
                    }
                });
            snapshot.fans.push(RawFan {
                id,
                name,
                min_rpm: lower,
                max_rpm: upper,
                max_rpm_known: known,
                current_rpm: current.unwrap_or(0.0),
                current_rpm_known: current.is_some(),
                mode: mode.unwrap_or_default(),
                mode_known: mode.is_some(),
            });
            if !known || current.is_none() || mode.is_none() {
                snapshot
                    .warnings
                    .push(format!("风扇 {} 的硬件读数不完整，手动控制不可用", id + 1));
            }
        }
        // Invalid reads are absent from this snapshot; old temperatures never become fresh evidence.
        for (key, name, group) in self.sensor_keys.clone() {
            if let Ok(value) = self.read_value(&key) {
                // Firmware may expose inactive sensor slots as exactly 1°C.
                if value > 1.0 && value < 120.0 {
                    snapshot.sensors.push(RawSensor {
                        key,
                        name,
                        group,
                        value,
                    });
                }
            }
        }
        snapshot.sensors.sort_by(|a, b| a.name.cmp(&b.name));
        if snapshot.sensors.is_empty() {
            snapshot
                .warnings
                .push("未读取到有效温度，自动策略应交还系统".into());
        }
        Ok(snapshot)
    }
}

pub fn valid_fan_count(value: Option<f64>) -> Option<u8> {
    let value = value?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    Some(value.min(10.0) as u8)
}
fn valid_bounds(min: f64, max: f64) -> bool {
    min.is_finite()
        && max.is_finite()
        && min >= 0.0
        && min < max
        && min.ceil() <= max.floor()
        && max <= 16_383.0
}
pub fn validated_rpm(request: i64, min: f64, max: f64) -> Option<u16> {
    if request < 0 || !valid_bounds(min, max) {
        return None;
    }
    // Fan stop is supported only when the hardware explicitly exposes minimum 0.
    let requested = (request as f64).clamp(min.ceil(), max.floor());
    if !(0.0..=16_383.0).contains(&requested) {
        return None;
    }
    Some(requested as u16)
}

#[repr(C)]
#[derive(Default)]
struct SmcVersion {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}
#[repr(C)]
#[derive(Default)]
struct SmcLimit {
    version: u16,
    length: u16,
    cpu: u32,
    gpu: u32,
    memory: u32,
}
// Swift's KeyInfo has a 9-byte size (12-byte stride). The driver packet
// places its following UInt16 at offset 38. Packed KeyInfo plus explicit
// outer padding preserves the existing 80-byte wire ABI instead of C's 84.
#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
struct SmcKeyInfo {
    size: u32,
    data_type: u32,
    attributes: u8,
}
#[repr(C)]
#[derive(Default)]
struct SmcKeyData {
    key: u32,
    version: SmcVersion,
    limit: SmcLimit,
    info: SmcKeyInfo,
    padding: u16,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}
fn fourcc(key: &str) -> Result<u32> {
    let bytes: [u8; 4] = key
        .as_bytes()
        .try_into()
        .map_err(|_| Error("SMC key must be exactly four bytes".into()))?;
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(any(target_os = "macos", test))]
fn checked_smc_response(
    status: i32,
    length: usize,
    request: u8,
    output: SmcKeyData,
) -> Result<Option<SmcKeyData>> {
    if status != 0 || length != std::mem::size_of::<SmcKeyData>() {
        return Err(Error(format!(
            "SMC call failed: kernel={status:#x} result={} bytes={length}",
            output.result
        )));
    }
    match output.result {
        0 => Ok(Some(output)),
        // Only a successful getKeyInfo transaction can attest key absence.
        0x84 if request == 9 => Ok(None),
        result => Err(Error(format!(
            "SMC request {request} failed: result={result}"
        ))),
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::ffi::{c_char, c_void};
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingServices(
            port: u32,
            matching: *mut c_void,
            iterator: *mut u32,
        ) -> i32;
        fn IOIteratorNext(iterator: u32) -> u32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IOServiceOpen(service: u32, task: u32, kind: u32, connection: *mut u32) -> i32;
        fn IOServiceClose(connection: u32) -> i32;
        fn IOConnectCallStructMethod(
            connection: u32,
            selector: u32,
            input: *const c_void,
            input_size: usize,
            output: *mut c_void,
            output_size: *mut usize,
        ) -> i32;
        static mach_task_self_: u32;
    }
    pub struct Connection {
        handle: u32,
        info: HashMap<String, SmcKeyInfo>,
    }
    impl Connection {
        pub fn open() -> Result<Self> {
            unsafe {
                let matching = IOServiceMatching(c"AppleSMC".as_ptr());
                if matching.is_null() {
                    return Err(Error("AppleSMC matching failed".into()));
                }
                let mut iterator = 0;
                let status = IOServiceGetMatchingServices(0, matching, &mut iterator);
                if status != 0 {
                    return Err(Error(format!("AppleSMC enumeration failed: {status:#x}")));
                }
                let service = IOIteratorNext(iterator);
                IOObjectRelease(iterator);
                if service == 0 {
                    return Err(Error("AppleSMC unavailable on this Mac".into()));
                }
                let mut handle = 0;
                let status = IOServiceOpen(service, mach_task_self_, 0, &mut handle);
                IOObjectRelease(service);
                if status != 0 {
                    return Err(Error(format!("AppleSMC open failed: {status:#x}")));
                }
                Ok(Self {
                    handle,
                    info: HashMap::new(),
                })
            }
        }
        fn call_packet(&self, input: &SmcKeyData) -> Result<Option<SmcKeyData>> {
            let mut output = SmcKeyData::default();
            let mut length = std::mem::size_of::<SmcKeyData>();
            let status = unsafe {
                IOConnectCallStructMethod(
                    self.handle,
                    2,
                    input as *const _ as _,
                    length,
                    &mut output as *mut _ as _,
                    &mut length,
                )
            };
            checked_smc_response(status, length, input.data8, output)
        }
        fn call(&self, input: &SmcKeyData) -> Result<SmcKeyData> {
            self.call_packet(input)?
                .ok_or_else(|| Error("SMC key is absent".into()))
        }
        fn read_optional_key(&mut self, key: &str) -> Result<Option<KeyValue>> {
            let mut input = SmcKeyData {
                key: fourcc(key)?,
                ..Default::default()
            };
            let info = if let Some(info) = self.info.get(key) {
                *info
            } else {
                input.data8 = 9;
                let Some(output) = self.call_packet(&input)? else {
                    return Ok(None);
                };
                let info = output.info;
                if info.size == 0 || info.size > 32 {
                    return Err(Error(format!("invalid SMC key size for {key}")));
                }
                self.info.insert(key.into(), info);
                info
            };
            input.info = info;
            input.data8 = 5;
            match self.call(&input) {
                Ok(output) => Ok(Some(KeyValue {
                    data_type: String::from_utf8_lossy(&info.data_type.to_be_bytes()).to_string(),
                    size: info.size,
                    bytes: output.bytes,
                })),
                Err(error) => {
                    self.info.remove(key);
                    Err(error)
                }
            }
        }
    }
    impl Drop for Connection {
        fn drop(&mut self) {
            unsafe {
                IOServiceClose(self.handle);
            }
        }
    }
    impl SmcTransport for Connection {
        fn read(&mut self, key: &str) -> Result<KeyValue> {
            self.read_optional_key(key)?
                .ok_or_else(|| Error(format!("SMC key {key} is absent")))
        }
        fn read_optional(&mut self, key: &str) -> Result<Option<KeyValue>> {
            self.read_optional_key(key)
        }
        fn write(&mut self, key: &str, value: &KeyValue) -> Result<()> {
            if value.size == 0 || value.size > 32 {
                return Err(Error("invalid SMC write size".into()));
            }
            let input = SmcKeyData {
                key: fourcc(key)?,
                info: SmcKeyInfo {
                    size: value.size,
                    ..Default::default()
                },
                data8: 6,
                bytes: value.bytes,
                ..Default::default()
            };
            self.call(&input).map(|_| ())
        }
        fn keys(&mut self) -> Result<Vec<String>> {
            let count = self
                .read("#KEY")?
                .numeric()
                .ok_or_else(|| Error("invalid key count".into()))?;
            if count < 0.0 || count.fract() != 0.0 || count > 65_536.0 {
                return Err(Error("invalid SMC key enumeration bound".into()));
            }
            let mut keys = Vec::new();
            for index in 0..count as u32 {
                let input = SmcKeyData {
                    data8: 8,
                    data32: index,
                    ..Default::default()
                };
                if let Ok(output) = self.call(&input) {
                    if let Ok(key) = String::from_utf8(output.key.to_be_bytes().to_vec()) {
                        keys.push(key);
                    }
                }
            }
            Ok(keys)
        }
    }
}
#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub struct Connection;
    impl Connection {
        pub fn open() -> Result<Self> {
            Err(Error("SMC requires macOS Apple Silicon".into()))
        }
    }
    impl SmcTransport for Connection {
        fn read(&mut self, _: &str) -> Result<KeyValue> {
            Err(Error("unsupported platform".into()))
        }
        fn write(&mut self, _: &str, _: &KeyValue) -> Result<()> {
            Err(Error("unsupported platform".into()))
        }
        fn keys(&mut self) -> Result<Vec<String>> {
            Err(Error("unsupported platform".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};
    #[test]
    fn smc_abi_matches_apple_driver_struct() {
        assert_eq!(size_of::<SmcVersion>(), 6);
        assert_eq!(size_of::<SmcLimit>(), 16);
        assert_eq!(size_of::<SmcKeyInfo>(), 9);
        assert_eq!(size_of::<SmcKeyData>(), 80);
        assert_eq!(align_of::<SmcKeyData>(), 4);
        assert_eq!(offset_of!(SmcKeyData, version), 4);
        assert_eq!(offset_of!(SmcKeyData, limit), 12);
        assert_eq!(offset_of!(SmcKeyData, info), 28);
        assert_eq!(offset_of!(SmcKeyData, data32), 44);
        assert_eq!(offset_of!(SmcKeyData, bytes), 48);
    }
    fn value(kind: &str, bytes: &[u8]) -> KeyValue {
        let mut value = KeyValue {
            data_type: kind.into(),
            size: bytes.len() as u32,
            bytes: [0; 32],
        };
        value.bytes[..bytes.len()].copy_from_slice(bytes);
        value
    }
    #[derive(Default)]
    struct MockState {
        keys: HashMap<String, KeyValue>,
        writes: Vec<String>,
        reject_read: HashSet<String>,
        reject: HashSet<String>,
        ignored: HashSet<String>,
        cancel_on_write: Option<(String, Arc<std::sync::atomic::AtomicBool>)>,
        force_after_test_release: bool,
        /// Mode writes that firmware accepts before reporting the new mode.
        mode_lag: usize,
    }
    struct Mock(Arc<Mutex<MockState>>);
    impl SmcTransport for Mock {
        fn read(&mut self, key: &str) -> Result<KeyValue> {
            self.read_optional(key)?
                .ok_or_else(|| Error(format!("missing {key}")))
        }
        fn read_optional(&mut self, key: &str) -> Result<Option<KeyValue>> {
            let state = self.0.lock().unwrap();
            if state.reject_read.contains(key) {
                return Err(Error(format!("transient read failure: {key}")));
            }
            Ok(state.keys.get(key).cloned())
        }
        fn write(&mut self, key: &str, value: &KeyValue) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            state.writes.push(key.into());
            if state.reject.contains(key) {
                return Err(Error("mock rejected".into()));
            }
            if key == "F0md" && state.mode_lag > 0 {
                state.mode_lag -= 1;
                return Ok(());
            }
            if !state.ignored.contains(key) {
                state.keys.insert(key.into(), value.clone());
            }
            if key == "Ftst" && value.bytes[0] == 0 && state.force_after_test_release {
                state.keys.get_mut("F0md").unwrap().bytes[0] = 1;
            }
            if let Some((cancel_key, token)) = &state.cancel_on_write {
                if cancel_key == key {
                    token.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            Ok(())
        }
        fn keys(&mut self) -> Result<Vec<String>> {
            Ok(self.0.lock().unwrap().keys.keys().cloned().collect())
        }
    }
    fn setup() -> (Smc, Arc<Mutex<MockState>>) {
        let state = Arc::new(Mutex::new(MockState::default()));
        {
            let mut s = state.lock().unwrap();
            for (key, val) in [
                ("FNum", value("ui8 ", &[1])),
                ("F0md", value("ui8 ", &[0])),
                ("F0Mn", value("flt ", &1200f32.to_le_bytes())),
                ("F0Mx", value("flt ", &6000f32.to_le_bytes())),
                ("F0Tg", value("flt ", &2000f32.to_le_bytes())),
                ("F0Ac", value("flt ", &1800f32.to_le_bytes())),
                ("TC0P", value("sp78", &0x3200u16.to_be_bytes())),
            ] {
                s.keys.insert(key.into(), val);
            }
        }
        let mut smc = Smc::with_transport(Box::new(Mock(state.clone())));
        smc.retry_delay = Duration::ZERO;
        (smc, state)
    }
    #[test]
    #[ignore = "opt-in, read-only physical temperature probe; no writes"]
    fn native_silicon_sensor_readonly_probe() {
        let mut smc = Smc::open().unwrap();
        let snapshot = smc.discover_snapshot().unwrap();
        let cpu_count = snapshot
            .sensors
            .iter()
            .filter(|sensor| sensor.group == SensorGroup::Cpu)
            .count();
        let gpu_count = snapshot
            .sensors
            .iter()
            .filter(|sensor| sensor.group == SensorGroup::Gpu)
            .count();
        assert_ne!(smc.silicon_generation, SiliconGeneration::Unknown);
        assert!(cpu_count > 0 && gpu_count > 0);
        println!(
            "chip={:?}, fans={}, CPU readings={}, GPU readings={}",
            smc.silicon_generation,
            snapshot.fans.len(),
            cpu_count,
            gpu_count
        );
    }

    #[test]
    fn mac_family_topologies_read_temperatures_without_inventing_air_fans() {
        // Mini, Studio, Pro and Air differ in physical fan count; telemetry must
        // use the chip profile and actual keys, without requiring a fan register.
        for count in [0, 1, 2] {
            for (chip, cpu_key, gpu_key) in [
                (SiliconGeneration::M1, "Tp01", "Tg05"),
                (SiliconGeneration::M2, "Tp1h", "Tg0f"),
                (SiliconGeneration::M3, "Te05", "Tf14"),
                (SiliconGeneration::M4, "Te09", "Tg0G"),
                (SiliconGeneration::M5, "Tp00", "Tg0U"),
            ] {
                let (mut smc, state) = setup();
                smc.silicon_generation = chip;
                {
                    let mut mock = state.lock().unwrap();
                    mock.keys.insert("FNum".into(), value("ui8 ", &[count]));
                    for key in [cpu_key, gpu_key, "TCMb", "TCMz"] {
                        mock.keys
                            .insert(key.into(), value("flt ", &80f32.to_le_bytes()));
                    }
                }
                let snapshot = smc.discover_snapshot().unwrap();
                assert_eq!(snapshot.fans.len(), usize::from(count));
                for (key, group) in [
                    (cpu_key, SensorGroup::Cpu),
                    (gpu_key, SensorGroup::Gpu),
                    ("TCMb", SensorGroup::Cpu),
                    ("TCMz", SensorGroup::Cpu),
                ] {
                    assert_eq!(
                        snapshot
                            .sensors
                            .iter()
                            .find(|sensor| sensor.key == key)
                            .unwrap()
                            .group,
                        group
                    );
                }
                assert!(state.lock().unwrap().writes.is_empty());
                if count == 0 {
                    assert!(smc.set_fan_rpm(0, 2000).is_err());
                    assert!(state.lock().unwrap().writes.is_empty());
                }
            }
        }
    }

    #[test]
    fn m4_mini_diagnostic_hotspot_enters_cpu_telemetry_and_placeholders_are_absent() {
        let readings: Vec<(String, f64)> =
            serde_json::from_str(include_str!("../tests/fixtures/m4-mini-temperatures.json"))
                .unwrap();
        let (mut smc, state) = setup();
        smc.silicon_generation = SiliconGeneration::M4;
        {
            let mut mock = state.lock().unwrap();
            mock.keys.retain(|key, _| !key.starts_with('T'));
            for (key, temperature) in readings {
                mock.keys
                    .insert(key, value("flt ", &(temperature as f32).to_le_bytes()));
            }
        }
        let snapshot = smc.discover_snapshot().unwrap();
        let hotspot = snapshot
            .sensors
            .iter()
            .find(|sensor| sensor.key == "TCMz")
            .unwrap();
        assert_eq!(hotspot.group, SensorGroup::Cpu);
        assert!((hotspot.value - 81.828).abs() < 0.01);
        assert_eq!(
            snapshot
                .sensors
                .iter()
                .find(|sensor| sensor.key == "TCMb")
                .unwrap()
                .group,
            SensorGroup::Cpu
        );
        assert!(snapshot.sensors.iter().all(|sensor| sensor.value > 1.0));
        assert!(snapshot
            .sensors
            .iter()
            .filter(|sensor| sensor.key.starts_with("Ta0"))
            .all(|sensor| sensor.group == SensorGroup::Other));
        assert!(state.lock().unwrap().writes.is_empty());
        state.lock().unwrap().keys.remove("TCMz");
        assert!(smc
            .refresh_snapshot()
            .unwrap()
            .sensors
            .iter()
            .all(|sensor| sensor.key != "TCMz"));
    }

    #[test]
    fn fixed_point_is_signed_and_float_rejects_nan() {
        assert_eq!(
            value("sp78", &0xff00u16.to_be_bytes()).numeric(),
            Some(-1.0)
        );
        assert_eq!(
            value("fpe2", &8000u16.to_be_bytes()).numeric(),
            Some(2000.0)
        );
        assert_eq!(value("flt ", &f32::NAN.to_le_bytes()).numeric(), None);
    }
    #[test]
    fn optional_absence_requires_successful_key_info_transaction() {
        let response = || SmcKeyData {
            result: 0x84,
            ..Default::default()
        };
        let packet_size = std::mem::size_of::<SmcKeyData>();
        assert!(checked_smc_response(0, packet_size, 9, response())
            .unwrap()
            .is_none());
        assert!(checked_smc_response(-1, packet_size, 9, response()).is_err());
        assert!(checked_smc_response(0, packet_size - 1, 9, response()).is_err());
        assert!(checked_smc_response(0, packet_size, 5, response()).is_err());
        assert!(
            checked_smc_response(0, packet_size, 9, SmcKeyData::default())
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn transient_mode_read_does_not_fall_through_to_another_register() {
        let (mut smc, state) = setup();
        {
            let mut s = state.lock().unwrap();
            s.keys.insert("F0Md".into(), value("ui8 ", &[3]));
            s.reject_read.insert("F0md".into());
        }
        assert!(smc.read_mode(0).is_err());
        assert!(smc.set_fan_rpm(0, 2500).is_err());
        assert!(state.lock().unwrap().writes.is_empty());
        state.lock().unwrap().reject_read.clear();
        state.lock().unwrap().keys.remove("F0md");
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Automatic);
    }
    #[test]
    fn reset_retains_failure_until_global_unlock_is_verified() {
        let (mut smc, state) = setup();
        {
            let mut s = state.lock().unwrap();
            s.keys.insert("F0md".into(), value("ui8 ", &[3]));
            s.keys.insert("Ftst".into(), value("ui8 ", &[1]));
            s.reject_read.insert("Ftst".into());
        }
        assert!(smc.set_fan_mode(0, FanMode::Automatic).is_err());
        assert!(smc.reset_all().unwrap_err().to_string().contains("Ftst"));
        assert_eq!(state.lock().unwrap().keys["Ftst"].bytes[0], 1);
        state.lock().unwrap().reject_read.clear();
        smc.reset_all().unwrap();
        assert_eq!(smc.read_value("Ftst").unwrap(), 0.0);
        state.lock().unwrap().keys.remove("Ftst");
        smc.reset_all().unwrap();
    }
    #[test]
    fn reset_rejects_invalid_global_unlock_encoding() {
        let (mut smc, state) = setup();
        state
            .lock()
            .unwrap()
            .keys
            .insert("Ftst".into(), value("ui16", &[0, 1]));
        assert!(smc
            .reset_all()
            .unwrap_err()
            .to_string()
            .contains("invalid Ftst encoding"));
    }
    #[test]
    fn live_bounds_and_ids_fail_closed() {
        for count in [None, Some(f64::NAN), Some(-1.0), Some(1.5)] {
            assert_eq!(valid_fan_count(count), None);
        }
        assert_eq!(valid_fan_count(Some(255.0)), Some(10));
        assert_eq!(validated_rpm(-1, 1000.0, 6000.0), None);
        assert_eq!(validated_rpm(0, 1000.0, 6000.0), Some(1000));
        assert_eq!(validated_rpm(100, f64::NAN, 6000.0), None);
        assert_eq!(validated_rpm(9000, 1000.0, 6000.0), Some(6000));
        let (mut smc, state) = setup();
        assert!(smc.set_fan_rpm(10, 2000).is_err());
        assert!(state.lock().unwrap().writes.is_empty());
    }
    #[test]
    fn wake_hardware_reset_overrides_cache_and_cooldown() {
        let (mut smc, state) = setup();
        assert_eq!(smc.set_fan_rpm(0, 2500).unwrap(), 2500);
        state
            .lock()
            .unwrap()
            .keys
            .insert("F0md".into(), value("ui8 ", &[0]));
        assert_eq!(smc.set_fan_rpm(0, 2700).unwrap(), 2700);
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Forced);
    }
    #[test]
    fn target_failure_returns_hardware_to_automatic() {
        let (mut smc, state) = setup();
        state.lock().unwrap().reject.insert("F0Tg".into());
        assert!(smc.set_fan_rpm(0, 2500).is_err());
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Automatic);
    }
    #[test]
    fn models_without_test_key_wait_for_slow_mode_readback() {
        let (mut smc, state) = setup();
        state.lock().unwrap().mode_lag = 3;
        assert_eq!(smc.set_fan_rpm(0, 2500).unwrap(), 2500);
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Forced);
        assert!(!state.lock().unwrap().writes.iter().any(|key| key == "Ftst"));

        let (mut smc, state) = setup();
        state.lock().unwrap().mode_lag = usize::MAX;
        let error = smc.set_fan_rpm(0, 2500).unwrap_err().to_string();
        assert!(!error.contains("Ftst"), "{error}");
        assert!(!state.lock().unwrap().writes.iter().any(|key| key == "Ftst"));
    }
    #[test]
    fn failed_target_releases_global_test_unlock_after_verified_handback() {
        let (mut smc, state) = setup();
        {
            let mut s = state.lock().unwrap();
            s.keys.insert("Ftst".into(), value("ui8 ", &[1]));
            s.reject.insert("F0Tg".into());
        }
        assert!(smc.set_fan_rpm(0, 2500).is_err());
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Automatic);
        assert_eq!(smc.read_value("Ftst").unwrap(), 0.0);
    }
    #[test]
    fn shutdown_during_unlock_stops_control_and_releases_test_mode() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (mut smc, state) = setup();
        let token = Arc::new(AtomicBool::new(false));
        let check = token.clone();
        smc.set_cancellation_check(move || check.load(Ordering::SeqCst));
        {
            let mut s = state.lock().unwrap();
            s.keys.insert("F0md".into(), value("ui8 ", &[3]));
            s.keys.insert("Ftst".into(), value("ui8 ", &[0]));
            s.reject.insert("F0md".into());
            s.cancel_on_write = Some(("Ftst".into(), token));
        }
        assert!(smc.set_fan_rpm(0, 2500).is_err());
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Automatic);
        assert_eq!(smc.read_value("Ftst").unwrap(), 0.0);
        assert!(state.lock().unwrap().writes.iter().all(|key| key != "F0Tg"));
    }
    #[test]
    fn shutdown_after_target_write_still_verifies_automatic_handback() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (mut smc, state) = setup();
        let token = Arc::new(AtomicBool::new(false));
        let check = token.clone();
        smc.set_cancellation_check(move || check.load(Ordering::SeqCst));
        state.lock().unwrap().cancel_on_write = Some(("F0Tg".into(), token));
        assert!(smc.set_fan_rpm(0, 2500).is_err());
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Automatic);
        assert_eq!(smc.read_value("F0Tg").unwrap(), 0.0);
    }
    #[test]
    fn reset_attests_final_mode_after_global_unlock_cleanup() {
        let (mut smc, state) = setup();
        {
            let mut s = state.lock().unwrap();
            s.keys.insert("F0md".into(), value("ui8 ", &[3]));
            s.keys.insert("Ftst".into(), value("ui8 ", &[1]));
            s.force_after_test_release = true;
        }
        assert!(smc
            .reset_all()
            .unwrap_err()
            .to_string()
            .contains("still forced after reset"));
    }
    #[test]
    fn system_mode_three_is_automatic_and_no_register_write_is_needed() {
        let (mut smc, state) = setup();
        state
            .lock()
            .unwrap()
            .keys
            .insert("F0md".into(), value("ui8 ", &[3]));
        let snapshot = smc.discover_snapshot().unwrap();
        assert!(snapshot.fans[0].mode_known);
        assert_eq!(snapshot.fans[0].mode, FanMode::Automatic);
        assert!(smc.set_fan_mode(0, FanMode::Automatic).is_ok());
        assert!(state.lock().unwrap().writes.is_empty());
    }
    #[test]
    fn corrupt_live_count_still_hands_previously_verified_fans_back() {
        let (mut smc, state) = setup();
        smc.set_fan_rpm(0, 2500).unwrap();
        state.lock().unwrap().keys.remove("FNum");
        assert!(smc.reset_all().is_err());
        assert_eq!(smc.read_mode(0).unwrap(), FanMode::Automatic);
    }
    #[test]
    fn unconfirmed_automatic_never_clears_target() {
        let (mut smc, state) = setup();
        let mut s = state.lock().unwrap();
        s.keys.insert("F0md".into(), value("ui8 ", &[1]));
        s.ignored.insert("F0md".into());
        drop(s);
        assert!(smc.set_fan_mode(0, FanMode::Automatic).is_err());
        assert!(state.lock().unwrap().writes.iter().all(|key| key != "F0Tg"));
    }
    #[test]
    fn stale_sensor_is_absent_and_missing_bounds_disable_control() {
        let (mut smc, state) = setup();
        assert_eq!(smc.discover_snapshot().unwrap().sensors.len(), 1);
        {
            let mut s = state.lock().unwrap();
            s.keys.remove("TC0P");
            s.keys.remove("F0Mx");
        }
        let snapshot = smc.refresh_snapshot().unwrap();
        assert!(snapshot.sensors.is_empty());
        assert!(!snapshot.fans[0].max_rpm_known);
    }
}
