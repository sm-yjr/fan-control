//! Native power notifications on a dedicated run loop; sleep acknowledgement follows hand-back.
use crate::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEvent {
    WillSleep,
    WillWake,
    DidWake,
}

pub struct PowerMonitor {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl PowerMonitor {
    /// The callback runs on the monitor thread. `WillSleep` must synchronously
    /// pause control and wait for privileged automatic hand-back before returning.
    /// Never wait for the GUI main thread here. The monitor then acknowledges IOKit.
    pub fn register<F>(callback: F) -> Result<Self>
    where
        F: Fn(PowerEvent) + Send + 'static,
    {
        native::register(Box::new(callback))
    }
}
impl Drop for PowerMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn deliver(callback: &dyn Fn(PowerEvent), event: PowerEvent, acknowledge: impl FnOnce()) {
    // A Rust panic must never unwind through the IOKit C callback boundary.
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event))).is_err() {
        eprintln!("[FanControl] power callback panicked");
    }
    acknowledge();
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::ffi::c_void;
    use std::sync::mpsc;
    type NotificationPort = *mut c_void;
    type CfObject = *const c_void;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IORegisterForSystemPower(
            context: *mut c_void,
            notification: *mut NotificationPort,
            callback: extern "C" fn(*mut c_void, u32, u32, *mut c_void),
            notifier: *mut u32,
        ) -> u32;
        fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
        fn IONotificationPortDestroy(port: NotificationPort);
        fn IONotificationPortGetRunLoopSource(port: NotificationPort) -> CfObject;
        fn IOServiceClose(connection: u32) -> i32;
        fn IOAllowPowerChange(connection: u32, notification: isize) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopGetCurrent() -> CfObject;
        fn CFRunLoopAddSource(run_loop: CfObject, source: CfObject, mode: CfObject);
        fn CFRunLoopRemoveSource(run_loop: CfObject, source: CfObject, mode: CfObject);
        fn CFRunLoopRunInMode(mode: CfObject, seconds: f64, return_after_source: u8) -> i32;
        static kCFRunLoopDefaultMode: CfObject;
    }
    struct Context {
        callback: Box<dyn Fn(PowerEvent) + Send>,
        connection: u32,
    }
    const CAN_SLEEP: u32 = 0xe0000270;
    const WILL_SLEEP: u32 = 0xe0000280;
    const WILL_WAKE: u32 = 0xe0000320;
    const DID_WAKE: u32 = 0xe0000300;
    extern "C" fn power_callback(
        context: *mut c_void,
        _: u32,
        message: u32,
        argument: *mut c_void,
    ) {
        if context.is_null() {
            return;
        }
        let context = unsafe { &*(context as *const Context) };
        match message {
            CAN_SLEEP => unsafe {
                IOAllowPowerChange(context.connection, argument as isize);
            },
            WILL_SLEEP => deliver(&*context.callback, PowerEvent::WillSleep, || unsafe {
                IOAllowPowerChange(context.connection, argument as isize);
            }),
            WILL_WAKE => deliver(&*context.callback, PowerEvent::WillWake, || {}),
            DID_WAKE => deliver(&*context.callback, PowerEvent::DidWake, || {}),
            _ => {}
        }
    }
    pub fn register(callback: Box<dyn Fn(PowerEvent) + Send>) -> Result<PowerMonitor> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("fan-power".into())
            .spawn(move || {
                let mut context = Box::new(Context {
                    callback,
                    connection: 0,
                });
                let mut notification: NotificationPort = std::ptr::null_mut();
                let mut notifier = 0;
                unsafe {
                    let connection = IORegisterForSystemPower(
                        &mut *context as *mut _ as _,
                        &mut notification,
                        power_callback,
                        &mut notifier,
                    );
                    if connection == 0 || notification.is_null() {
                        if notifier != 0 {
                            IODeregisterForSystemPower(&mut notifier);
                        }
                        if !notification.is_null() {
                            IONotificationPortDestroy(notification);
                        }
                        if connection != 0 {
                            IOServiceClose(connection);
                        }
                        let _ = sender.send(Err(Error("system power registration failed".into())));
                        return;
                    }
                    context.connection = connection;
                    let source = IONotificationPortGetRunLoopSource(notification);
                    if source.is_null() {
                        IODeregisterForSystemPower(&mut notifier);
                        IONotificationPortDestroy(notification);
                        IOServiceClose(connection);
                        let _ = sender.send(Err(Error(
                            "system power run loop source unavailable".into(),
                        )));
                        return;
                    }
                    let run_loop = CFRunLoopGetCurrent();
                    CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
                    let _ = sender.send(Ok(()));
                    while !thread_stop.load(Ordering::SeqCst) {
                        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, 1);
                    }
                    CFRunLoopRemoveSource(run_loop, source, kCFRunLoopDefaultMode);
                    IODeregisterForSystemPower(&mut notifier);
                    IONotificationPortDestroy(notification);
                    IOServiceClose(connection);
                }
            })?;
        match receiver.recv() {
            Ok(Ok(())) => Ok(PowerMonitor {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(Error(error.to_string()))
            }
        }
    }
}
#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub fn register(_: Box<dyn Fn(PowerEvent) + Send>) -> Result<PowerMonitor> {
        Err(Error("power monitoring requires macOS".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn sleep_acknowledgement_follows_synchronous_handback() {
        let order = RefCell::new(Vec::new());
        deliver(
            &|event| {
                assert_eq!(event, PowerEvent::WillSleep);
                order.borrow_mut().push("reset");
            },
            PowerEvent::WillSleep,
            || order.borrow_mut().push("ack"),
        );
        assert_eq!(*order.borrow(), vec!["reset", "ack"]);
    }
}
