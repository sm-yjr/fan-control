//! Exact Apple Silicon temperature keys; never infer CPU/GPU from a key prefix.
//! Key/group facts adapted from Stats (MIT), commit
//! ee4265f3b9afdffebd3273cf6a83b9327ead45b5, Modules/Sensors/values.swift.
//! Upstream license: ../licenses/Stats-LICENSE.txt.
//! Aggregate TCMb/TCMz meanings independently verified in iSMC's sensors.go.
//! Keys overlap across generations and do not reliably identify physical core numbers.
use super::SensorGroup;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SiliconGeneration {
    #[default]
    Unknown,
    M1,
    M2,
    M3,
    M4,
    M5,
}
impl SiliconGeneration {
    pub(super) fn from_brand(brand: &str) -> Self {
        let mut words = brand.split_whitespace();
        if words.next() != Some("Apple") {
            return Self::Unknown;
        }
        match words.next() {
            Some("M1") => Self::M1,
            Some("M2") => Self::M2,
            Some("M3") => Self::M3,
            Some("M4") => Self::M4,
            Some("M5") => Self::M5,
            _ => Self::Unknown,
        }
    }
    pub(super) fn read() -> Self {
        #[cfg(target_os = "macos")]
        {
            let mut buffer = [0u8; 128];
            let mut length = buffer.len();
            // Read-only sysctl, bounded destination. No private device identifier.
            let status = unsafe {
                libc::sysctlbyname(
                    c"machdep.cpu.brand_string".as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    &mut length,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if status == 0 && (1..=buffer.len()).contains(&length) {
                if let Ok(brand) = std::str::from_utf8(&buffer[..length]) {
                    return Self::from_brand(brand.trim_end_matches('\0'));
                }
            }
        }
        Self::Unknown
    }
    fn keys(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::Unknown => (&[], &[]),
            Self::M1 => (M1_CPU, M1_GPU),
            Self::M2 => (M2_CPU, M2_GPU),
            Self::M3 => (M3_CPU, M3_GPU),
            Self::M4 => (M4_CPU, M4_GPU),
            Self::M5 => (M5_CPU, M5_GPU),
        }
    }
}

pub(super) fn sensor_name(key: &str, chip: SiliconGeneration) -> (String, SensorGroup) {
    use SensorGroup::*;
    // Firmware aggregate readings have stable semantics across chip generations.
    let known = match key {
        "TCMb" => Some(("CPU Die Average", Cpu)),
        "TCMz" => Some(("CPU Die Max", Cpu)),
        "TC0P" => Some(("CPU Proximity", Cpu)),
        "TC0D" => Some(("CPU Diode", Cpu)),
        "TCAD" => Some(("CPU Package", Cpu)),
        "TG0P" => Some(("GPU Proximity", Gpu)),
        "TG0D" => Some(("GPU Diode", Gpu)),
        "Tm0P" => Some(("主板", System)),
        "TaLP" => Some(("左侧气流", System)),
        "TaRF" => Some(("右侧气流", System)),
        "TH0x" => Some(("NAND", System)),
        "TB1T" => Some(("电池 1", System)),
        "TB2T" => Some(("电池 2", System)),
        "TW0P" => Some(("无线网卡", System)),
        _ => None,
    };
    if let Some((name, group)) = known {
        return (name.into(), group);
    }
    let (cpu, gpu) = chip.keys();
    if cpu.contains(&key) {
        (format!("CPU Temperature ({key})"), Cpu)
    } else if gpu.contains(&key) {
        (format!("GPU Temperature ({key})"), Gpu)
    } else {
        (key.into(), Other)
    }
}

const M1_CPU: &[&str] = &[
    "Tp09", "Tp0T", "Tp01", "Tp05", "Tp0D", "Tp0H", "Tp0L", "Tp0P", "Tp0X", "Tp0b",
];
const M1_GPU: &[&str] = &["Tg05", "Tg0D", "Tg0L", "Tg0T"];
const M2_CPU: &[&str] = &[
    "Tp1h", "Tp1t", "Tp1p", "Tp1l", "Tp01", "Tp05", "Tp09", "Tp0D", "Tp0X", "Tp0b", "Tp0f", "Tp0j",
];
const M2_GPU: &[&str] = &["Tg0f", "Tg0j"];
const M3_CPU: &[&str] = &[
    "Te05", "Te0L", "Te0P", "Te0S", "Tf04", "Tf09", "Tf0A", "Tf0B", "Tf0D", "Tf0E", "Tf44", "Tf49",
    "Tf4A", "Tf4B", "Tf4D", "Tf4E",
];
const M3_GPU: &[&str] = &[
    "Tf14", "Tf18", "Tf19", "Tf1A", "Tf24", "Tf28", "Tf29", "Tf2A",
];
const M4_CPU: &[&str] = &[
    "Te05", "Te0S", "Te09", "Te0H", "Tp01", "Tp05", "Tp09", "Tp0D", "Tp0V", "Tp0Y", "Tp0b", "Tp0e",
];
const M4_GPU: &[&str] = &[
    "Tg0G", "Tg0H", "Tg1U", "Tg1k", "Tg0K", "Tg0L", "Tg0d", "Tg0e", "Tg0j", "Tg0k",
];
const M5_CPU: &[&str] = &[
    "Tp00", "Tp04", "Tp08", "Tp0C", "Tp0G", "Tp0K", "Tp0O", "Tp0R", "Tp0U", "Tp0X", "Tp0a", "Tp0d",
    "Tp0g", "Tp0j", "Tp0m", "Tp0p", "Tp0u", "Tp0y",
];
const M5_GPU: &[&str] = &[
    "Tg0U", "Tg0X", "Tg0d", "Tg0g", "Tg0j", "Tg1Y", "Tg1c", "Tg1g",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_brand_recognizes_variants_without_guessing_future_generations() {
        for (brand, expected) in [
            ("Apple M1", SiliconGeneration::M1),
            ("Apple M1 Ultra", SiliconGeneration::M1),
            ("Apple M2 Max", SiliconGeneration::M2),
            ("Apple M3 Pro", SiliconGeneration::M3),
            ("Apple M4", SiliconGeneration::M4),
            ("Apple M5 Max", SiliconGeneration::M5),
            ("Apple M10", SiliconGeneration::Unknown),
            ("Apple M6", SiliconGeneration::Unknown),
            ("Intel Core M4", SiliconGeneration::Unknown),
            ("Unknown", SiliconGeneration::Unknown),
        ] {
            assert_eq!(SiliconGeneration::from_brand(brand), expected, "{brand}");
        }
    }

    #[test]
    fn same_key_is_not_assumed_to_have_same_core_semantics() {
        // Tp09 is an efficiency probe on M1 and a performance probe on M2/M4.
        for chip in [
            SiliconGeneration::M1,
            SiliconGeneration::M2,
            SiliconGeneration::M4,
        ] {
            let (name, group) = sensor_name("Tp09", chip);
            assert_eq!(group, SensorGroup::Cpu);
            assert_eq!(name, "CPU Temperature (Tp09)");
        }
        // Tf04 is a CPU probe on M3; later generations reuse it in GPU fabric.
        assert_eq!(
            sensor_name("Tf04", SiliconGeneration::M3).1,
            SensorGroup::Cpu
        );
        assert_eq!(
            sensor_name("Tf04", SiliconGeneration::M4).1,
            SensorGroup::Other
        );
        assert_eq!(
            sensor_name("Tf04", SiliconGeneration::M5).1,
            SensorGroup::Other
        );
    }

    #[test]
    fn aggregates_survive_unknown_models_and_headroom_does_not_become_temperature() {
        for chip in [
            SiliconGeneration::Unknown,
            SiliconGeneration::M1,
            SiliconGeneration::M2,
            SiliconGeneration::M3,
            SiliconGeneration::M4,
            SiliconGeneration::M5,
        ] {
            assert_eq!(
                sensor_name("TCMb", chip),
                ("CPU Die Average".into(), SensorGroup::Cpu)
            );
            assert_eq!(
                sensor_name("TCMz", chip),
                ("CPU Die Max".into(), SensorGroup::Cpu)
            );
            for key in ["Ta00", "Ta01", "Ta04", "TpZZ", "TgZZ", "TVMN"] {
                assert_eq!(sensor_name(key, chip).1, SensorGroup::Other, "{key}");
            }
        }
        assert_eq!(
            sensor_name("Tp01", SiliconGeneration::Unknown).1,
            SensorGroup::Other
        );
    }

    #[test]
    fn exact_tables_have_valid_unique_keys_and_no_cpu_gpu_overlap() {
        for chip in [
            SiliconGeneration::M1,
            SiliconGeneration::M2,
            SiliconGeneration::M3,
            SiliconGeneration::M4,
            SiliconGeneration::M5,
        ] {
            let (cpu, gpu) = chip.keys();
            assert!(!cpu.is_empty() && !gpu.is_empty());
            let mut seen = std::collections::HashSet::new();
            for key in cpu.iter().chain(gpu) {
                assert_eq!(key.len(), 4);
                assert!(key.is_ascii());
                assert!(seen.insert(key), "duplicate key {key}: {chip:?}");
            }
        }
    }
}
