//! The packaged version is authoritative; Cargo is a fallback for a bare binary.
use objc2_foundation::{NSBundle, NSString};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct VersionInfo {
    pub version: String,
    pub build: String,
}
pub fn current() -> VersionInfo {
    let bundle = NSBundle::mainBundle();
    let value = |key: &str| {
        bundle
            .objectForInfoDictionaryKey(&NSString::from_str(key))
            .and_then(|object| {
                object
                    .downcast_ref::<NSString>()
                    .map(|value| value.to_string())
            })
    };
    VersionInfo {
        version: value("CFBundleShortVersionString")
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into()),
        build: value("CFBundleVersion").unwrap_or_else(|| "development".into()),
    }
}
