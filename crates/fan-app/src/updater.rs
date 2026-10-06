//! Sparkle is looked up at runtime in the GUI. The standalone helper never links it.
use objc2::{
    define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject},
    MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{NSBundle, NSObject, NSObjectProtocol, NSString};

define_class!(
    #[unsafe(super=NSObject)]
    #[thread_kind=MainThreadOnly]
    struct UserDriverDelegate;
    unsafe impl NSObjectProtocol for UserDriverDelegate {}
    impl UserDriverDelegate {
        #[unsafe(method(supportsGentleScheduledUpdateReminders))]
        fn gentle_reminders(&self)->bool {true}
        #[unsafe(method(standardUserDriverWillHandleShowingUpdate:forUpdate:state:))]
        fn showing_update(&self,_handled:bool,_update:&AnyObject,_state:&AnyObject) {
            NSApplication::sharedApplication(self.mtm()).setActivationPolicy(NSApplicationActivationPolicy::Regular);
        }
        #[unsafe(method(standardUserDriverWillFinishUpdateSession))]
        fn finished_update(&self) {
            NSApplication::sharedApplication(self.mtm()).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        }
    }
);

#[derive(Clone)]
pub struct Updater {
    controller: Retained<AnyObject>,
    updater: Retained<AnyObject>,
    _delegate: Retained<UserDriverDelegate>,
}
impl Updater {
    pub fn load(mtm: MainThreadMarker) -> Option<Self> {
        Self::initialize(mtm, true)
    }
    /// Verify the bundled runtime without scheduling checks or opening update UI.
    pub fn probe(mtm: MainThreadMarker) -> Option<Self> {
        Self::initialize(mtm, false)
    }
    fn initialize(mtm: MainThreadMarker, starting: bool) -> Option<Self> {
        let directory = NSBundle::mainBundle().privateFrameworksPath()?;
        let path = NSString::from_str(&format!("{directory}/Sparkle.framework"));
        let bundle = NSBundle::bundleWithPath(&path)?;
        if !unsafe { bundle.load() } {
            return None;
        }
        let name = c"SPUStandardUpdaterController";
        let class = AnyClass::get(name)?;
        // SAFETY: The checked class belongs to the pinned Sparkle 2.9.2 framework.
        let allocated: Allocated<AnyObject> = unsafe { msg_send![class, alloc] };
        let delegate: Retained<UserDriverDelegate> =
            unsafe { msg_send![UserDriverDelegate::alloc(mtm), init] };
        let controller: Option<Retained<AnyObject>> = unsafe {
            msg_send![allocated, initWithStartingUpdater: starting, updaterDelegate: std::ptr::null::<AnyObject>(), userDriverDelegate: &*delegate]
        };
        let controller = controller?;
        let updater: Option<Retained<AnyObject>> = unsafe { msg_send![&controller, updater] };
        Some(Self {
            controller,
            updater: updater?,
            _delegate: delegate,
        })
    }
    pub fn can_check(&self) -> bool {
        unsafe { msg_send![&self.updater, canCheckForUpdates] }
    }
    pub fn check(&self) {
        if self.can_check() {
            let _: () = unsafe {
                msg_send![&self.controller, checkForUpdates: std::ptr::null::<AnyObject>()]
            };
        }
    }
}
