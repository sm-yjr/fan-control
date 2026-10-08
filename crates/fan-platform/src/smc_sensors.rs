//! Exact Apple Silicon temperature keys; never infer CPU/GPU from a key prefix.
//! Key/group facts adapted from Stats (MIT), commit
//! ee4265f3b9afdffebd3273cf6a83b9327ead45b5, Modules/Sensors/values.swift.
//! Upstream license: ../licenses/Stats-LICENSE.txt.
//! Aggregate TCMb/TCMz meanings independently verified in iSMC's sensors.go.
//! Supplemental labels adapted from iSMC (GPL-3.0-only),
//! Copyright (C) 2019 Dinko Korunic, commit
//! 6b107a39b5759fc9ac510291dff0f6f13e61b388, smc/sensors.go.
//! Upstream license: ../licenses/iSMC-LICENSE.txt.
//! Modified 2026-10-08 for Fan Control: Chinese labels, generation bounds and
//! display-only admission; original hardware control groups are preserved.
//! Keys overlap across generations and do not reliably identify physical core numbers.
//! See docs/sensor-semantics.md for source conflicts and control-admission boundaries.
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
        (cpu_probe_name(key, chip).into(), Cpu)
    } else if gpu.contains(&key) {
        ("GPU 温度探头".into(), Gpu)
    } else if let Some(name) = diagnostic_name(key, chip) {
        (name.into(), Other)
    } else {
        (key.into(), Other)
    }
}

// Describe a region, never a physical core number. Stats and iSMC disagree on
// several M3 Tf* and M4 Tp* core/fabric identities; the coarse names stay valid.
fn cpu_probe_name(key: &str, chip: SiliconGeneration) -> &'static str {
    let efficiency = match chip {
        SiliconGeneration::M1 => &["Tp09", "Tp0T"][..],
        SiliconGeneration::M2 => &["Tp1h", "Tp1t", "Tp1p", "Tp1l"],
        SiliconGeneration::M3 => &["Te05", "Te0L", "Te0P", "Te0S"],
        SiliconGeneration::M4 => &["Te05", "Te0S", "Te09", "Te0H"],
        _ => &[],
    };
    if efficiency.contains(&key) {
        "CPU 能效区温度"
    } else if chip == SiliconGeneration::M3 {
        "CPU 温度探头"
    } else if chip == SiliconGeneration::M5
        && ["Tp00", "Tp04", "Tp08", "Tp0C", "Tp0G", "Tp0K"].contains(&key)
    {
        "CPU 超级核心区温度"
    } else {
        "CPU 性能区温度"
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

// Supplemental labels are presentation facts, not control-sensor admission. In particular,
// CPU/GPU probes newly described by iSMC remain Other, and SSD/virtual/power probes
// must not become System: that group is consumed by the body-temperature model.
// Source: iSMC 6b107a39b5759fc9ac510291dff0f6f13e61b388, smc/sensors.go.
// Exact keys only; expanded upstream % patterns are bounded to digits 0..=9.
fn diagnostic_name(key: &str, chip: SiliconGeneration) -> Option<&'static str> {
    let specific = match chip {
        SiliconGeneration::Unknown => &[],
        SiliconGeneration::M1 => M1_DIAGNOSTICS,
        SiliconGeneration::M2 => M2_DIAGNOSTICS,
        SiliconGeneration::M3 => M3_DIAGNOSTICS,
        SiliconGeneration::M4 => M4_DIAGNOSTICS,
        SiliconGeneration::M5 => M5_DIAGNOSTICS,
    };
    specific
        .iter()
        .chain(COMMON_DIAGNOSTICS)
        .find_map(|(name, keys)| keys.contains(&key).then_some(*name))
}

const COMMON_DIAGNOSTICS: &[(&str, &[&str])] = &[
    ("电源管理邻近温度", &["TPMP"]),
    ("系统存储二极管温度", &["TSDD"]),
    ("CPU 芯片汇总温度", &["TCDX"]),
    (
        "雷电接口二极管温度",
        &[
            "TT0D", "TT1D", "TT2D", "TT3D", "TT4D", "TT5D", "TT6D", "TT7D", "TT8D", "TT9D",
        ],
    ),
    (
        "芯片散热器温度",
        &[
            "Th00", "Th01", "Th02", "Th04", "Th05", "Th06", "Th08", "Th09", "Th0A", "Th0C", "Th0D",
            "Th0E", "Th0G", "Th0H", "Th0I", "Th0K", "Th0L", "Th0M", "Th0O", "Th0P", "Th0Q", "Th0S",
            "Th0T", "Th0U", "Th20", "Th21", "Th22", "Th24", "Th25", "Th26", "Th28", "Th29", "Th2A",
            "Th2C", "Th2D", "Th2E", "Th2G", "Th2H", "Th2I",
        ],
    ),
    ("左侧雷电接口温度", &["TTLD"]),
    ("右侧雷电接口温度", &["TTRD"]),
    (
        "NVMe 存储温度",
        &[
            "TH0a", "TH1a", "TH2a", "TH3a", "TH4a", "TH5a", "TH6a", "TH7a", "TH8a", "TH9a", "TH0b",
            "TH1b", "TH2b", "TH3b", "TH4b", "TH5b", "TH6b", "TH7b", "TH8b", "TH9b", "TH0c", "TH1c",
            "TH2c", "TH3c", "TH4c", "TH5c", "TH6c", "TH7c", "TH8c", "TH9c",
        ],
    ),
    ("电源邻近温度", &["TPSP"]),
    ("系统控制器温度", &["TSCD"]),
    ("内存供电温度", &["TMVR"]),
    ("嵌入式设备温度", &["TIED"]),
    ("左侧雷电接口邻近温度", &["TaLT"]),
    ("右侧雷电接口邻近温度", &["TaRT"]),
    ("左侧风道壁温度", &["TaLW"]),
    ("右侧风道壁温度", &["TaRW"]),
    ("左前侧气流温度", &["TaFL"]),
    ("右前侧气流温度", &["TaFR"]),
    ("左后侧气流温度", &["TaRL"]),
    ("右后侧气流温度", &["TaRR"]),
    ("屏幕盖外侧环境温度", &["TAOL"]),
    ("顶部环境邻近温度", &["TaTP"]),
    ("SSD 邻近温度", &["TS0P", "TS1P"]),
    ("芯片供电温度", &["TSVR", "TSWR", "TSXR"]),
    (
        "虚拟芯片温度",
        &[
            "TVD0", "TVD1", "TVD2", "TVD3", "TVD4", "TVD5", "TVD6", "TVD7", "TVD8", "TVD9",
        ],
    ),
    (
        "内存虚拟温度",
        &[
            "TVMR", "TVMr", "TVM0", "TVM4", "TVm0", "TVm1", "TVm2", "TVh0", "TVh1", "TVh2",
        ],
    ),
    ("内存虚拟汇总温度", &["TVmS", "TVms", "TVMX"]),
    ("内存虚拟簇温度", &["TVMC"]),
    (
        "虚拟环境温度",
        &[
            "TVA0", "TVA1", "TVA2", "TVA3", "TVA4", "TVA5", "TVA6", "TVA7", "TVA8", "TVA9",
        ],
    ),
    (
        "虚拟传感器温度",
        &[
            "TVS0", "TVS1", "TVS2", "TVS3", "TVS4", "TVS5", "TVS6", "TVS7", "TVS8", "TVS9", "TVSx",
        ],
    ),
    ("供电虚拟温度", &["TVV0"]),
    ("电源二极管温度", &["TPSD"]),
    ("雷电接口邻近温度", &["TT0P"]),
    (
        "供电芯片温度",
        &[
            "TPD0", "TPD1", "TPD2", "TPD3", "TPD4", "TPD5", "TPD6", "TPD7", "TPD8", "TPD9", "TPDA",
            "TPDB", "TPDC", "TPDD", "TPDE", "TPDF", "TPDa", "TPDb", "TPDc", "TPDd", "TPDe", "TPDf",
            "TPDg", "TPDh", "TPDi", "TPDj",
        ],
    ),
    ("供电芯片最高温度", &["TPDX"]),
    (
        "射频供电温度",
        &[
            "TRD0", "TRD1", "TRD2", "TRD3", "TRD4", "TRD5", "TRD6", "TRD7", "TRD8", "TRD9", "TRDa",
            "TRDb", "TRDc", "TRDd", "TRDe", "TRDf", "TRDg", "TRDh", "TRDi", "TRDj",
        ],
    ),
    ("射频供电最高温度", &["TRDX"]),
];

const M1_DIAGNOSTICS: &[(&str, &[&str])] = &[
    (
        "CPU 温度探头",
        &[
            "Tp08", "Tp09", "Tp0A", "Tp0S", "Tp0T", "Tp0U", "Tp00", "Tp01", "Tp02", "Tp04", "Tp05",
            "Tp06", "Tp0C", "Tp0D", "Tp0E", "Tp0G", "Tp0H", "Tp0I", "Tp0K", "Tp0L", "Tp0M", "Tp0O",
            "Tp0P", "Tp0Q", "Tp0W", "Tp0X", "Tp0Y", "Tp0a", "Tp0b", "Tp0c", "Tp28", "Tp29", "Tp2A",
            "Tp2S", "Tp2T", "Tp2U", "Tp20", "Tp21", "Tp22", "Tp24", "Tp25", "Tp26", "Tp2C", "Tp2D",
            "Tp2E", "Tp2G", "Tp2H", "Tp2I", "Tp2K", "Tp2L", "Tp2M", "Tp2O", "Tp2P", "Tp2Q", "Tp2W",
            "Tp2X", "Tp2Y", "Tp2a", "Tp2b", "Tp2c", "Te00", "Te01", "Te02", "Te20", "Te21",
        ],
    ),
    (
        "GPU 温度探头",
        &[
            "Tg04", "Tg05", "Tg0C", "Tg0D", "Tg0K", "Tg0L", "Tg0S", "Tg0T", "Tg0a", "Tg0b", "Tg0i",
            "Tg0j", "Tg0q", "Tg0r", "Tg0y", "Tg0z",
        ],
    ),
    (
        "内存温度探头",
        &[
            "Tm00", "Tm01", "Tm02", "Tm04", "Tm05", "Tm06", "Tm08", "Tm09", "Tm0A", "Tm0C", "Tm0D",
            "Tm0E",
        ],
    ),
    (
        "SSD 温度探头",
        &[
            "Ts00", "Ts01", "Ts02", "Ts04", "Ts05", "Ts06", "Ts20", "Ts21", "Ts22", "Ts24", "Ts25",
            "Ts26",
        ],
    ),
    ("SSD 控制器温度", &["Ts0P", "Ts1P"]),
    (
        "供电芯片温度",
        &[
            "TpD0", "TpD1", "TpD2", "TpD3", "TpD4", "TpD5", "TpD6", "TpD7", "TpD8", "TpD9", "TpDa",
            "TpDb", "TpDc", "TpDd", "TpDe", "TpDf", "TpDg", "TpDh", "TpDi", "TpDj",
        ],
    ),
    ("供电芯片最高温度", &["TpDX"]),
    (
        "射频供电温度",
        &[
            "TrD0", "TrD1", "TrD2", "TrD3", "TrD4", "TrD5", "TrD6", "TrD7", "TrD8", "TrD9", "TrDa",
            "TrDb", "TrDc", "TrDd", "TrDe", "TrDf", "TrDg", "TrDh", "TrDi", "TrDj",
        ],
    ),
    ("射频供电最高温度", &["TrDX"]),
];

const M2_DIAGNOSTICS: &[(&str, &[&str])] = &[
    (
        "CPU 温度探头",
        &[
            "Tp00", "Tp01", "Tp02", "Tp04", "Tp05", "Tp06", "Tp08", "Tp09", "Tp0A", "Tp0C", "Tp0D",
            "Tp0E", "Tp0e", "Tp0f", "Tp0g", "Tp0i", "Tp0j", "Tp0k", "Tp0m", "Tp0n", "Tp0o", "Tp0q",
            "Tp0r", "Tp0s", "Tp0a", "Tp0b", "Tp0c", "Te04", "Te05", "Te06",
        ],
    ),
    (
        "GPU 温度探头",
        &[
            "Tg0e", "Tg0f", "Tg0m", "Tg0n", "Tg0q", "Tg0r", "Tg0W", "Tg0X", "Tg0i", "Tg0j", "Tg1g",
            "Tg1h", "Tg1k", "Tg1l", "Tg2W", "Tg2X", "Tg2e", "Tg2f", "Tg3Y", "Tg3Z",
        ],
    ),
    (
        "SSD 温度探头",
        &[
            "Ts0K", "Ts0L", "Ts0M", "Ts0O", "Ts0P", "Ts0Q", "Ts0S", "Ts0T", "Ts0U", "Ts0W", "Ts0X",
            "Ts0Y", "Ts0a", "Ts0b", "Ts0c", "Ts0C", "Ts0D", "Ts0E", "Ts0e", "Ts0f", "Ts0g",
        ],
    ),
    ("SSD 控制器温度", &["Ts1P", "TsOP"]),
];

const M3_DIAGNOSTICS: &[(&str, &[&str])] = &[
    (
        "CPU 温度探头",
        &[
            "Te04", "Te05", "Te06", "Te0G", "Te0H", "Te0I", "Te0P", "Te0Q", "Te0R", "Te0S", "Te0T",
            "Te0U", "Te0K", "Te0L", "Te0M", "Tp04", "Tp05", "Tp06", "Tp0C", "Tp0D", "Tp0E", "Tp0K",
            "Tp0L", "Tp0M", "Tp0a", "Tp0b", "Tp0c", "Tp0g", "Tp0h", "Tp0i", "Tp0m", "Tp0n", "Tp0o",
            "Tp1E", "Tp1F", "Tp1G", "Tp1Q", "Tp1R", "Tp1S", "Tp0R", "Tp0S", "Tp0T", "Tp0U", "Tp0V",
            "Tp0W", "Tp3O", "Tp3P", "Tp1I", "Tp1J", "Tp1K", "Tp3W", "Tp3X", "Tp0y", "Tp0z", "Tp10",
            "Tp16", "Tp17", "Tp18", "Tp3S", "Tp3T",
        ],
    ),
    (
        "GPU 温度探头",
        &[
            "Tg04", "Tg05", "Tg0C", "Tg0D", "Tg0K", "Tg0L", "Tg00", "Tg01", "Tg0U", "Tg0u", "Tg0V",
            "Tg0v", "Tg12", "Tg13", "Tg1A", "Tg1B", "Tg1k", "Tg1l", "Tg0y", "Tg0z", "Tg1E", "Tg1F",
            "Tg16", "Tg17", "Tg1s", "Tg1t", "Tg1x", "Tg1y", "Tg21", "Tg22", "Tg29", "Tg2A", "Tg2H",
            "Tg2I", "Tg33", "Tg34", "Tg3B", "Tg3C", "Tg3J", "Tg3K", "Tg3x", "Tg3y",
        ],
    ),
    (
        "SSD 温度探头",
        &[
            "Ts00", "Ts01", "Ts02", "Ts04", "Ts05", "Ts06", "Ts0C", "Ts0D", "Ts0E", "Ts0K", "Ts0L",
            "Ts0M", "Ts0R", "Ts0S", "Ts0T", "Ts0U", "Ts0V", "Ts0W", "Ts0Y", "Ts0Z", "Ts0a", "Ts0h",
            "Ts0i",
        ],
    ),
    ("SSD 控制器温度", &["Ts0P", "Ts1P"]),
];

const M4_DIAGNOSTICS: &[(&str, &[&str])] = &[
    (
        "CPU 温度探头",
        &[
            "Te04", "Te05", "Te06", "Te0R", "Te0S", "Te0T", "Te08", "Te09", "Te0A", "Te0G", "Te0H",
            "Te0I", "Te0U", "Te0V", "Te0W", "Te0X", "Tpx8", "Tpx9", "TpxA", "TpxB", "TpxC", "TpxD",
            "Tex0", "Tex1", "Tex2", "Tex3", "Tp1i", "Tp1j", "Tp1k", "Tp1m", "Tp1n", "Tp1o", "Tp1q",
            "Tp1t", "Tp1u", "Tp1v", "Tp1w", "Tp1x", "Tp1y", "Tp1z", "Tp20", "Tp21", "Tp22", "Tp23",
            "Tp24", "Tp25", "Tp26", "Tp27", "Tp28", "Tp29", "Tp2A", "Tp2B", "Tp2C", "Tp2D", "Tp2E",
            "Tp2G", "Tp00", "Tp01", "Tp02", "Tp04", "Tp05", "Tp06", "Tp08", "Tp09", "Tp0A", "Tp0C",
            "Tp0D", "Tp0E", "Tp0G", "Tp0H", "Tp0I", "Tp0K", "Tp0L", "Tp0M", "Tp0O", "Tp0P", "Tp0Q",
            "Tp0S", "Tp0T", "Tp0U", "Tp0W", "Tp0X", "Tp0Y", "Tp0Z", "Tp0a", "Tp0b", "Tp0c", "Tp0d",
            "Tp0e", "Tp0f", "Tp0g", "Tp0i", "Tp0k", "Tp0l", "Tp0m", "Tp0n", "Tp0o", "Tp0q", "Tp0t",
            "Tp0u", "Tp0v", "Tp0w", "Tpx0", "Tpx1", "Tpx2", "Tpx3", "Tpx4", "Tpx5", "Tp0V", "Tp3O",
            "Tp3P", "Tp3W", "Tp3X", "Tp3S", "Tp3T", "Tp1A", "Tp1B", "Tp1C", "Tp1E", "Tp1F", "Tp1G",
            "Tp1Q", "Tp1R", "Tp1S",
        ],
    ),
    (
        "GPU 温度探头",
        &[
            "Tg0G", "Tg0H", "Tg0C", "Tg0K", "Tg0D", "Tg0L", "Tg0O", "Tg0d", "Tg0P", "Tg0e", "Tg0U",
            "Tg0j", "Tg0V", "Tg0k", "Tg0m", "Tg0n", "Tg04", "Tg05", "Tg0R", "Tg0S", "Tg0X", "Tg0Y",
            "Tg0y", "Tg0z", "Tg1E", "Tg1F", "Tg1U", "Tg1V", "Tg1c", "Tg1d", "Tg1k", "Tg1l", "Tg21",
            "Tg22", "Tg2H", "Tg2I", "Tg2P", "Tg2Q", "Tg2X", "Tg2Y", "Tg2f", "Tg2g", "Tg2n", "Tg2o",
            "Tg33", "Tg34", "Tg3J", "Tg3K", "Tg3Z", "Tg3a", "Tg3h", "Tg3i", "Tg3p", "Tg3q",
        ],
    ),
    ("左后侧气流温度", &["TaLR"]),
    (
        "GPU 互连温度探头",
        &["TfC0", "TfC1", "TfC2", "TfC3", "TfC4"],
    ),
    (
        "非核心区温度",
        &[
            "TUD0", "TUD1", "TUD2", "TUD3", "TUD4", "TUD5", "TUD6", "TUD7", "TUD8", "TUD9", "TUDX",
            "TUDa", "TUDb", "TUDc", "TUDd", "TUDe", "TUDf",
        ],
    ),
    (
        "SSD 温度探头",
        &[
            "Ts00", "Ts01", "Ts02", "Ts04", "Ts05", "Ts06", "Ts08", "Ts09", "Ts0A", "Ts0C", "Ts0D",
            "Ts0E", "Ts0G", "Ts0H", "Ts0I", "Ts0K", "Ts0L", "Ts0M", "Ts0O", "Ts0Q", "Ts0R", "Ts0S",
            "Ts0T", "Ts0U", "Ts0V", "Ts0W", "Ts0X", "Ts0d", "Ts0e", "Ts0f", "Ts0g", "Ts0h", "Ts0i",
            "TS0p",
        ],
    ),
    ("闪存邻近温度", &["TH0p"]),
    ("SSD 控制器温度", &["Ts0P", "TsOP", "Ts1P"]),
    ("SSD 汇总温度", &["Tsx0", "Tsx1"]),
    ("内存邻近温度", &["Tm0p", "Tm1p", "Tm2p"]),
    ("内存温度探头", &["Tm0B"]),
    ("芯片散热区温度", &["TSCP"]),
    ("环境邻近温度", &["Ta0p"]),
    ("非核心区供电温度", &["TUVR"]),
    ("供电散热器温度", &["TPH3"]),
    ("芯片互连温度", &["TF2S"]),
];

const M5_DIAGNOSTICS: &[(&str, &[&str])] = &[
    (
        "CPU 温度探头",
        &[
            "Tp00", "Tp04", "Tp08", "Tp0C", "Tp0G", "Tp0K", "Tp0O", "Tp0R", "Tp0U", "Tp0X", "Tp0a",
            "Tp0d", "Tp0g", "Tp0j", "Tp0m", "Tp0p", "Tp0u", "Tp0y", "Tp1E", "Tp1I", "Tp1Q", "Tp1U",
            "Tp1g",
        ],
    ),
    (
        "CPU 热余量（非绝对温度）",
        &["Ta00", "Ta04", "Ta08", "Ta0K", "Ta0O", "Ta0R"],
    ),
    (
        "GPU 温度探头",
        &[
            "Tg08", "Tg0C", "Tg0O", "Tg0R", "Tg0U", "Tg0X", "Tg0a", "Tg0d", "Tg0g", "Tg0j", "Tg12",
            "Tg16", "Tg1I", "Tg1M", "Tg1Q", "Tg1U", "Tg1Y", "Tg1c", "Tg1k", "Tg1o", "Tg1x", "Tg29",
            "Tg2D", "Tg2P", "Tg2T", "Tg2X", "Tg2b", "Tg2f", "Tg2j", "Tg2n", "Tg2r", "Tg3B", "Tg3F",
            "Tg3R", "Tg3V", "Tg3Z", "Tg3d", "Tg3h", "Tg3l", "Tg3t", "Tg3x", "Tg43",
        ],
    ),
    (
        "GPU 互连温度探头",
        &[
            "Tf04", "Tf06", "Tf08", "Tf09", "Tf0A", "Tf0B", "Tf14", "Tf16", "Tf18", "Tf19", "Tf1A",
            "Tf1B", "TfC0", "TfC1", "TfC3", "TfC5", "TfCD", "TfCE", "TfCF", "TfC6", "TfC7", "TfC8",
            "TfC9", "TfCA",
        ],
    ),
    ("虚拟芯片温度", &["TVDA", "TVDG", "TVDM"]),
    (
        "内存温度探头",
        &[
            "Tm00", "Tm04", "Tm08", "Tm0C", "Tm0G", "Tm0K", "Tm0O", "Tm0R", "Tm0U", "Tm0X", "Tm0a",
            "Tm0d", "Tm0g", "Tm0j", "Tm0m", "Tm0p", "Tm0u", "Tm1E", "Tm1I", "Tm1M", "Tm1Q", "Tm1U",
            "Tm1Y", "Tm1c", "Tm1g", "Tm1k", "Tm1o", "Tm1s", "Tm1x", "Tm21", "Tm25", "Tm29", "Tm2D",
            "Tm2H", "Tm2L", "Tm2P", "Tm2j", "Tm2n",
        ],
    ),
    (
        "SSD 温度探头",
        &[
            "Ts00", "Ts04", "Ts08", "Ts0C", "Ts0K", "Ts0O", "Ts0R", "Ts0U", "Ts0X", "Ts0a", "Ts0d",
            "Ts0g",
        ],
    ),
    ("SSD 控制器温度", &["Ts0P", "Ts1P"]),
    (
        "芯片封装温度探头",
        &[
            "TN00", "TN01", "TN02", "TN03", "TN04", "TN05", "TN06", "TN07",
        ],
    ),
    (
        "供电温度探头",
        &[
            "TV00", "TV01", "TV02", "TV03", "TV04", "TV05", "TV06", "TV07", "TV08", "TV09", "TV10",
            "TV11", "TV12", "TV13", "TV14", "TV15", "TV16", "TV17", "TV18", "TV19", "TVN0", "TVN1",
            "TVN2", "TVN3", "TVN4", "TVN5", "TVN6", "TVN7", "TVN8", "TVN9", "TVMN", "TVNN",
        ],
    ),
    (
        "非核心区温度",
        &[
            "TUD0", "TUD1", "TUD2", "TUD3", "TUD4", "TUD5", "TUD6", "TUD7", "TUD8", "TUD9", "TUDX",
            "TUDa", "TUDb", "TUDc", "TUDd", "TUDe", "TUDf",
        ],
    ),
    ("供电散热器温度", &["TPH3"]),
    ("芯片互连温度", &["TF2S"]),
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
            let expected = if chip == SiliconGeneration::M1 {
                "CPU 能效区温度"
            } else {
                "CPU 性能区温度"
            };
            assert_eq!(name, expected);
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
    fn diagnostics_add_labels_without_changing_control_or_body_inputs() {
        // Every newly described key remains outside Cpu/Gpu/System admission.
        for (chip, diagnostics) in [
            (SiliconGeneration::Unknown, &[][..]),
            (SiliconGeneration::M1, M1_DIAGNOSTICS),
            (SiliconGeneration::M2, M2_DIAGNOSTICS),
            (SiliconGeneration::M3, M3_DIAGNOSTICS),
            (SiliconGeneration::M4, M4_DIAGNOSTICS),
            (SiliconGeneration::M5, M5_DIAGNOSTICS),
        ] {
            let (cpu, gpu) = chip.keys();
            let mut seen = std::collections::HashSet::new();
            for (name, keys) in diagnostics.iter().chain(COMMON_DIAGNOSTICS) {
                assert!(!name.is_empty());
                for key in *keys {
                    assert_eq!(key.len(), 4);
                    assert!(key.is_ascii());
                    assert!(seen.insert(key), "duplicate diagnostic key {key}: {chip:?}");
                    let (actual_name, group) = sensor_name(key, chip);
                    assert_ne!(actual_name, *key, "unlabelled key {key}: {chip:?}");
                    let expected = if cpu.contains(key) {
                        SensorGroup::Cpu
                    } else if gpu.contains(key) {
                        SensorGroup::Gpu
                    } else {
                        SensorGroup::Other
                    };
                    assert_eq!(
                        group, expected,
                        "control admission changed for {key}: {chip:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn semantics_are_case_sensitive_generation_scoped_and_bounded() {
        assert_eq!(sensor_name("Tm0p", SiliconGeneration::M4).0, "内存邻近温度");
        assert_eq!(sensor_name("Tm0p", SiliconGeneration::M5).0, "内存温度探头");
        assert_eq!(sensor_name("Tm0p", SiliconGeneration::M3).0, "Tm0p");
        assert_eq!(
            sensor_name("Tm0P", SiliconGeneration::M4).1,
            SensorGroup::System
        );
        assert_eq!(
            sensor_name("TH0a", SiliconGeneration::Unknown).0,
            "NVMe 存储温度"
        );
        assert_eq!(
            sensor_name("TPSD", SiliconGeneration::M4).0,
            "电源二极管温度"
        );
        assert_eq!(
            sensor_name("Ta00", SiliconGeneration::M5).0,
            "CPU 热余量（非绝对温度）"
        );
        // M4 Ta0p is ambient proximity. It does not establish semantics for its neighbours.
        for key in ["Ta01", "Ta05", "Ta09", "Ta0L", "Ta0P", "Ta0S"] {
            assert_eq!(
                sensor_name(key, SiliconGeneration::M4),
                (key.into(), SensorGroup::Other)
            );
        }
        for key in ["TPDZ", "TRDZ", "THZa", "TVSZ", "TpZZ", "TgZZ", "TmZZ"] {
            assert_eq!(
                sensor_name(key, SiliconGeneration::M4),
                (key.into(), SensorGroup::Other)
            );
        }
        assert_eq!(sensor_name("Tf04", SiliconGeneration::M4).0, "Tf04");
        assert_eq!(
            sensor_name("Tf04", SiliconGeneration::M5).1,
            SensorGroup::Other
        );
    }

    #[test]
    fn m1_ultra_recorded_keys_keep_unconfirmed_regions_and_control_groups_separate() {
        // Key-only capture from a read-only Apple M1 Ultra snapshot, 2026-10-08.
        // No temperature values, fan commands or private device identifier in the fixture.
        let keys: Vec<String> =
            serde_json::from_str(include_str!("../tests/fixtures/m1-ultra-sensor-keys.json"))
                .unwrap();
        let known = keys
            .iter()
            .filter(|key| sensor_name(key, SiliconGeneration::M1).0 != **key)
            .count();
        assert_eq!(keys.len(), 316);
        assert_eq!(known, 243);
        for key in ["TCDX", "Th00", "Th2I", "TPMP", "TSDD", "TT0D", "TT5D"] {
            let (name, group) = sensor_name(key, SiliconGeneration::M1);
            assert_ne!(name, key);
            assert_eq!(group, SensorGroup::Other);
        }
        // No first-hand source currently establishes these exact case-sensitive keys.
        for key in [
            "TC10", "TCA3", "Td00", "Td0M", "TB0p", "TB1p", "TIDP", "TMVC", "TSVL",
        ] {
            assert_eq!(
                sensor_name(key, SiliconGeneration::M1),
                (key.into(), SensorGroup::Other)
            );
        }
    }

    #[test]
    fn m4_mini_recorded_keys_have_semantics_except_unconfirmed_headroom_slots() {
        let readings: Vec<(String, f64)> =
            serde_json::from_str(include_str!("../tests/fixtures/m4-mini-temperatures.json"))
                .unwrap();
        let unresolved = ["Ta01", "Ta05", "Ta09", "Ta0L", "Ta0P", "Ta0S"];
        for (key, _) in readings {
            let (name, _) = sensor_name(&key, SiliconGeneration::M4);
            assert_eq!(
                name == key,
                unresolved.contains(&key.as_str()),
                "{key}: {name}"
            );
        }
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
