//! User-owned configuration and process exclusion. No temporary world-writable files.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn config_directory() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/fan-control")
}

pub fn atomic_save(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write(path, contents, true)
}

/// Exported files remain private without changing the user's chosen folder.
pub fn atomic_export(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write(path, contents, false)
}

fn atomic_write(path: &Path, contents: &[u8], private_directory: bool) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    if private_directory {
        fs::create_dir_all(directory)?;
    }
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "configuration directory must be a real directory",
        ));
    }
    if private_directory {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let staged = directory.join(format!(
        ".config.{}.{}.{}.tmp",
        std::process::id(),
        timestamp,
        sequence
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&staged)?;
    let result = (|| {
        clear_inherited_acl(&file)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&staged, path)?;
        File::open(directory)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

#[cfg(target_os = "macos")]
fn clear_inherited_acl(file: &File) -> io::Result<()> {
    use std::{ffi::c_void, os::fd::AsRawFd};
    extern "C" {
        fn acl_init(count: libc::c_int) -> *mut c_void;
        fn acl_set_fd_np(fd: libc::c_int, acl: *mut c_void, kind: libc::c_int) -> libc::c_int;
        fn acl_free(acl: *mut c_void) -> libc::c_int;
    }
    let acl = unsafe { acl_init(0) };
    if acl.is_null() {
        return Err(io::Error::last_os_error());
    }
    let status = unsafe { acl_set_fd_np(file.as_raw_fd(), acl, 0x100) }; // ACL_TYPE_EXTENDED.
    let error = io::Error::last_os_error();
    unsafe {
        acl_free(acl);
    }
    if status != 0 {
        return Err(error);
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn clear_inherited_acl(_: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private application storage requires macOS ACL handling",
    ))
}

pub struct InstanceLock(File);
impl InstanceLock {
    pub fn acquire(directory: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let metadata = fs::symlink_metadata(directory)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid lock directory",
            ));
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join("instance.lock"))?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "session lock must be a regular file",
            ));
        }
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}
impl Drop for InstanceLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fan-app-{name}-{}", std::process::id()))
    }
    #[test]
    fn export_does_not_change_the_selected_directory_permissions() {
        let directory = temp("export");
        fs::create_dir_all(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        let file = directory.join("diagnostics.json");
        atomic_export(&file, b"private report").unwrap();
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(&file).unwrap(), b"private report");
        fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn private_files_do_not_inherit_the_selected_directory_acl() {
        let directory = temp("export-acl");
        fs::create_dir_all(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone allow read,file_inherit,directory_inherit"])
            .arg(&directory)
            .status()
            .unwrap()
            .success());
        let acl_before = std::process::Command::new("/bin/ls")
            .args(["-lde"])
            .arg(&directory)
            .output()
            .unwrap();
        let plain = directory.join("inherited.txt");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&plain)
            .unwrap();
        let inherited = std::process::Command::new("/bin/ls")
            .arg("-le")
            .arg(&plain)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&inherited.stdout).contains("inherited allow read"));
        let exported = directory.join("diagnostics.json");
        atomic_export(&exported, b"synthetic diagnostics").unwrap();
        let output = std::process::Command::new("/bin/ls")
            .arg("-le")
            .arg(&exported)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
        assert_eq!(
            fs::metadata(&exported).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let acl_after = std::process::Command::new("/bin/ls")
            .args(["-lde"])
            .arg(&directory)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&acl_before.stdout)
                .lines()
                .skip(1)
                .collect::<Vec<_>>(),
            String::from_utf8_lossy(&acl_after.stdout)
                .lines()
                .skip(1)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let saved = directory.join("config.json");
        atomic_save(&saved, b"synthetic config").unwrap();
        let output = std::process::Command::new("/bin/ls")
            .arg("-le")
            .arg(&saved)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn lock_is_exclusive_and_releases() {
        let directory = temp("lock");
        let first = InstanceLock::acquire(&directory).unwrap();
        assert!(InstanceLock::acquire(&directory).is_err());
        drop(first);
        assert!(InstanceLock::acquire(&directory).is_ok());
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn lock_refuses_symlinks() {
        let directory = temp("symlink");
        fs::create_dir_all(&directory).unwrap();
        std::os::unix::fs::symlink("/dev/null", directory.join("instance.lock")).unwrap();
        assert!(InstanceLock::acquire(&directory).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn replacement_is_complete_and_private() {
        let directory = temp("save");
        let path = directory.join("config.json");
        atomic_save(&path, b"old").unwrap();
        atomic_save(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn leftovers_from_a_previous_process_do_not_block_atomic_save() {
        let directory = temp("leftover");
        fs::create_dir_all(&directory).unwrap();
        let leftover = directory.join(format!(".config.{}.tmp", std::process::id()));
        fs::write(&leftover, b"incomplete previous save").unwrap();
        atomic_save(&directory.join("config.json"), b"complete").unwrap();
        assert_eq!(
            fs::read(directory.join("config.json")).unwrap(),
            b"complete"
        );
        assert_eq!(fs::read(&leftover).unwrap(), b"incomplete previous save");
        fs::remove_dir_all(directory).unwrap();
    }
}
