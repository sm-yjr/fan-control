//! One serial worker owns all sensor samples, configuration transitions and helper requests.
use crate::storage::{atomic_save, config_directory};
use fan_core::{
    ActionReason, Command, Config, Controller, Fan, FanConfig, HardwareMode, Sensor, SensorGroup,
    Snapshot, ThermalPressure, ThermalReading,
};
use fan_platform::{
    FanMode, HelperClient, PowerEvent, PowerMonitor, RawSnapshot, Smc, PROTOCOL_VERSION,
};
use objc2_foundation::NSProcessInfo;
use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct UiSnapshot {
    pub snapshot: Snapshot,
    pub thermal: ThermalReading,
    pub config: Config,
    pub helper_ready: bool,
    pub installing: bool,
    pub discovered: bool,
    pub message: String,
    pub fan_status: BTreeMap<u8, String>,
    pub battery: Option<fan_platform::BatteryReading>,
    pub configuration_notice: String,
    pub safety_active: bool,
    /// Requested and acknowledged targets; neither is the tachometer reading.
    pub targets: BTreeMap<u8, Option<u32>>,
    pub confirmed_targets: BTreeMap<u8, Option<u32>>,
    pub sample_age_secs: f64,
    pub published_at: Instant,
}
pub enum WorkerCommand {
    Configure(FanConfig),
    ConfigurePolicy(fan_core::ThermalPolicy),
    Reset,
    Uninstall,
    Uninstalled(fan_platform::HelperResponse),
    Install,
    Installed(fan_platform::HelperResponse),
    Retry,
    Visibility(bool),
    Export(PathBuf),
    Sleep(Sender<bool>),
    Wake,
    Shutdown(Sender<bool>),
}
pub struct Worker {
    sender: Sender<WorkerCommand>,
    state: Arc<Mutex<UiSnapshot>>,
}
impl Worker {
    pub fn start(demo: bool) -> Self {
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(Mutex::new(UiSnapshot {
            snapshot: empty_snapshot(0.),
            thermal: ThermalReading::default(),
            config: Config::default(),
            helper_ready: demo,
            installing: false,
            discovered: false,
            message: "正在检测硬件与控制服务…".into(),
            fan_status: BTreeMap::new(),
            battery: None,
            configuration_notice: String::new(),
            safety_active: false,
            targets: BTreeMap::new(),
            confirmed_targets: BTreeMap::new(),
            sample_age_secs: 0.,
            published_at: Instant::now(),
        }));
        let worker_state = state.clone();
        let worker_sender = sender.clone();
        thread::Builder::new()
            .name("fan-control-worker".into())
            .spawn(move || run(demo, worker_sender, receiver, worker_state))
            .expect("start sensor worker");
        Self { sender, state }
    }
    pub fn sender(&self) -> Sender<WorkerCommand> {
        self.sender.clone()
    }
    pub fn snapshot(&self) -> UiSnapshot {
        self.state.lock().expect("UI snapshot lock").clone()
    }
    pub fn send(&self, command: WorkerCommand) -> Result<(), String> {
        self.sender.send(command).map_err(|error| error.to_string())
    }
    pub fn shutdown(&self) -> bool {
        let (tx, rx) = mpsc::channel();
        if self.send(WorkerCommand::Shutdown(tx)).is_ok() {
            return rx.recv_timeout(Duration::from_secs(6)).unwrap_or(false);
        }
        false
    }
}
fn empty_snapshot(now: f64) -> Snapshot {
    Snapshot {
        sampled_at: now,
        fan_count: None,
        fans: Vec::new(),
        sensors: Vec::new(),
        thermal_pressure: ThermalPressure::Nominal,
        cpu_utilization_percent: None,
        machine_id: None,
    }
}
fn pressure() -> ThermalPressure {
    match NSProcessInfo::processInfo().thermalState().0 {
        0 => ThermalPressure::Nominal,
        1 => ThermalPressure::Fair,
        2 => ThermalPressure::Serious,
        3 => ThermalPressure::Critical,
        _ => ThermalPressure::Fair,
    }
}
pub fn convert_snapshot(raw: RawSnapshot, count: Option<f64>, now: f64) -> Snapshot {
    Snapshot {
        sampled_at: now,
        fan_count: count,
        thermal_pressure: pressure(),
        cpu_utilization_percent: None,
        machine_id: None,
        fans: raw
            .fans
            .into_iter()
            .map(|fan| Fan {
                id: fan.id,
                name: fan.name,
                min_rpm: fan.max_rpm_known.then_some(fan.min_rpm),
                max_rpm: fan.max_rpm_known.then_some(fan.max_rpm),
                current_rpm: fan.current_rpm_known.then_some(fan.current_rpm),
                mode: if !fan.mode_known {
                    HardwareMode::Unknown
                } else if fan.mode == FanMode::Forced {
                    HardwareMode::Forced
                } else {
                    HardwareMode::Automatic
                },
            })
            .collect(),
        sensors: raw
            .sensors
            .into_iter()
            .map(|sensor| Sensor {
                key: sensor.key,
                name: sensor.name,
                group: match sensor.group {
                    fan_platform::SensorGroup::Cpu => SensorGroup::Cpu,
                    fan_platform::SensorGroup::Gpu => SensorGroup::Gpu,
                    fan_platform::SensorGroup::System => SensorGroup::System,
                    fan_platform::SensorGroup::Other => SensorGroup::Other,
                },
                value: Some(sensor.value),
            })
            .collect(),
    }
}
fn demo_snapshot(now: f64, previous: &Snapshot) -> Snapshot {
    let mut snapshot = Snapshot {
        sampled_at: now,
        fan_count: Some(2.),
        thermal_pressure: ThermalPressure::Nominal,
        cpu_utilization_percent: Some(72. + (now / 15.).sin() * 18.),
        machine_id: Some("demo-device".into()),
        fans: vec![
            Fan {
                id: 0,
                name: "左侧风扇".into(),
                min_rpm: Some(1200.),
                max_rpm: Some(6000.),
                current_rpm: Some(1450.),
                mode: HardwareMode::Automatic,
            },
            Fan {
                id: 1,
                name: "右侧风扇".into(),
                min_rpm: Some(1200.),
                max_rpm: Some(6000.),
                current_rpm: Some(1470.),
                mode: HardwareMode::Automatic,
            },
        ],
        sensors: vec![
            Sensor {
                key: "Tp01".into(),
                name: "CPU 性能核心".into(),
                group: SensorGroup::Cpu,
                value: Some(58. + (now / 12.).sin() * 3.),
            },
            Sensor {
                key: "Tg05".into(),
                name: "GPU".into(),
                group: SensorGroup::Gpu,
                value: Some(48.),
            },
            Sensor {
                key: "Tm0P".into(),
                name: "主板".into(),
                group: SensorGroup::System,
                value: Some(36.),
            },
        ],
    };
    for fan in &mut snapshot.fans {
        if let Some(old) = previous.fans.iter().find(|old| old.id == fan.id) {
            fan.current_rpm = old.current_rpm;
            fan.mode = old.mode;
        }
    }
    snapshot
}
fn bundled_helper() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()?.parent().map(|contents| {
                contents.join("Library/LaunchServices/com.local.fan-control.helper")
            })
        })
        .unwrap_or_default()
}
const RECOVERY_MARKER: &str = "config.save-incomplete";
const PERSISTENCE_RETRY: Duration = Duration::from_secs(5);
const HANDBACK_RETRY: Duration = Duration::from_secs(2);
const TAKEOVER_HOLD: Duration = Duration::from_secs(60);
const TAKEOVER_HOLD_MAX: Duration = Duration::from_secs(600);
const TAKEOVER_FAILURE_MEMORY: Duration = Duration::from_secs(900);
const RECONNECTED_MESSAGE: &str = "控制服务已重新连接，正在核对硬件状态。";

/// A failed directory fsync can follow a successful rename. The durable marker
/// makes that uncertain configuration automatic-only on the next startup.
fn save_config_at(directory: &Path, config: &Config) -> Result<Option<String>, String> {
    let json = config.to_json().map_err(|e| e.to_string())?;
    let marker = directory.join(RECOVERY_MARKER);
    atomic_save(
        &marker,
        b"Custom control must not resume until this save is confirmed.\n",
    )
    .map_err(|e| format!("无法创建配置恢复标记，原配置未覆盖：{e}"))?;
    atomic_save(&directory.join("config.json"), json.as_bytes())
        .map_err(|e| format!("配置保存未获持久确认，下次启动将使用系统自动：{e}"))?;
    // The configuration itself is now durable. Cleanup failure does not undo
    // this confirmed save, and a residual marker only makes startup safer.
    match fs::remove_file(marker).and_then(|()| fs::File::open(directory)?.sync_all()) {
        Ok(()) => Ok(None),
        Err(error) => Ok(Some(format!(
            "配置已保存，但恢复标记清理未确认；下次启动可能使用系统自动：{error}"
        ))),
    }
}
fn backup_original(directory: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut path = directory.join("config.pre-rust.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if metadata.len() <= 1_048_576
                && fs::read(&path).map_err(|error| format!("原配置备份无法读取：{error}"))? == bytes
            {
                return Ok(());
            }
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos();
            path = directory.join(format!("config.pre-rust.{suffix}.json"));
        }
        Ok(_) => return Err("原配置备份路径不是普通文件，禁止覆盖原配置。".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法检查原配置备份：{error}")),
    }
    atomic_save(&path, bytes).map_err(|error| format!("无法备份原有配置，禁止覆盖：{error}"))
}

fn read_original(directory: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = directory.join("config.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if metadata.len() > 1_048_576 {
                return Err("配置超过 1 MiB，禁止覆盖；请修复原文件。".into());
            }
            fs::read(path).map(Some).map_err(|error| error.to_string())
        }
        Ok(_) => Err("配置路径必须是普通文件，禁止覆盖。".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn recovery_required(directory: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(directory.join(RECOVERY_MARKER)) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err("配置恢复标记不是普通文件，禁止覆盖。".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("无法检查配置恢复标记：{error}")),
    }
}

fn decode_config(original: Option<&[u8]>) -> (Config, String) {
    let Some(bytes) = original else {
        return (Config::default(), String::new());
    };
    match std::str::from_utf8(bytes)
        .map_err(|e| e.to_string())
        .and_then(|json| Config::from_json(json).map_err(|e| e.to_string()))
    {
        Ok(load) => {
            let mut notices = load.warnings;
            if load.migrated {
                notices.insert(0, "旧版配置已读取；首次保存前将保留备份。".into());
            }
            (load.config, notices.join(" "))
        }
        Err(error) => (
            Config::default(),
            format!("配置读取失败，已使用系统自动；原文件将先备份再保存：{error}"),
        ),
    }
}

struct Persistence {
    directory: PathBuf,
    demo: bool,
    original: Option<Vec<u8>>,
    original_backed_up: bool,
    read_blocked: bool,
    base_notice: String,
    save_error: Option<String>,
    cleanup_notice: Option<String>,
    pending: bool,
    next_retry: Instant,
}
impl Persistence {
    fn load(directory: PathBuf, demo: bool) -> (Self, Config) {
        let now = Instant::now();
        let read = if demo {
            Ok(None)
        } else {
            read_original(&directory)
        };
        let recovery = if demo {
            Ok(false)
        } else {
            recovery_required(&directory)
        };
        let (original, read_error) = match read {
            Ok(original) => (original, None),
            Err(error) => (None, Some(error)),
        };
        let (mut config, mut base_notice) = decode_config(original.as_deref());
        let (recover, recovery_error) = match recovery {
            Ok(recover) => (recover, None),
            Err(error) => (false, Some(error)),
        };
        let read_error = read_error.or(recovery_error);
        let read_blocked = read_error.is_some();
        if let Some(error) = read_error {
            config = Config::default();
            base_notice =
                format!("原配置无法安全读取，已禁用自定义控制；禁止覆盖并将重试读取：{error}");
        } else if recover {
            for fan in &mut config.fans {
                fan.mode = fan_core::ControlMode::Automatic;
            }
            base_notice.push_str(" 上次保存未完成，已使用系统自动并将重试保存安全配置。");
        }
        (
            Self {
                directory,
                demo,
                original,
                original_backed_up: false,
                read_blocked,
                base_notice,
                save_error: None,
                cleanup_notice: None,
                pending: recover && !read_blocked,
                next_retry: now + PERSISTENCE_RETRY,
            },
            config,
        )
    }
    fn notice(&self) -> String {
        let mut notice: String = match &self.save_error {
            Some(error) => format!("{} 保存失败，将重试当前设置：{error}", self.base_notice)
                .trim()
                .into(),
            None => self.base_notice.clone(),
        };
        if let Some(cleanup) = &self.cleanup_notice {
            notice.push(' ');
            notice.push_str(cleanup);
        }
        notice.trim().into()
    }
    fn save(&mut self, config: &Config) -> Result<(), String> {
        let result = if self.demo {
            Ok(None)
        } else if self.read_blocked {
            Err("原配置尚无法读取和备份，已阻止覆盖。".into())
        } else {
            (|| {
                // Recheck before every replacement: permissions or an external
                // edit may have changed since application startup.
                let current = read_original(&self.directory).map_err(|error| {
                    self.read_blocked = true;
                    format!("原配置现在无法安全读取，已阻止覆盖：{error}")
                })?;
                if current != self.original {
                    self.original = current;
                    self.original_backed_up = false;
                }
                if !self.original_backed_up {
                    if let Some(bytes) = &self.original {
                        backup_original(&self.directory, bytes)?;
                    }
                    self.original_backed_up = true;
                }
                save_config_at(&self.directory, config)
            })()
        };
        self.next_retry = Instant::now() + PERSISTENCE_RETRY;
        match result {
            Ok(cleanup_notice) => {
                self.pending = cleanup_notice.is_some();
                self.save_error = None;
                self.cleanup_notice = cleanup_notice;
                self.base_notice = self
                    .base_notice
                    .replace(
                        "旧版配置已读取；首次保存前将保留备份。",
                        "旧版配置已迁移，原文件已备份。",
                    )
                    .replace(
                        "上次保存未完成，已使用系统自动并将重试保存安全配置。",
                        "上次保存未完成；现已保存系统自动配置，原文件已备份。",
                    );
                Ok(())
            }
            Err(error) => {
                self.pending = true;
                self.save_error = Some(error.clone());
                self.cleanup_notice = None;
                Err(error)
            }
        }
    }
    fn retry(&mut self, now: Instant, accepted_config: &Config) {
        if now < self.next_retry || (!self.pending && !self.read_blocked) {
            return;
        }
        self.next_retry = now + PERSISTENCE_RETRY;
        if self.read_blocked {
            match read_original(&self.directory)
                .and_then(|original| Ok((original, recovery_required(&self.directory)?)))
            {
                Ok((original, recovery)) => {
                    self.original = original;
                    self.original_backed_up = false;
                    self.read_blocked = false;
                    self.pending |= recovery;
                    let (_, warning) = decode_config(self.original.as_deref());
                    self.base_notice = format!(
                        "原配置现已可读取；为避免意外恢复，自定义设置需重新应用。 {warning}"
                    );
                }
                Err(error) => {
                    self.base_notice = format!("原配置仍无法安全读取，禁止覆盖：{error}");
                    return;
                }
            }
        }
        if self.pending {
            // The rejected custom candidate is never replayed here. Only the
            // controller's currently accepted configuration may be persisted.
            let _ = self.save(accepted_config);
        }
    }
}

#[derive(Default)]
struct Handback {
    active: bool,
    controlled: bool,
    next_retry: Option<Instant>,
    failure: String,
    /// After a failed write, custom takeover waits so a fan is not started and
    /// stopped repeatedly while firmware or the helper's unlock cooldown settles.
    hold_until: Option<Instant>,
    failures: u32,
    last_failure_at: Option<Instant>,
}
impl Handback {
    fn record_failure(&mut self, now: Instant) {
        self.failures = if self
            .last_failure_at
            .is_some_and(|last| now.saturating_duration_since(last) < TAKEOVER_FAILURE_MEMORY)
        {
            self.failures.saturating_add(1)
        } else {
            1
        };
        self.last_failure_at = Some(now);
        let hold = TAKEOVER_HOLD
            .saturating_mul(1 << (self.failures - 1).min(4))
            .min(TAKEOVER_HOLD_MAX);
        self.hold_until = Some(now + hold);
    }
    fn hold_remaining(&self, now: Instant) -> Option<Duration> {
        self.hold_until
            .map(|until| until.saturating_duration_since(now))
            .filter(|remaining| !remaining.is_zero())
    }
    fn require(&mut self, failure: impl Into<String>, now: Instant) {
        self.active = true;
        self.failure = failure.into();
        self.next_retry = Some(now);
    }
    fn due(&self, now: Instant) -> bool {
        self.active && self.next_retry.is_none_or(|deadline| now >= deadline)
    }
    fn heartbeat_allowed(&self) -> bool {
        !self.active
    }
    fn accepts(&self, command: &Command) -> bool {
        !self.active || matches!(command, Command::SetAutomatic)
    }
    fn record_write(
        &mut self,
        command: &Command,
        response: &fan_platform::HelperResponse,
        now: Instant,
    ) -> bool {
        if matches!(command, Command::SetRpm { .. }) {
            self.controlled = true;
        }
        let confirmed = response_confirms(command, response);
        if !confirmed {
            // Helper replies carry SMC and IPC details only, never user data.
            crate::telemetry::event(&format!(
                "control.write.unconfirmed command={command:?} ok={} actual={:?} message={}",
                response.ok, response.actual_rpm, response.message
            ));
            let failure = if response.ok {
                "硬件回执无法确认请求，正在恢复系统自动".to_string()
            } else {
                format!("写入失败：{}", response.message)
            };
            self.require(failure, now);
            self.record_failure(now);
        }
        confirmed
    }
    fn acknowledge_reset(&mut self, response: &fan_platform::HelperResponse, now: Instant) -> bool {
        // reset_all authenticates its peer and reads back the known fan modes,
        // including remembered IDs when a current FNum read fails.
        if response_confirms(&Command::SetAutomatic, response) {
            self.active = false;
            self.controlled = false;
            self.next_retry = None;
            self.failure.clear();
            true
        } else {
            self.active = true;
            self.next_retry = Some(now + HANDBACK_RETRY);
            self.failure = response.message.clone();
            false
        }
    }
}
fn run(
    demo: bool,
    sender: Sender<WorkerCommand>,
    receiver: Receiver<WorkerCommand>,
    shared: Arc<Mutex<UiSnapshot>>,
) {
    crate::telemetry::event(if demo {
        "gui.demo.start"
    } else {
        "gui.worker.start"
    });
    let origin = Instant::now();
    let client = HelperClient::default();
    let mut cpu_monitor = if demo {
        None
    } else {
        fan_platform::CpuLoadMonitor::new().ok()
    };
    let mut machine_id = if demo {
        Some("demo-device".into())
    } else {
        fan_platform::read_device_identity()
            .ok()
            .flatten()
            .map(|identity| identity.scope_id)
    };
    let (mut persistence, config) = Persistence::load(config_directory(), demo);
    let mut message = String::new();
    let mut controller = Controller::new(config);
    let mut handback = Handback::default();
    let mut smc = if demo {
        None
    } else {
        match Smc::open() {
            Ok(smc) => Some(smc),
            Err(error) => {
                message = format!("SMC 无法连接：{error}。点击重新检测可重试。");
                None
            }
        }
    };
    let mut snapshot = empty_snapshot(0.);
    let mut discovered = false;
    let mut installing = false;
    let mut ready = demo;
    let mut paused = false;
    let mut visible = true;
    let mut next_heartbeat = Instant::now();
    let mut next_sample = Instant::now();
    let mut wake_started: Option<Instant> = None;
    let mut wake_retries = 0;
    let mut statuses = BTreeMap::new();
    let mut battery = None;
    let power_sender = sender.clone();
    let _power = if demo {
        None
    } else {
        PowerMonitor::register(move |event| match event {
            PowerEvent::WillSleep => {
                let (tx, rx) = mpsc::channel();
                if power_sender.send(WorkerCommand::Sleep(tx)).is_ok() {
                    let _ = rx.recv_timeout(Duration::from_secs(6));
                }
            }
            PowerEvent::WillWake | PowerEvent::DidWake => {
                let _ = power_sender.send(WorkerCommand::Wake);
            }
        })
        .ok()
    };
    let power_ready = demo || _power.is_some();
    loop {
        let timeout = if paused {
            Duration::from_millis(500)
        } else {
            next_sample
                .min(next_heartbeat)
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(500))
        };
        match receiver.recv_timeout(timeout) {
            Ok(WorkerCommand::Configure(fan)) => {
                if installing && !matches!(fan.mode, fan_core::ControlMode::Automatic) {
                    message = "正在安装或移除控制服务，请完成授权后再应用自定义设置。".into();
                    continue;
                }
                crate::telemetry::event("control.configure");
                let custom = !matches!(fan.mode, fan_core::ControlMode::Automatic);
                match configure(&mut controller, fan, |candidate| {
                    persistence.save(candidate)
                }) {
                    Ok(()) => message = "设置已保存，等待硬件确认…".into(),
                    Err(error) => {
                        message = error;
                        if custom && persistence.save_error.is_some() {
                            let notice = "本次自定义设置未应用；恢复保存后请重新应用。";
                            if !persistence.base_notice.contains(notice) {
                                persistence.base_notice.push(' ');
                                persistence.base_notice.push_str(notice);
                            }
                        }
                    }
                }
                next_sample = Instant::now();
            }
            Ok(WorkerCommand::ConfigurePolicy(policy)) => {
                if installing && policy.comfort_target_celsius.is_some() {
                    message = "正在安装或移除控制服务，请完成后再更新智能策略。".into();
                    continue;
                }
                let result = configure_policy(&mut controller, policy, |candidate| {
                    persistence.save(candidate)
                });
                message = match result {
                    Ok(()) => "智能策略已保存，等待新采样评估。".into(),
                    Err(error) => error,
                };
                next_sample = Instant::now();
            }
            Ok(WorkerCommand::Reset) => {
                // Stop custom control in memory even if persistence fails.
                let persist =
                    reset_configuration(&mut controller, |config| persistence.save(config));
                handback.require("用户请求交还系统", Instant::now());
                message = "正在交还系统自动控制…".into();
                if let Err(error) = persist {
                    message.push_str(&format!(" 配置保存失败，重启前请修复：{error}"));
                }
                next_sample = Instant::now();
            }
            Ok(WorkerCommand::Uninstall) => {
                if !demo && !installing {
                    let mut config = controller.config().clone();
                    for fan in &mut config.fans {
                        fan.mode = fan_core::ControlMode::Automatic;
                    }
                    let _ = controller.replace_config(config.clone());
                    if let Err(error) = persistence.save(&config) {
                        message = error;
                    } else {
                        message = "正在交还系统并移除控制服务，等待管理员授权…".into();
                    }
                    handback.require("移除控制服务前交还系统", Instant::now());
                    let response = client.reset_all();
                    if handback.acknowledge_reset(&response, Instant::now()) {
                        controller.resume();
                    }
                    installing = true;
                    let tx = sender.clone();
                    thread::spawn(move || {
                        let response = fan_platform::uninstall_helper();
                        let _ = tx.send(WorkerCommand::Uninstalled(response));
                    });
                }
            }
            Ok(WorkerCommand::Uninstalled(response)) => {
                installing = false;
                ready = false;
                message = if response.ok {
                    "控制服务已移除。应用继续以只读模式运行。".into()
                } else {
                    format!("移除服务未完成：{}", response.message)
                };
                next_sample = Instant::now();
            }
            Ok(WorkerCommand::Install) => {
                if !demo && !installing {
                    installing = true;
                    message = "等待管理员授权。取消后仍可查看温度。".into();
                    let tx = sender.clone();
                    let helper = bundled_helper();
                    thread::spawn(move || {
                        let response = fan_platform::install_bundled_helper(&helper);
                        let _ = tx.send(WorkerCommand::Installed(response));
                    });
                }
            }
            Ok(WorkerCommand::Installed(response)) => {
                installing = false;
                message = if response.ok {
                    "控制服务已安装并验证。".into()
                } else {
                    format!("安装未完成：{}", response.message)
                };
                next_sample = Instant::now();
            }
            Ok(WorkerCommand::Visibility(value)) => {
                visible = value;
                next_sample = Instant::now();
            }
            Ok(WorkerCommand::Retry) => {
                if !demo {
                    smc = Smc::open().ok();
                    if machine_id.is_none() {
                        machine_id = fan_platform::read_device_identity()
                            .ok()
                            .flatten()
                            .map(|identity| identity.scope_id);
                    }
                    if cpu_monitor.is_none() {
                        cpu_monitor = fan_platform::CpuLoadMonitor::new().ok();
                    }
                }
                discovered = false;
                persistence.next_retry = Instant::now();
                message = "正在重新检测…".into();
                next_sample = Instant::now();
            }
            Ok(WorkerCommand::Export(path)) => {
                let report_config = diagnostic_config(controller.config());
                let report = serde_json::json!({"app_version":crate::app_version::current(),"helper_protocol":PROTOCOL_VERSION,"demo":demo,"helper_ready":ready,"snapshot":snapshot,"config":report_config,"thermal":controller.thermal_reading(),"fan_status":statuses,"configuration_notice":persistence.notice(),"safety_active":handback.active,"sample_age_secs":(origin.elapsed().as_secs_f64()-snapshot.sampled_at).max(0.)});
                message = match serde_json::to_vec_pretty(&report)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        crate::storage::atomic_export(&path, &bytes).map_err(|e| e.to_string())
                    }) {
                    Ok(()) => "诊断已导出。".into(),
                    Err(e) => format!("导出失败：{e}"),
                };
            }
            Ok(WorkerCommand::Sleep(reply)) => {
                crate::telemetry::event("power.sleep.handback");
                paused = true;
                if let Some(monitor) = cpu_monitor.as_mut() {
                    monitor.reset();
                }
                let ok = if demo || (!handback.controlled && !handback.active) {
                    handback = Handback::default();
                    true
                } else {
                    handback.require("睡眠前交还系统", Instant::now());
                    let response = client.reset_all();
                    handback.acknowledge_reset(&response, Instant::now())
                };
                if ok {
                    controller.resume();
                }
                let _ = reply.send(ok);
                message = if ok {
                    "睡眠前已交还系统。".into()
                } else {
                    "睡眠前交还未获确认，请检查 helper。".into()
                };
            }
            Ok(WorkerCommand::Wake) => {
                crate::telemetry::event("power.wake.restore");
                paused = false;
                if let Some(monitor) = cpu_monitor.as_mut() {
                    monitor.reset();
                }
                wake_started = Some(Instant::now());
                wake_retries = 0;
                let config = controller.config().clone();
                controller = Controller::new(config);
                next_sample = Instant::now();
                message = "正在恢复唤醒前设置…".into();
            }
            Ok(WorkerCommand::Shutdown(reply)) => {
                crate::telemetry::event("gui.shutdown.handback");
                // A read-only session did not acquire a control lease; an old
                // or absent helper need not produce a misleading exit error.
                let ok = if demo || (!handback.controlled && !handback.active) {
                    true
                } else {
                    handback.require("退出前交还系统", Instant::now());
                    let response = client.reset_all();
                    handback.acknowledge_reset(&response, Instant::now())
                };
                let _ = reply.send(ok);
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !demo && (handback.controlled || handback.active) {
                    let _ = client.reset_all();
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if next_sample <= Instant::now() && !paused {
            let sample_started = origin.elapsed().as_secs_f64();
            let cpu_load = cpu_monitor
                .as_mut()
                .and_then(fan_platform::CpuLoadMonitor::sample);
            battery = if demo {
                Some(fan_platform::BatteryReading {
                    charge_percent: Some(82.),
                    is_charging: Some(false),
                    is_on_ac: Some(true),
                    cycle_count: Some(120),
                    health_percent: Some(97.),
                    health_is_estimate: true,
                    power_watts: Some(0.),
                    adapter_watts: Some(96.),
                })
            } else {
                fan_platform::read_battery().ok().flatten()
            };
            if demo {
                snapshot = demo_snapshot(sample_started, &snapshot);
                discovered = true;
                ready = true;
            } else {
                let was_ready = ready;
                let helper = client.status();
                ready = helper.ok
                    && helper.is_root
                    && helper.protocol_version == Some(PROTOCOL_VERSION)
                    && power_ready;
                if !power_ready {
                    message = "无法注册睡眠保护，控制保持禁用；请重新启动应用。".into();
                } else if ready {
                    // Heartbeat is scheduled independently of background sensor polling.
                    if !was_ready {
                        // Per-fan "unavailable" text belongs to the previous session.
                        statuses.clear();
                        message = RECONNECTED_MESSAGE.into();
                    } else if message == RECONNECTED_MESSAGE {
                        // A second ready sample has re-read hardware; fall through to live status.
                        message.clear();
                    }
                } else if helper.ok {
                    message = format!(
                        "控制服务需要更新（协议 {} → {}）。点击启用 / 修复。",
                        helper
                            .protocol_version
                            .map_or_else(|| "未知".to_string(), |version| version.to_string()),
                        PROTOCOL_VERSION
                    );
                } else if !installing {
                    message = format!(
                        "控制服务不可用：{}。可查看温度；点击启用 / 修复以恢复控制。",
                        helper.message
                    );
                }
                if let Some(smc) = smc.as_mut() {
                    let count = smc.read_value("FNum").ok();
                    let raw = if discovered {
                        smc.refresh_snapshot()
                    } else {
                        smc.discover_snapshot()
                    };
                    match raw {
                        Ok(raw) => {
                            // Include battery, IPC and SMC sampling time in the freshness budget.
                            snapshot = convert_snapshot(raw, count, sample_started);
                            discovered = true;
                        }
                        Err(error) => {
                            snapshot = empty_snapshot(sample_started);
                            message = format!("传感器读取失败：{error}，自定义控制将安全回退。");
                        }
                    }
                } else {
                    snapshot = empty_snapshot(sample_started);
                }
                snapshot.cpu_utilization_percent = cpu_load;
                snapshot.machine_id = machine_id.clone();
            }
            if !ready {
                for value in statuses.values_mut() {
                    *value = "控制服务不可用，等待安全回退".into();
                }
                if handback.controlled && !handback.active {
                    handback.require("控制服务失联，交还系统尚未确认", Instant::now());
                }
            }
            // The core evaluates data even while writes are unavailable, keeping thermal UI current.
            let actions = controller.update(&snapshot, origin.elapsed().as_secs_f64());
            for action in actions {
                // A complete reset must be confirmed before any saved custom
                // intent is resumed. Discard this batch on the first failure.
                if handback.active {
                    break;
                }
                // After a failed write, wait before taking a fan over again;
                // emergency cooling is never held back.
                if !demo
                    && matches!(action.command, Command::SetRpm { .. })
                    && action.reason != ActionReason::Emergency
                    && handback.hold_remaining(Instant::now()).is_some()
                {
                    controller.defer(&action);
                    statuses.insert(action.fan_id, crate::presenter::HELD_STATUS.into());
                    continue;
                }
                let allowed = handback.accepts(&action.command)
                    && action_allowed(
                        &action.command,
                        ready,
                        installing,
                        &snapshot,
                        origin.elapsed().as_secs_f64(),
                    );
                let mut attempted = false;
                let ok = if demo {
                    if let Some(fan) = snapshot.fans.iter_mut().find(|f| f.id == action.fan_id) {
                        match action.command {
                            Command::SetAutomatic => {
                                fan.mode = HardwareMode::Automatic;
                                fan.current_rpm = Some(1450.);
                            }
                            Command::SetRpm { rpm, .. } => {
                                fan.mode = HardwareMode::Forced;
                                fan.current_rpm = Some(rpm as f64);
                            }
                        }
                    }
                    true
                } else if allowed {
                    attempted = true;
                    if matches!(action.command, Command::SetRpm { .. }) {
                        // Failure can follow a partial write, so an attempted
                        // RPM command also requires confirmed shutdown reset.
                        handback.controlled = true;
                    }
                    let response = match action.command {
                        Command::SetAutomatic => {
                            client.set_fan_mode(i64::from(action.fan_id), FanMode::Automatic)
                        }
                        Command::SetRpm { rpm, .. } => {
                            client.set_fan_rpm(i64::from(action.fan_id), i64::from(rpm))
                        }
                    };
                    if !response.ok {
                        message =
                            format!("风扇 {} 设置失败：{}", action.fan_id + 1, response.message);
                    }
                    let confirmed =
                        handback.record_write(&action.command, &response, Instant::now());
                    if response.ok && !confirmed {
                        message = "硬件确认的目标与请求不一致，正在恢复系统自动。".into();
                    }
                    confirmed
                } else {
                    false
                };
                controller.acknowledge(&action, ok, origin.elapsed().as_secs_f64());
                statuses.insert(
                    action.fan_id,
                    if ok {
                        format!("目标已确认 · {}", reason_label(action.reason))
                    } else if attempted {
                        "写入失败，已请求安全回退".into()
                    } else {
                        "等待控制服务".into()
                    },
                );
                if ok {
                    message = "控制目标已回读确认，当前 RPM 将随硬件响应变化。".into();
                }
                if ok && action.reason == ActionReason::Emergency {
                    message =
                        "安全保护已接管：温度或系统热压力过高。用户设置将在安全后恢复。".into();
                }
                if attempted && !ok {
                    handback.require(message.clone(), Instant::now());
                    break;
                }
            }
            if message.is_empty() {
                message = if demo {
                    "演示模式：所有控制均为模拟，无硬件写入。".into()
                } else if ready
                    && snapshot.fresh(
                        origin.elapsed().as_secs_f64(),
                        fan_core::SNAPSHOT_MAXIMUM_AGE,
                    )
                {
                    "控制服务就绪 · 数据实时更新".into()
                } else {
                    "仅监测模式".into()
                };
            }
            next_sample = Instant::now()
                + Duration::from_secs_f64(fan_core::polling_interval(
                    visible,
                    snapshot.thermal_pressure,
                    snapshot.hottest_silicon(),
                    controller.polling_activity(),
                ));
        }
        if !paused && next_heartbeat <= Instant::now() {
            if !demo && ready && handback.heartbeat_allowed() {
                let response = client.heartbeat();
                if !response_confirms(&Command::SetAutomatic, &response) {
                    ready = false;
                    message = format!("控制服务失联：{}。服务将按租约交还系统。", response.message);
                    if handback.controlled {
                        handback.require(message.clone(), Instant::now());
                    }
                    for value in statuses.values_mut() {
                        *value = "控制服务失联，等待安全回退".into();
                    }
                }
            }
            next_heartbeat = Instant::now() + Duration::from_secs(2);
        }
        if handback.due(Instant::now()) {
            let confirmed = if demo {
                for fan in &mut snapshot.fans {
                    fan.mode = HardwareMode::Automatic;
                    fan.current_rpm = Some(1450.);
                }
                handback = Handback::default();
                true
            } else {
                let response = client.reset_all();
                handback.acknowledge_reset(&response, Instant::now())
            };
            if confirmed {
                controller.resume();
                for status in statuses.values_mut() {
                    *status = "已确认交还系统自动".into();
                }
                message = match handback.hold_remaining(Instant::now()) {
                    Some(remaining) => format!(
                        "已确认交还系统。为避免风扇反复启停，{} 秒后再尝试接管。",
                        remaining.as_secs().max(1)
                    ),
                    None => "已回读确认交还系统；将以新采样重新评估设置。".into(),
                };
                next_sample = Instant::now();
            }
        }
        persistence.retry(Instant::now(), controller.config());
        if let Some(wake) = wake_started {
            let delays = [0.5, 2., 8., 20., 60.];
            if wake_retries < delays.len() && wake.elapsed().as_secs_f64() >= delays[wake_retries] {
                let config = controller.config().clone();
                controller = Controller::new(config);
                wake_retries += 1;
                next_sample = Instant::now();
            }
            if wake_retries == delays.len() {
                wake_started = None;
            }
        }
        let mut targets = BTreeMap::new();
        let mut confirmed_targets = BTreeMap::new();
        let mut safety_active = handback.active;
        let ids = snapshot
            .fans
            .iter()
            .map(|fan| fan.id)
            .chain(controller.config().fans.iter().map(|fan| fan.fan_id));
        for id in ids {
            if let Some(status) = controller.fan_status(id) {
                targets.insert(id, status.desired_rpm);
                confirmed_targets.insert(id, status.applied_rpm);
                safety_active |= status.safety_override;
                if !handback.active
                    && ready
                    && status.last_failure.is_none()
                    && status.pending.is_none()
                {
                    if let Some(reason) = status.status_reason {
                        statuses.insert(id, reason_label(reason).into());
                    }
                }
            }
        }
        if handback.active {
            message = format!(
                "安全回退未获确认：{}。已暂停续租和 RPM 写入，正在重试交还系统。",
                handback.failure
            );
            for status in statuses.values_mut() {
                *status = "交还系统未确认，RPM 写入已暂停".into();
            }
        }
        let published_at = Instant::now();
        *shared.lock().expect("worker snapshot lock") = UiSnapshot {
            snapshot: snapshot.clone(),
            thermal: controller.thermal_reading().clone(),
            config: controller.config().clone(),
            helper_ready: ready,
            installing,
            discovered,
            message: message.clone(),
            fan_status: statuses.clone(),
            battery: battery.clone(),
            configuration_notice: persistence.notice(),
            safety_active,
            targets,
            confirmed_targets,
            sample_age_secs: (origin.elapsed().as_secs_f64() - snapshot.sampled_at).max(0.),
            published_at,
        };
    }
}

fn reason_label(reason: ActionReason) -> &'static str {
    match reason {
        ActionReason::LowDemandSystem => "低热负荷，交还系统决定停转",
        ActionReason::AdaptivePerformance => "持续负载，提前提高散热",
        ActionReason::AdaptiveTrend => "芯片温度趋势预测，提前散热",
        ActionReason::AdaptiveHeatSoak => "持续热浸或冷却驻留",
        ActionReason::AdaptiveTemperature => "智能温度调节",
        ActionReason::SurfaceComfort => "已校准舒适策略",
        ActionReason::Emergency => "安全保护已接管",
        ActionReason::MissingInput | ActionReason::StaleSnapshot => "数据失效，恢复系统自动",
        ActionReason::InvalidBounds => "硬件上限未知，恢复系统自动",
        ActionReason::UnknownHardwareMode => "读取状态失败，恢复系统自动",
        ActionReason::WriteFailure => "写入失败，恢复系统自动",
        ActionReason::Sleep => "睡眠前交还系统",
        ActionReason::Shutdown => "退出前交还系统",
        _ => "设置已生效",
    }
}

fn configure(
    controller: &mut Controller,
    fan: FanConfig,
    save: impl FnOnce(&Config) -> Result<(), String>,
) -> Result<(), String> {
    let mut candidate = controller.config().clone();
    candidate.fans.retain(|old| old.fan_id != fan.fan_id);
    candidate.fans.push(fan.clone());
    candidate.validate().map_err(|e| e.to_string())?;
    if matches!(fan.mode, fan_core::ControlMode::Automatic) {
        // Safety handback takes priority over durable storage.
        controller.set_fan_config(fan).map_err(|e| e.to_string())?;
        save(&candidate).map_err(|e| format!("自定义控制已停止；保存失败，重启前请修复：{e}"))
    } else {
        save(&candidate)?;
        controller.set_fan_config(fan).map_err(|e| e.to_string())
    }
}
fn configure_policy(
    controller: &mut Controller,
    policy: fan_core::ThermalPolicy,
    save: impl FnOnce(&Config) -> Result<(), String>,
) -> Result<(), String> {
    let mut candidate = controller.config().clone();
    candidate.thermal_policy = policy;
    candidate.validate().map_err(|error| error.to_string())?;
    save(&candidate)?;
    controller
        .replace_config(candidate)
        .map_err(|error| error.to_string())
}
fn diagnostic_config(config: &Config) -> serde_json::Value {
    let mut report = serde_json::to_value(config).unwrap_or_default();
    if let Some(calibration) = report
        .get_mut("thermal_policy")
        .and_then(|policy| policy.get_mut("calibration"))
        .and_then(serde_json::Value::as_object_mut)
    {
        calibration.remove("machine_id");
    }
    report
}
fn action_allowed(
    command: &Command,
    ready: bool,
    maintenance: bool,
    snapshot: &Snapshot,
    now: f64,
) -> bool {
    ready
        && (matches!(command, Command::SetAutomatic)
            || (!maintenance && snapshot.fresh(now, fan_core::SNAPSHOT_MAXIMUM_AGE)))
}
fn reset_configuration(
    controller: &mut Controller,
    save: impl FnOnce(&Config) -> Result<(), String>,
) -> Result<(), String> {
    let mut config = controller.config().clone();
    for fan in &mut config.fans {
        fan.mode = fan_core::ControlMode::Automatic;
    }
    controller
        .replace_config(config.clone())
        .map_err(|e| e.to_string())?;
    save(&config)
}
fn response_confirms(command: &Command, response: &fan_platform::HelperResponse) -> bool {
    response.ok
        && response.is_root
        && response.protocol_version == Some(PROTOCOL_VERSION)
        && match command {
            Command::SetRpm { rpm, .. } => response.actual_rpm.map(u32::from) == Some(*rpm),
            Command::SetAutomatic => true,
        }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn response(ok: bool, rpm: Option<u16>) -> fan_platform::HelperResponse {
        fan_platform::HelperResponse {
            ok,
            message: if ok { "verified" } else { "FNum read failed" }.into(),
            is_root: true,
            protocol_version: Some(PROTOCOL_VERSION),
            actual_rpm: rpm,
        }
    }
    fn temporary_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fan-worker-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn manual() -> Controller {
        Controller::new(Config {
            version: fan_core::CONFIG_VERSION,
            thermal_policy: fan_core::ThermalPolicy::default(),
            fans: vec![FanConfig {
                fan_id: 0,
                mode: fan_core::ControlMode::Manual { rpm: 1800 },
                curve: None,
            }],
        })
    }
    #[test]
    fn failed_writes_hold_takeover_with_bounded_backoff() {
        let now = Instant::now();
        let mut handback = Handback::default();
        let rpm = Command::SetRpm {
            rpm: 2000,
            allow_fan_off: false,
        };
        assert!(handback.hold_remaining(now).is_none());
        assert!(handback.record_write(&rpm, &response(true, Some(2000)), now));
        assert!(handback.hold_remaining(now).is_none());
        assert!(!handback.record_write(&rpm, &response(false, None), now));
        assert_eq!(handback.hold_remaining(now), Some(TAKEOVER_HOLD));
        assert!(handback.hold_remaining(now + TAKEOVER_HOLD).is_none());
        // Repeated failures double the wait up to the cap.
        let later = now + TAKEOVER_HOLD;
        assert!(!handback.record_write(&Command::SetAutomatic, &response(false, None), later));
        assert_eq!(handback.hold_remaining(later), Some(TAKEOVER_HOLD * 2));
        let mut at = later;
        for _ in 0..8 {
            at += Duration::from_secs(1);
            handback.record_write(&rpm, &response(false, None), at);
        }
        assert_eq!(handback.hold_remaining(at), Some(TAKEOVER_HOLD_MAX));
        // A long quiet period forgets earlier failures.
        let quiet = at + TAKEOVER_FAILURE_MEMORY;
        handback.record_write(&rpm, &response(false, None), quiet);
        assert_eq!(handback.hold_remaining(quiet), Some(TAKEOVER_HOLD));
    }
    #[test]
    fn invalid_config_never_reaches_storage() {
        let mut controller = manual();
        let result = configure(
            &mut controller,
            FanConfig {
                fan_id: 0,
                mode: fan_core::ControlMode::Manual { rpm: 0 },
                curve: None,
            },
            |_| panic!("invalid candidate must not be saved"),
        );
        assert!(result.is_err());
        assert!(matches!(
            controller.config().fans[0].mode,
            fan_core::ControlMode::Manual { rpm: 1800 }
        ));
    }
    #[test]
    fn save_failure_does_not_apply_a_new_custom_mode() {
        let mut controller = manual();
        assert!(configure(&mut controller, FanConfig::balanced(0), |_| Err(
            "disk full".into()
        ))
        .is_err());
        assert!(matches!(
            controller.config().fans[0].mode,
            fan_core::ControlMode::Manual { rpm: 1800 }
        ));
    }
    #[test]
    fn reset_stops_custom_control_even_when_save_fails() {
        let mut controller = manual();
        assert!(reset_configuration(&mut controller, |_| Err("disk full".into())).is_err());
        assert!(matches!(
            controller.config().fans[0].mode,
            fan_core::ControlMode::Automatic
        ));
    }
    #[test]
    fn individual_automatic_stops_custom_control_when_storage_fails() {
        let mut controller = manual();
        assert!(configure(&mut controller, FanConfig::automatic(0), |_| Err(
            "disk full".into()
        ))
        .is_err());
        assert!(matches!(
            controller.config().fans[0].mode,
            fan_core::ControlMode::Automatic
        ));
    }
    #[test]
    fn stale_data_blocks_rpm_but_never_blocks_safety_handback() {
        let snapshot = empty_snapshot(0.);
        assert!(!action_allowed(
            &Command::SetRpm {
                rpm: 1800,
                allow_fan_off: false
            },
            true,
            false,
            &snapshot,
            30.
        ));
        assert!(action_allowed(
            &Command::SetAutomatic,
            true,
            false,
            &snapshot,
            30.
        ));
        assert!(!action_allowed(
            &Command::SetAutomatic,
            false,
            false,
            &snapshot,
            30.
        ));
        assert!(!action_allowed(
            &Command::SetRpm {
                rpm: 1800,
                allow_fan_off: false
            },
            true,
            true,
            &snapshot,
            0.
        ));
        assert!(action_allowed(
            &Command::SetAutomatic,
            true,
            true,
            &snapshot,
            0.
        ));
    }
    #[test]
    fn only_actual_rpm_and_compatible_privileged_receipts_confirm_targets() {
        let command = Command::SetRpm {
            rpm: 1800,
            allow_fan_off: false,
        };
        let mut response = fan_platform::HelperResponse {
            ok: true,
            message: "success".into(),
            is_root: true,
            protocol_version: Some(PROTOCOL_VERSION),
            actual_rpm: Some(1800),
        };
        assert!(response_confirms(&command, &response));
        response.actual_rpm = Some(1499);
        assert!(!response_confirms(&command, &response));
        response.actual_rpm = None;
        assert!(!response_confirms(&command, &response));
        response.actual_rpm = Some(1800);
        response.protocol_version = Some(4);
        assert!(!response_confirms(&command, &response));
    }
    #[test]
    fn failed_automatic_uses_full_reset_barrier_and_never_renews_the_lease() {
        let now = Instant::now();
        let mut handback = Handback::default();
        assert!(!handback.record_write(&Command::SetAutomatic, &response(false, None), now));
        assert!(handback.active);
        assert!(handback.due(now));
        assert!(!handback.heartbeat_allowed());
        assert!(!handback.accepts(&Command::SetRpm {
            rpm: 1800,
            allow_fan_off: false
        }));
        assert!(handback.accepts(&Command::SetAutomatic));
        assert!(!handback.acknowledge_reset(&response(false, None), now));
        assert!(!handback.due(now + Duration::from_secs(1)));
        assert!(handback.due(now + HANDBACK_RETRY));
        assert!(!handback.heartbeat_allowed());
        assert!(handback.acknowledge_reset(&response(true, None), now + HANDBACK_RETRY));
        assert!(!handback.active);
        assert!(handback.heartbeat_allowed());
    }
    #[test]
    fn mismatched_rpm_requires_handback_and_invalidates_the_rest_of_the_batch() {
        let now = Instant::now();
        let mut handback = Handback::default();
        let rpm = Command::SetRpm {
            rpm: 1800,
            allow_fan_off: false,
        };
        assert!(!handback.record_write(&rpm, &response(true, Some(1499)), now));
        assert!(handback.controlled);
        assert!(!handback.accepts(&rpm));
        let mut false_root = response(true, None);
        false_root.is_root = false;
        assert!(!handback.acknowledge_reset(&false_root, now));
        assert!(handback.active);
        assert!(handback.acknowledge_reset(&response(true, None), now + HANDBACK_RETRY));
        assert!(!handback.controlled);
    }
    #[test]
    fn recovery_marker_prevents_a_renamed_but_unconfirmed_custom_configuration_from_replaying() {
        let directory = temporary_directory("incomplete-save");
        let bytes = manual().config().to_json().unwrap();
        fs::write(directory.join("config.json"), &bytes).unwrap();
        fs::write(directory.join(RECOVERY_MARKER), b"save was not confirmed").unwrap();
        let (mut persistence, config) = Persistence::load(directory.clone(), false);
        assert!(matches!(
            config.fans[0].mode,
            fan_core::ControlMode::Automatic
        ));
        assert!(persistence.notice().contains("上次保存未完成"));
        assert!(persistence.pending);
        persistence.retry(Instant::now() + PERSISTENCE_RETRY, &config);
        assert!(!persistence.pending);
        assert!(!directory.join(RECOVERY_MARKER).exists());
        assert_eq!(
            fs::read(directory.join("config.pre-rust.json")).unwrap(),
            bytes.as_bytes()
        );
        let (_, reloaded) = Persistence::load(directory.clone(), false);
        assert!(matches!(
            reloaded.fans[0].mode,
            fan_core::ControlMode::Automatic
        ));
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn unreadable_original_blocks_overwrite_and_retry_never_applies_recovered_custom_intent() {
        let directory = temporary_directory("unreadable");
        let original = manual().config().to_json().unwrap();
        let source = directory.join("original.json");
        fs::write(&source, &original).unwrap();
        std::os::unix::fs::symlink(&source, directory.join("config.json")).unwrap();
        let (mut persistence, config) = Persistence::load(directory.clone(), false);
        assert!(config.fans.is_empty());
        assert!(persistence.read_blocked);
        assert!(persistence.save(&config).is_err());
        assert_eq!(fs::read(&source).unwrap(), original.as_bytes());
        assert!(!directory.join("config.pre-rust.json").exists());
        fs::remove_file(directory.join("config.json")).unwrap();
        fs::write(directory.join("config.json"), &original).unwrap();
        persistence.retry(Instant::now() + PERSISTENCE_RETRY, &config);
        assert!(!persistence.read_blocked);
        assert!(config.fans.is_empty());
        assert!(
            Config::from_json(&fs::read_to_string(directory.join("config.json")).unwrap())
                .unwrap()
                .config
                .fans
                .is_empty()
        );
        assert_eq!(
            fs::read(directory.join("config.pre-rust.json")).unwrap(),
            original.as_bytes()
        );
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn permission_denied_is_visible_and_distinct_from_missing_configuration() {
        use std::os::unix::fs::PermissionsExt;
        let directory = temporary_directory("permission");
        let path = directory.join("config.json");
        assert_eq!(read_original(&directory).unwrap(), None);
        fs::write(&path, b"unreadable original").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let (mut persistence, config) = Persistence::load(directory.clone(), false);
        let blocked = persistence.read_blocked;
        let save = if blocked {
            persistence.save(&config)
        } else {
            Ok(())
        };
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        // The production app is an ordinary console-user process. A root test
        // runner can still read mode-000 files and does not model this failure.
        if unsafe { libc::geteuid() } != 0 {
            assert!(blocked);
            assert!(save.is_err());
            assert!(persistence.notice().contains("无法安全读取"));
            assert_eq!(fs::read(&path).unwrap(), b"unreadable original");
        }
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn config_load_repairs_and_migration_warnings_have_a_persistent_channel() {
        let legacy = br#"[{"fanId":0,"mode":{"manual":{"rpm":-10}}}]"#;
        let (config, notice) = decode_config(Some(legacy));
        assert!(matches!(
            config.fans[0].mode,
            fan_core::ControlMode::Automatic
        ));
        assert!(notice.contains("旧版"));
        assert!(!notice.is_empty());
        let (mut persistence, _) = Persistence::load(PathBuf::new(), true);
        persistence.base_notice = notice.clone();
        persistence.save_error = Some("disk full".into());
        assert!(persistence.notice().contains("disk full"));
        assert!(persistence.notice().contains(&notice));
        persistence.save(&config).unwrap();
        assert!(!persistence.notice().contains("disk full"));
        assert!(persistence.notice().contains("已迁移"));
    }
    #[test]
    fn background_save_retry_never_applies_the_rejected_custom_candidate() {
        let directory = temporary_directory("rejected-custom");
        fs::write(
            directory.join("config.json"),
            manual().config().to_json().unwrap(),
        )
        .unwrap();
        let (mut persistence, config) = Persistence::load(directory.clone(), false);
        let mut controller = Controller::new(config);
        // A failing transaction marker prevents touching the existing JSON.
        fs::create_dir(directory.join(RECOVERY_MARKER)).unwrap();
        assert!(
            configure(&mut controller, FanConfig::balanced(0), |candidate| {
                persistence.save(candidate)
            })
            .is_err()
        );
        assert!(persistence.pending);
        assert!(matches!(
            controller.config().fans[0].mode,
            fan_core::ControlMode::Manual { rpm: 1800 }
        ));
        fs::remove_dir(directory.join(RECOVERY_MARKER)).unwrap();
        persistence.retry(Instant::now() + PERSISTENCE_RETRY, controller.config());
        assert!(!persistence.pending);
        let (_, disk) = Persistence::load(directory.clone(), false);
        assert!(matches!(
            disk.fans[0].mode,
            fan_core::ControlMode::Manual { rpm: 1800 }
        ));
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn policy_save_failure_keeps_the_accepted_target_and_never_saves_invalid_calibration() {
        let mut controller = manual();
        let policy = fan_core::ThermalPolicy {
            comfort_target_celsius: Some(35.0),
            calibration: None,
        };
        assert!(
            configure_policy(&mut controller, policy.clone(), |_| Err("disk full".into())).is_err()
        );
        assert_eq!(
            controller.config().thermal_policy.comfort_target_celsius,
            Some(38.0)
        );
        assert!(configure_policy(
            &mut controller,
            fan_core::ThermalPolicy {
                comfort_target_celsius: Some(46.0),
                calibration: None
            },
            |_| panic!("invalid policy must not be saved")
        )
        .is_err());
        configure_policy(&mut controller, policy, |_| Ok(())).unwrap();
        assert_eq!(
            controller.config().thermal_policy.comfort_target_celsius,
            Some(35.0)
        );
    }
    #[test]
    fn diagnostic_configuration_omits_the_device_scope_but_persistence_keeps_it() {
        let mut config = Config::default();
        config.thermal_policy.calibration = Some(
            fan_core::SurfaceCalibration::from_measurements(
                "private-device-scope",
                "body",
                40.0,
                30.0,
                50.0,
                40.0,
            )
            .unwrap(),
        );
        assert!(config.to_json().unwrap().contains("private-device-scope"));
        let report = diagnostic_config(&config);
        assert!(!report.to_string().contains("private-device-scope"));
        assert!(!report.to_string().contains("machine_id"));
        assert!(report["thermal_policy"]["calibration"]["sensor_key"].is_string());
    }
    #[test]
    fn a_previous_backup_does_not_allow_discarding_different_original_contents() {
        let directory = temporary_directory("backup-revision");
        fs::write(directory.join("config.pre-rust.json"), b"earlier original").unwrap();
        fs::write(directory.join("config.json"), b"later malformed original").unwrap();
        let (mut persistence, config) = Persistence::load(directory.clone(), false);
        persistence.save(&config).unwrap();
        assert_eq!(
            fs::read(directory.join("config.pre-rust.json")).unwrap(),
            b"earlier original"
        );
        let recovered = fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.pre-rust.")
            })
            .any(|entry| fs::read(entry.path()).unwrap() == b"later malformed original");
        assert!(recovered);
        fs::remove_dir_all(directory).unwrap();
    }
}
