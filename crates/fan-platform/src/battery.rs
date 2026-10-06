//! Read-only battery telemetry. Missing values remain unknown; no battery control APIs.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BatteryReading {
    pub charge_percent: Option<f64>,
    pub is_charging: Option<bool>,
    pub is_on_ac: Option<bool>,
    pub cycle_count: Option<u32>,
    pub health_percent: Option<f64>,
    /// Signed battery power: positive charging, negative discharging.
    pub power_watts: Option<f64>,
    /// Attached adapter's reported/derived rating; absent when AC is unknown/off.
    pub adapter_watts: Option<f64>,
    /// A capacity ratio is an estimate and can differ from System Settings.
    pub health_is_estimate: bool,
}

pub fn read_battery() -> Result<Option<BatteryReading>> {
    native::read()
}

fn capacity_percent(current: Option<f64>, maximum: Option<f64>) -> Option<f64> {
    let (current, maximum) = current.zip(maximum)?;
    if !current.is_finite()
        || !maximum.is_finite()
        || current < 0.0
        || maximum <= 0.0
        || current > maximum
    {
        return None;
    }
    Some(current / maximum * 100.0)
}
fn cycle_count(value: Option<f64>) -> Option<u32> {
    value
        .filter(|value| {
            value.is_finite() && *value >= 0.0 && *value <= u32::MAX as f64 && value.fract() == 0.0
        })
        .map(|value| value as u32)
}
fn estimated_health(capacity: Option<f64>, design: Option<f64>) -> Option<f64> {
    let (capacity, design) = capacity.zip(design)?;
    if !capacity.is_finite()
        || !design.is_finite()
        || capacity <= 0.0
        || design <= 0.0
        || capacity > 1_000_000.0
        || design > 1_000_000.0
    {
        return None;
    }
    let ratio = capacity / design * 100.0;
    // Fresh batteries can exceed nameplate capacity; impossible readings stay absent.
    if ratio > 120.0 {
        return None;
    }
    Some(ratio.min(100.0))
}
fn normalized_amperage(raw: Option<f64>) -> Option<f64> {
    let raw = raw?;
    if !raw.is_finite() || raw.fract() != 0.0 || raw < i32::MIN as f64 || raw > u32::MAX as f64 {
        return None;
    }
    // Some firmware exports signed discharge mA as an unsigned 32-bit number.
    let current = if raw > i32::MAX as f64 {
        (raw as u32 as i32) as f64
    } else {
        raw
    };
    (current.abs() <= 50_000.0).then_some(current)
}
fn battery_power(voltage_mv: Option<f64>, raw_current_ma: Option<f64>) -> Option<f64> {
    let (voltage, current) = voltage_mv.zip(normalized_amperage(raw_current_ma))?;
    if !voltage.is_finite() || !(1_000.0..=30_000.0).contains(&voltage) {
        return None;
    }
    let watts = voltage * current / 1_000_000.0;
    (watts.is_finite() && watts.abs() <= 500.0).then_some(watts)
}
fn adapter_power(
    watts: Option<f64>,
    voltage_mv: Option<f64>,
    current_ma: Option<f64>,
    is_on_ac: Option<bool>,
) -> Option<f64> {
    if is_on_ac != Some(true) {
        return None;
    }
    if let Some(watts) = watts.filter(|watts| watts.is_finite() && *watts > 0.0 && *watts <= 500.0)
    {
        return Some(watts);
    }
    let (voltage, current) = voltage_mv.zip(current_ma)?;
    if !voltage.is_finite()
        || !current.is_finite()
        || !(1_000.0..=60_000.0).contains(&voltage)
        || current <= 0.0
        || current > 30_000.0
    {
        return None;
    }
    let watts = voltage * current / 1_000_000.0;
    (watts.is_finite() && watts > 0.0 && watts <= 500.0).then_some(watts)
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::ffi::{c_char, c_void, CStr, CString};
    type CfRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CfRef;
        fn IOPSCopyPowerSourcesList(info: CfRef) -> CfRef;
        fn IOPSGetPowerSourceDescription(info: CfRef, source: CfRef) -> CfRef;
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(port: u32, matching: *mut c_void) -> u32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IORegistryEntryCreateCFProperties(
            entry: u32,
            properties: *mut CfRef,
            allocator: CfRef,
            options: u32,
        ) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: CfRef);
        fn CFGetTypeID(value: CfRef) -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFArrayGetCount(array: CfRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CfRef, index: isize) -> CfRef;
        fn CFDictionaryGetValue(dictionary: CfRef, key: CfRef) -> CfRef;
        fn CFStringCreateWithCString(allocator: CfRef, text: *const c_char, encoding: u32)
            -> CfRef;
        fn CFStringGetCString(
            string: CfRef,
            buffer: *mut c_char,
            length: isize,
            encoding: u32,
        ) -> u8;
        fn CFNumberGetValue(number: CfRef, kind: isize, output: *mut c_void) -> u8;
        fn CFBooleanGetValue(boolean: CfRef) -> u8;
    }
    struct OwnedCf(CfRef);
    impl OwnedCf {
        fn new(value: CfRef) -> Option<Self> {
            // `then_some(Self(value))` eagerly constructs and drops a null
            // owner on None, which would incorrectly call CFRelease(NULL).
            if value.is_null() {
                None
            } else {
                Some(Self(value))
            }
        }
    }
    impl Drop for OwnedCf {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.0);
            }
        }
    }
    struct Service(u32);
    impl Drop for Service {
        fn drop(&mut self) {
            unsafe {
                IOObjectRelease(self.0);
            }
        }
    }
    fn has_type(value: CfRef, expected: usize) -> bool {
        !value.is_null() && unsafe { CFGetTypeID(value) } == expected
    }
    fn property(dictionary: CfRef, name: &str) -> CfRef {
        if !has_type(dictionary, unsafe { CFDictionaryGetTypeID() }) {
            return std::ptr::null();
        }
        let Ok(name) = CString::new(name) else {
            return std::ptr::null();
        };
        let Some(key) = OwnedCf::new(unsafe {
            CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8)
        }) else {
            return std::ptr::null();
        };
        // Borrowed values remain valid while their owning dictionary is retained.
        unsafe { CFDictionaryGetValue(dictionary, key.0) }
    }
    fn string(value: CfRef) -> Option<String> {
        if !has_type(value, unsafe { CFStringGetTypeID() }) {
            return None;
        }
        let mut buffer = [0 as c_char; 512];
        if unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) }
            == 0
        {
            return None;
        }
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_str()
            .ok()
            .map(str::to_owned)
    }
    fn number(value: CfRef) -> Option<f64> {
        if !has_type(value, unsafe { CFNumberGetTypeID() }) {
            return None;
        }
        let mut output = 0f64;
        // kCFNumberFloat64Type = 6 in the active CoreFoundation SDK.
        if unsafe { CFNumberGetValue(value, 6, &mut output as *mut _ as _) } == 0 {
            return None;
        }
        output.is_finite().then_some(output)
    }
    fn boolean(value: CfRef) -> Option<bool> {
        if !has_type(value, unsafe { CFBooleanGetTypeID() }) {
            return None;
        }
        Some(unsafe { CFBooleanGetValue(value) } != 0)
    }
    fn registry_properties() -> Option<OwnedCf> {
        let matching = unsafe { IOServiceMatching(c"AppleSmartBattery".as_ptr()) };
        if matching.is_null() {
            return None;
        }
        // IOServiceGetMatchingService consumes the matching dictionary reference.
        let handle = unsafe { IOServiceGetMatchingService(0, matching) };
        if handle == 0 {
            return None;
        }
        let service = Service(handle);
        let mut properties = std::ptr::null();
        let status = unsafe {
            IORegistryEntryCreateCFProperties(service.0, &mut properties, std::ptr::null(), 0)
        };
        let owned = OwnedCf::new(properties);
        if status != 0 {
            return None;
        }
        owned.filter(|value| has_type(value.0, unsafe { CFDictionaryGetTypeID() }))
    }
    pub fn read() -> Result<Option<BatteryReading>> {
        let info = OwnedCf::new(unsafe { IOPSCopyPowerSourcesInfo() })
            .ok_or_else(|| Error("power source snapshot unavailable".into()))?;
        let sources = OwnedCf::new(unsafe { IOPSCopyPowerSourcesList(info.0) })
            .ok_or_else(|| Error("power source list unavailable".into()))?;
        if !has_type(sources.0, unsafe { CFArrayGetTypeID() }) {
            return Err(Error("invalid power source list".into()));
        }
        let mut reading = BatteryReading::default();
        let mut internal_seen = false;
        let count = unsafe { CFArrayGetCount(sources.0) };
        if !(0..=256).contains(&count) {
            return Err(Error("invalid power source count".into()));
        }
        for index in 0..count {
            let source = unsafe { CFArrayGetValueAtIndex(sources.0, index) };
            let description = unsafe { IOPSGetPowerSourceDescription(info.0, source) };
            if string(property(description, "Type")).as_deref() != Some("InternalBattery") {
                continue;
            }
            internal_seen = true;
            reading.charge_percent = capacity_percent(
                number(property(description, "Current Capacity")),
                number(property(description, "Max Capacity")),
            );
            reading.is_charging = boolean(property(description, "Is Charging"));
            reading.is_on_ac = match string(property(description, "Power Source State")).as_deref()
            {
                Some("AC Power") => Some(true),
                Some("Battery Power") => Some(false),
                _ => None,
            };
            break;
        }
        let properties = registry_properties();
        if let Some(properties) = &properties {
            let dictionary = properties.0;
            if boolean(property(dictionary, "BatteryInstalled")) == Some(false) && !internal_seen {
                return Ok(None);
            }
            internal_seen = true;
            let battery_data = property(dictionary, "BatteryData");
            reading.cycle_count = cycle_count(
                number(property(dictionary, "CycleCount"))
                    .or_else(|| number(property(battery_data, "CycleCount"))),
            );
            reading.is_charging = reading
                .is_charging
                .or_else(|| boolean(property(dictionary, "IsCharging")));
            reading.is_on_ac = reading
                .is_on_ac
                .or_else(|| boolean(property(dictionary, "ExternalConnected")));
            // New macOS versions expose these capacities only in BatteryData.
            let design = number(property(dictionary, "DesignCapacity"))
                .or_else(|| number(property(battery_data, "DesignCapacity")));
            let capacity = number(property(dictionary, "NominalChargeCapacity"))
                .or_else(|| number(property(battery_data, "NominalChargeCapacity")))
                .or_else(|| number(property(dictionary, "AppleRawMaxCapacity")))
                .or_else(|| number(property(battery_data, "FullChargeCapacity")));
            reading.health_percent = estimated_health(capacity, design);
            reading.health_is_estimate = reading.health_percent.is_some();
            reading.power_watts = battery_power(
                number(property(dictionary, "Voltage")),
                number(property(dictionary, "Amperage")),
            );
            let adapter = property(dictionary, "AdapterDetails");
            reading.adapter_watts = adapter_power(
                number(property(adapter, "Watts")),
                number(property(adapter, "Voltage")),
                number(property(adapter, "Current")),
                reading.is_on_ac,
            );
        }
        Ok(internal_seen.then_some(reading))
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn missing_cf_properties_never_release_or_convert_null() {
            let null = std::ptr::null();
            assert!(OwnedCf::new(null).is_none());
            assert_eq!(number(null), None);
            assert_eq!(boolean(null), None);
            assert_eq!(string(null), None);
            assert!(property(null, "Capacity").is_null());
        }
    }
}
#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub fn read() -> Result<Option<BatteryReading>> {
        Err(Error("battery telemetry requires macOS".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percent_uses_matching_units_and_missing_values_stay_unknown() {
        assert_eq!(capacity_percent(Some(47.0), Some(100.0)), Some(47.0));
        assert_eq!(capacity_percent(Some(2500.0), Some(5000.0)), Some(50.0));
        assert_eq!(capacity_percent(Some(0.0), Some(100.0)), Some(0.0));
        for values in [
            (None, Some(100.0)),
            (Some(5.0), None),
            (Some(5.0), Some(0.0)),
            (Some(f64::NAN), Some(100.0)),
            (Some(101.0), Some(100.0)),
        ] {
            assert_eq!(capacity_percent(values.0, values.1), None);
        }
    }
    #[test]
    fn health_estimate_never_fabricates_a_capacity() {
        assert_eq!(estimated_health(None, Some(8694.0)), None);
        assert_eq!(estimated_health(Some(1.0), None), None);
        assert_eq!(estimated_health(Some(0.0), Some(8694.0)), None);
        assert_eq!(estimated_health(Some(103.0), Some(100.0)), Some(100.0));
        assert_eq!(cycle_count(Some(211.0)), Some(211));
        assert_eq!(cycle_count(Some(1.5)), None);
        assert_eq!(cycle_count(Some(-1.0)), None);
        let missing = serde_json::to_value(BatteryReading::default()).unwrap();
        assert!(missing["charge_percent"].is_null());
        assert!(missing["health_percent"].is_null());
    }
    #[test]
    fn battery_watts_preserve_sign_units_and_real_zero() {
        assert_eq!(battery_power(Some(12_000.0), Some(1500.0)), Some(18.0));
        assert_eq!(battery_power(Some(12_000.0), Some(-1500.0)), Some(-18.0));
        assert_eq!(
            battery_power(Some(12_000.0), Some(4_294_965_796.0)),
            Some(-18.0)
        );
        assert_eq!(battery_power(Some(12_000.0), Some(0.0)), Some(0.0));
        assert_eq!(battery_power(Some(12_000.0), None), None);
        assert_eq!(battery_power(None, Some(0.0)), None);
        assert_eq!(battery_power(Some(f64::NAN), Some(1.0)), None);
        assert_eq!(battery_power(Some(12.0), Some(1500.0)), None);
        assert_eq!(battery_power(Some(12_000.0), Some(50_001.0)), None);
        assert_eq!(normalized_amperage(Some(4_294_967_296.0)), None);
    }
    #[test]
    fn adapter_rating_requires_ac_and_sane_milliunits() {
        assert_eq!(
            adapter_power(Some(140.0), None, None, Some(true)),
            Some(140.0)
        );
        assert_eq!(
            adapter_power(None, Some(20_000.0), Some(5000.0), Some(true)),
            Some(100.0)
        );
        assert_eq!(adapter_power(Some(140.0), None, None, Some(false)), None);
        assert_eq!(adapter_power(Some(140.0), None, None, None), None);
        assert_eq!(adapter_power(None, Some(20.0), Some(5.0), Some(true)), None);
        assert_eq!(
            adapter_power(Some(f64::INFINITY), None, None, Some(true)),
            None
        );
    }
    #[test]
    #[ignore = "opt-in, read-only physical device probe"]
    fn native_readonly_probe() {
        let reading = read_battery().unwrap();
        println!("{}", serde_json::to_string_pretty(&reading).unwrap());
    }
}
