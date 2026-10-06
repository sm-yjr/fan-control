//! macOS hardware and privileged-service boundary. No UI or dynamic updater dependency.
pub mod battery;
pub mod device;
pub mod helper;
pub mod install;
pub mod power;
pub mod smc;
pub mod workload;

pub use battery::{read_battery, BatteryReading};
pub use device::{read_device_identity, DeviceIdentity};
pub use helper::{
    run_helper, HelperClient, HelperCommand, HelperRequest, HelperResponse, PROTOCOL_VERSION,
};
pub use install::{
    install_bundled_helper, install_script, launch_daemon_plist, uninstall_helper, uninstall_script,
};
pub use power::{PowerEvent, PowerMonitor};
pub use smc::{FanMode, RawFan, RawSensor, RawSnapshot, SensorGroup, SharedSmc, Smc};
pub use workload::CpuLoadMonitor;

#[derive(Debug)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;
