use std::sync::atomic::{AtomicBool, Ordering};
pub static TERMINATE_REQUEST: AtomicBool = AtomicBool::new(false);
extern "C" fn terminate_signal(_: libc::c_int) {
    TERMINATE_REQUEST.store(true, Ordering::SeqCst);
}
mod app_version;
mod chart;
mod details;
mod fans;
mod form;
mod gauge;
mod i18n;
mod launch;
mod overview;
mod policy;
mod popover;
mod preferences;
mod presenter;
mod settings;
mod storage;
mod telemetry;
mod trend;
mod ui;
mod updater;
mod worker;

use launch::LaunchMode;
fn main() {
    let mode = match LaunchMode::parse(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    match mode {
        LaunchMode::Helper=>if let Err(error)=fan_platform::run_helper(){eprintln!("helper: {error}");std::process::exit(1);},
        LaunchMode::ReadSensors=>match fan_platform::Smc::open(){
            Ok(mut smc)=>{let snapshot=match smc.discover_snapshot(){Ok(snapshot)=>snapshot,Err(error)=>{eprintln!("SMC snapshot: {error}");std::process::exit(1);}};println!("{}",serde_json::to_string_pretty(&snapshot).expect("serialize snapshot"));},
            Err(error)=>{eprintln!("SMC: {error}");std::process::exit(1);}
        },
        LaunchMode::UpdaterCheck=>{
            let mtm=objc2::MainThreadMarker::new().expect("main thread");
            let app=objc2_app_kit::NSApplication::sharedApplication(mtm);
            app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Prohibited);
            if updater::Updater::probe(mtm).is_some(){println!("Sparkle updater runtime is available");}else{eprintln!("Sparkle updater runtime is unavailable");std::process::exit(1);}
        },
        LaunchMode::PackageInfo=>println!("{}",serde_json::to_string(&serde_json::json!({"app":app_version::current(),"helper_protocol":fan_platform::PROTOCOL_VERSION,"architecture":std::env::consts::ARCH})).expect("serialize package metadata")),
        LaunchMode::Gui{demo}=>{
            unsafe {libc::signal(libc::SIGTERM,terminate_signal as *const () as libc::sighandler_t);libc::signal(libc::SIGINT,terminate_signal as *const () as libc::sighandler_t);}
            let lock_directory=std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/FanControl");
            let _lock=match storage::InstanceLock::acquire(&lock_directory){Ok(lock)=>lock,Err(error)=>{eprintln!("Fan Control is already running, or the session lock cannot be acquired: {error}");return;}};
            ui::run(demo,false);
        },
        LaunchMode::UiSmoke=>ui::run(true,true),
        LaunchMode::Help=>println!("FanControl [--demo | --read-sensors | --check-updater-runtime | --package-info | --ui-smoke | --helper]\nDefault: native menu-bar application. --read-sensors only reads SMC; --demo never connects to hardware."),
    }
}
