//! Operational logs contain no configuration, home path, username or credentials.
use std::{
    ffi::{c_void, CString},
    sync::OnceLock,
};
#[link(name = "System")]
extern "C" {
    static __dso_handle: u8;
    fn os_log_create(subsystem: *const libc::c_char, category: *const libc::c_char) -> *mut c_void;
    fn _os_log_internal(
        dso: *const c_void,
        log: *mut c_void,
        kind: u8,
        format: *const libc::c_char,
        ...
    );
}
pub fn event(message: &str) {
    static LOG: OnceLock<usize> = OnceLock::new();
    let handle = *LOG.get_or_init(|| unsafe {
        os_log_create(c"com.local.fan-control".as_ptr(), c"runtime".as_ptr()) as usize
    });
    if let Ok(message) = CString::new(message) {
        unsafe {
            _os_log_internal(
                std::ptr::addr_of!(__dso_handle).cast(),
                handle as *mut c_void,
                0,
                c"%{public}s".as_ptr(),
                message.as_ptr(),
            );
        }
    }
}
