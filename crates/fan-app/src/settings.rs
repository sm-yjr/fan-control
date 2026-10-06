//! User-requested login startup through the system's SMAppService mainAppService.
//!
//! Loading this module never registers anything. Call `set_enabled` only from an
//! explicit settings action; `enabled` reads the system state without changing it.
//! ServiceManagement is loaded from the OS at runtime, keeping the standalone
//! helper free of app-bundled dynamic dependencies. No root access or LaunchAgent
//! installation is needed.

use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{NSBundle, NSError, NSString};
use std::path::Path;

const BUNDLE_IDENTIFIER: &str = "com.local.fan-control";
const APPROVAL_MESSAGE: &str =
    "登录启动已注册，但需要在系统设置 → 通用 → 登录项中允许 Fan Control。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

impl LoginStatus {
    fn from_raw(value: isize) -> Result<Self, String> {
        // SMAppServiceStatus is NSInteger in the macOS SDK.
        match value {
            0 => Ok(Self::NotRegistered),
            1 => Ok(Self::Enabled),
            2 => Ok(Self::RequiresApproval),
            3 => Ok(Self::NotFound),
            _ => Err(format!("系统返回未知登录启动状态：{value}")),
        }
    }

    fn enabled(self) -> Result<bool, String> {
        match self {
            Self::NotRegistered => Ok(false),
            Self::Enabled => Ok(true),
            Self::RequiresApproval => Err(APPROVAL_MESSAGE.into()),
            Self::NotFound => Err("系统找不到当前应用的登录启动服务。".into()),
        }
    }
}

trait LoginService {
    fn status(&self) -> Result<LoginStatus, String>;
    fn register(&self) -> Result<(), String>;
    fn unregister(&self) -> Result<(), String>;
}

struct SystemLoginService(Retained<AnyObject>);

impl SystemLoginService {
    fn load() -> Result<Self, String> {
        let path = NSString::from_str("/System/Library/Frameworks/ServiceManagement.framework");
        let bundle =
            NSBundle::bundleWithPath(&path).ok_or("无法找到系统 ServiceManagement framework。")?;
        // SAFETY: Only the OS framework at this fixed path is loaded; NSBundle
        // owns its image for the lifetime of the process.
        unsafe { bundle.loadAndReturnError() }
            .map_err(|error| format!("无法加载登录启动服务：{}", error.localizedDescription()))?;
        let class = AnyClass::get(c"SMAppService")
            .ok_or("当前系统不提供 SMAppService；需要 macOS 13 或更新系统。")?;
        // SAFETY: The checked class and selector are declared by the macOS
        // SMAppService API. msg_send retains its autoreleased object.
        let service: Option<Retained<AnyObject>> = unsafe { msg_send![class, mainAppService] };
        service
            .map(Self)
            .ok_or("无法获取当前应用的登录启动服务。".into())
    }
}

impl LoginService for SystemLoginService {
    fn status(&self) -> Result<LoginStatus, String> {
        // SAFETY: SMAppService.status returns the NSInteger-backed enum.
        let status: isize = unsafe { msg_send![&self.0, status] };
        LoginStatus::from_raw(status)
    }

    fn register(&self) -> Result<(), String> {
        // SAFETY: The NSError out-parameter and BOOL return follow Cocoa's
        // standard error convention; objc2 retains the NSError on failure.
        let result: Result<(), Retained<NSError>> =
            unsafe { msg_send![&self.0, registerAndReturnError: _] };
        result.map_err(|error| format!("启用登录启动失败：{}", error.localizedDescription()))
    }

    fn unregister(&self) -> Result<(), String> {
        // SAFETY: This is the matching SMAppService NSError/BOOL API.
        let result: Result<(), Retained<NSError>> =
            unsafe { msg_send![&self.0, unregisterAndReturnError: _] };
        result.map_err(|error| format!("关闭登录启动失败：{}", error.localizedDescription()))
    }
}

/// Reports the actual system state. A registered service awaiting user approval
/// returns an actionable error and must not be displayed as enabled.
pub fn enabled() -> Result<bool, String> {
    SystemLoginService::load()?.status()?.enabled()
}

/// Changes login startup for the currently running, packaged Fan Control App.
/// Call only after an explicit user toggle. A successful API request is followed
/// by a status read; permission pending or a mismatched final state is an error.
pub fn set_enabled(wanted: bool, app_bundle: &Path) -> Result<(), String> {
    // SAFETY: geteuid has no arguments and is read-only.
    if unsafe { libc::geteuid() } == 0 {
        return Err("登录启动属于当前用户设置，不能以 root 身份修改。".into());
    }
    let bundle = NSBundle::mainBundle();
    let identifier = bundle
        .bundleIdentifier()
        .map(|identifier| identifier.to_string());
    if identifier.as_deref() != Some(BUNDLE_IDENTIFIER) {
        return Err("请从完整的 FanControl.app 中修改登录启动。".into());
    }
    let running = bundle.bundlePath().to_string();
    validate_bundle_paths(app_bundle, Path::new(&running))?;
    set_with_service(&SystemLoginService::load()?, wanted)
}

fn validate_bundle_paths(requested: &Path, running: &Path) -> Result<(), String> {
    let requested = requested
        .canonicalize()
        .map_err(|error| format!("无法读取应用位置：{error}"))?;
    let running = running
        .canonicalize()
        .map_err(|error| format!("无法读取当前应用位置：{error}"))?;
    if requested != running || requested.extension().is_none_or(|value| value != "app") {
        return Err("登录启动只能为当前运行的 FanControl.app 设置。".into());
    }
    if !requested.join("Contents/Info.plist").is_file()
        || !requested.join("Contents/MacOS/FanControl").is_file()
    {
        return Err("应用包不完整，请重新安装 FanControl.app 后再设置登录启动。".into());
    }
    Ok(())
}

fn set_with_service(service: &impl LoginService, wanted: bool) -> Result<(), String> {
    let before = service.status()?;
    match (wanted, before) {
        (true, LoginStatus::Enabled) | (false, LoginStatus::NotRegistered) => return Ok(()),
        (true, LoginStatus::RequiresApproval) => return Err(APPROVAL_MESSAGE.into()),
        (_, LoginStatus::NotFound) => return Err("系统找不到当前应用的登录启动服务。".into()),
        _ => {}
    }
    if wanted {
        service.register()?;
    } else {
        service.unregister()?;
    }
    let after = service.status()?;
    match (wanted, after) {
        (true, LoginStatus::Enabled) | (false, LoginStatus::NotRegistered) => Ok(()),
        (_, LoginStatus::RequiresApproval) => Err(APPROVAL_MESSAGE.into()),
        (_, LoginStatus::NotFound) => Err("系统找不到当前应用的登录启动服务。".into()),
        _ => Err("登录启动状态尚未确认，请重新查询后再试。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct MockService {
        status: Cell<LoginStatus>,
        register_calls: Cell<u32>,
        unregister_calls: Cell<u32>,
        register_result: Result<LoginStatus, String>,
        unregister_result: Result<LoginStatus, String>,
    }

    impl MockService {
        fn new(status: LoginStatus) -> Self {
            Self {
                status: Cell::new(status),
                register_calls: Cell::new(0),
                unregister_calls: Cell::new(0),
                register_result: Ok(LoginStatus::Enabled),
                unregister_result: Ok(LoginStatus::NotRegistered),
            }
        }
    }

    impl LoginService for MockService {
        fn status(&self) -> Result<LoginStatus, String> {
            Ok(self.status.get())
        }
        fn register(&self) -> Result<(), String> {
            self.register_calls.set(self.register_calls.get() + 1);
            self.status.set(self.register_result.clone()?);
            Ok(())
        }
        fn unregister(&self) -> Result<(), String> {
            self.unregister_calls.set(self.unregister_calls.get() + 1);
            self.status.set(self.unregister_result.clone()?);
            Ok(())
        }
    }

    #[test]
    fn query_distinguishes_enabled_from_pending_and_unknown() {
        assert_eq!(LoginStatus::from_raw(0).unwrap().enabled(), Ok(false));
        assert_eq!(LoginStatus::from_raw(1).unwrap().enabled(), Ok(true));
        assert!(LoginStatus::from_raw(2).unwrap().enabled().is_err());
        assert!(LoginStatus::from_raw(3).unwrap().enabled().is_err());
        assert!(LoginStatus::from_raw(99).is_err());
    }

    #[test]
    fn repeated_toggles_are_idempotent() {
        let service = MockService::new(LoginStatus::NotRegistered);
        set_with_service(&service, false).unwrap();
        assert_eq!(service.unregister_calls.get(), 0);
        set_with_service(&service, true).unwrap();
        set_with_service(&service, true).unwrap();
        assert_eq!(service.register_calls.get(), 1);
        set_with_service(&service, false).unwrap();
        assert_eq!(service.unregister_calls.get(), 1);
    }

    #[test]
    fn pending_approval_is_not_claimed_as_enabled() {
        let mut service = MockService::new(LoginStatus::NotRegistered);
        service.register_result = Ok(LoginStatus::RequiresApproval);
        assert!(set_with_service(&service, true).is_err());
        assert_eq!(service.register_calls.get(), 1);
        assert!(set_with_service(&service, true).is_err());
        assert_eq!(service.register_calls.get(), 1);
        set_with_service(&service, false).unwrap();
    }

    #[test]
    fn api_errors_and_unconfirmed_status_propagate() {
        let mut service = MockService::new(LoginStatus::NotRegistered);
        service.register_result = Err("invalid signature".into());
        assert_eq!(
            set_with_service(&service, true),
            Err("invalid signature".into())
        );
        service.register_result = Ok(LoginStatus::NotRegistered);
        assert!(set_with_service(&service, true).is_err());
        let mut service = MockService::new(LoginStatus::Enabled);
        service.unregister_result = Err("denied".into());
        assert_eq!(set_with_service(&service, false), Err("denied".into()));
    }

    #[test]
    fn missing_service_does_not_attempt_changes() {
        let service = MockService::new(LoginStatus::NotFound);
        assert!(set_with_service(&service, true).is_err());
        assert!(set_with_service(&service, false).is_err());
        assert_eq!(service.register_calls.get(), 0);
        assert_eq!(service.unregister_calls.get(), 0);
    }

    #[test]
    fn bundle_must_match_running_app_and_be_complete() {
        let root = std::env::temp_dir().join(format!("fan-settings-{}", std::process::id()));
        let app = root.join("FanControl.app");
        let other = root.join("Other.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        assert!(validate_bundle_paths(&app, &app).is_err());
        std::fs::write(app.join("Contents/Info.plist"), b"fixture").unwrap();
        std::fs::write(app.join("Contents/MacOS/FanControl"), b"fixture").unwrap();
        assert!(validate_bundle_paths(&app, &app).is_ok());
        assert!(validate_bundle_paths(&other, &app).is_err());
        assert!(validate_bundle_paths(&root, &root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
