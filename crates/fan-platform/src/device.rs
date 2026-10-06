//! Local calibration scope from IOPlatformExpertDevice, with no raw identifier output.
use crate::Result;
use sha2::{Digest, Sha256};

/// Keep `scope_id` in local configuration; exclude it from diagnostic reports.
/// Deliberately has no Debug/Serialize implementation that could expose it by accident.
#[derive(Clone)]
pub struct DeviceIdentity {
    pub scope_id: String,
    pub model: String,
}

/// Missing/invalid device properties return None, so calibration stays disabled.
pub fn read_device_identity() -> Result<Option<DeviceIdentity>> {
    native::read()
}

fn identity_from_properties(uuid: Option<&str>, model: Option<&str>) -> Option<DeviceIdentity> {
    let uuid = uuid?;
    let model = model?;
    if uuid.len() != 36
        || !uuid.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
        || uuid
            .bytes()
            .filter(|byte| *byte != b'-')
            .all(|byte| byte == b'0')
        || uuid
            .bytes()
            .filter(|byte| *byte != b'-')
            .all(|byte| byte.eq_ignore_ascii_case(&b'f'))
        || !(3..=64).contains(&model.len())
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || [b',', b'.', b'-'].contains(&byte))
        || !model.bytes().any(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(b"fan-control:comfort-calibration:v1\0");
    hash.update(uuid.to_ascii_lowercase().as_bytes());
    let digest = hash.finalize();
    let mut scope_id = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut scope_id, "{byte:02x}").ok()?;
    }
    Some(DeviceIdentity {
        scope_id,
        model: model.to_owned(),
    })
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use crate::Error;
    use std::ffi::{c_char, c_void, CStr};
    type CfRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(port: u32, matching: *mut c_void) -> u32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: CfRef,
            allocator: CfRef,
            options: u32,
        ) -> CfRef;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: CfRef);
        fn CFGetTypeID(value: CfRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFDataGetTypeID() -> usize;
        fn CFStringCreateWithCString(allocator: CfRef, text: *const c_char, encoding: u32)
            -> CfRef;
        fn CFStringGetCString(
            value: CfRef,
            buffer: *mut c_char,
            length: isize,
            encoding: u32,
        ) -> u8;
        fn CFDataGetLength(value: CfRef) -> isize;
        fn CFDataGetBytePtr(value: CfRef) -> *const u8;
    }
    struct OwnedCf(CfRef);
    impl OwnedCf {
        fn new(value: CfRef) -> Option<Self> {
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
    struct OwnedService(u32);
    impl Drop for OwnedService {
        fn drop(&mut self) {
            unsafe {
                IOObjectRelease(self.0);
            }
        }
    }

    fn property(service: &OwnedService, key: &CStr) -> Option<OwnedCf> {
        let key = OwnedCf::new(unsafe {
            CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), UTF8)
        })?;
        OwnedCf::new(unsafe {
            IORegistryEntryCreateCFProperty(service.0, key.0, std::ptr::null(), 0)
        })
    }
    fn string(value: &OwnedCf) -> Option<String> {
        if unsafe { CFGetTypeID(value.0) } != unsafe { CFStringGetTypeID() } {
            return None;
        }
        let mut bytes = [0i8; 129];
        if unsafe { CFStringGetCString(value.0, bytes.as_mut_ptr(), bytes.len() as isize, UTF8) }
            == 0
        {
            return None;
        }
        unsafe { CStr::from_ptr(bytes.as_ptr()) }
            .to_str()
            .ok()
            .map(str::to_owned)
    }
    fn model_string(value: &OwnedCf) -> Option<String> {
        if unsafe { CFGetTypeID(value.0) } == unsafe { CFStringGetTypeID() } {
            return string(value);
        }
        if unsafe { CFGetTypeID(value.0) } != unsafe { CFDataGetTypeID() } {
            return None;
        }
        let length = unsafe { CFDataGetLength(value.0) };
        if !(1..=128).contains(&length) {
            return None;
        }
        let bytes = unsafe { CFDataGetBytePtr(value.0) };
        if bytes.is_null() {
            return None;
        }
        std::str::from_utf8(unsafe { std::slice::from_raw_parts(bytes, length as usize) })
            .ok()
            .map(|value| value.trim_end_matches('\0').to_owned())
    }
    pub fn read() -> Result<Option<DeviceIdentity>> {
        let matching = unsafe { IOServiceMatching(c"IOPlatformExpertDevice".as_ptr()) };
        if matching.is_null() {
            return Err(Error("platform device lookup unavailable".into()));
        }
        // IOServiceGetMatchingService consumes its matching dictionary.
        let service = unsafe { IOServiceGetMatchingService(0, matching) };
        if service == 0 {
            return Ok(None);
        }
        let service = OwnedService(service);
        let uuid = property(&service, c"IOPlatformUUID")
            .as_ref()
            .and_then(string);
        let model = property(&service, c"model").as_ref().and_then(model_string);
        Ok(identity_from_properties(uuid.as_deref(), model.as_deref()))
    }
}
#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    pub fn read() -> Result<Option<DeviceIdentity>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const UUID: &str = "12345678-1234-1234-1234-123456789abc";
    #[test]
    fn scope_is_device_specific_case_normalized_and_domain_separated() {
        let first = identity_from_properties(Some(UUID), Some("Mac15,3")).unwrap();
        let upper =
            identity_from_properties(Some(&UUID.to_ascii_uppercase()), Some("Mac15,3")).unwrap();
        let other = identity_from_properties(
            Some("22345678-1234-1234-1234-123456789abc"),
            Some("Mac15,3"),
        )
        .unwrap();
        assert_eq!(first.scope_id, upper.scope_id);
        assert_ne!(first.scope_id, other.scope_id);
        assert_eq!(first.scope_id.len(), 64);
        assert!(!first.scope_id.contains(UUID));
        assert_ne!(
            first.scope_id,
            format!("{:x}", Sha256::digest(UUID.as_bytes()))
        );
    }
    #[test]
    fn missing_or_placeholder_properties_disable_calibration() {
        assert!(identity_from_properties(None, Some("Mac15,3")).is_none());
        assert!(identity_from_properties(Some(UUID), None).is_none());
        for uuid in [
            "",
            "bogus",
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ] {
            assert!(identity_from_properties(Some(uuid), Some("Mac15,3")).is_none());
        }
        for model in ["", "Unknown", "Mac15,3\0unexpected", "Mac 15,3"] {
            assert!(identity_from_properties(Some(UUID), Some(model)).is_none());
        }
    }
    #[test]
    #[ignore = "opt-in, read-only physical device identity probe; no ID output"]
    fn native_identity_readonly_probe() {
        let first = read_device_identity()
            .unwrap()
            .expect("platform UUID and model available");
        let second = read_device_identity().unwrap().unwrap();
        assert_eq!(first.scope_id, second.scope_id);
        assert_eq!(first.scope_id.len(), 64);
        println!("local calibration identity available and stable; identifiers excluded");
    }
}
