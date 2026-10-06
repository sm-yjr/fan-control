//! Bounded Unix IPC, authenticated peers, and an owner-bound manual-control lease.
use crate::smc::{FanMode, Smc};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: u32 = 8;
pub const LABEL: &str = "com.local.fan-control.helper";
pub const RUNTIME_DIRECTORY: &str = "/Library/PrivilegedHelperTools/com.local.fan-control.runtime";
pub const SOCKET_PATH: &str =
    "/Library/PrivilegedHelperTools/com.local.fan-control.runtime/helper.sock";
pub const LEGACY_SOCKET_PATH: &str = "/var/run/com.local.fan-control.helper.sock";
pub const TOOL_PATH: &str = "/Library/PrivilegedHelperTools/com.local.fan-control.helper";
pub const DAEMON_PATH: &str = "/Library/LaunchDaemons/com.local.fan-control.helper.plist";
pub const LOG_DIRECTORY: &str = "/Library/Logs/FanControl";
pub const LOG_PATH: &str = "/Library/Logs/FanControl/helper.log";
const MAX_FRAME: usize = 4096;
const LEASE_DURATION: Duration = Duration::from_secs(15);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HelperCommand {
    Status,
    SetFanMode,
    SetFanRPM,
    ResetAll,
    Heartbeat,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HelperRequest {
    pub command: HelperCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<FanMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperResponse {
    pub ok: bool,
    pub message: String,
    pub is_root: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
    #[serde(default, rename = "actualRPM", skip_serializing_if = "Option::is_none")]
    pub actual_rpm: Option<u16>,
}
impl HelperResponse {
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            is_root: false,
            protocol_version: None,
            actual_rpm: None,
        }
    }
    fn daemon(result: Result<CommandOutcome>) -> Self {
        let (ok, message, actual_rpm) = match result {
            Ok(outcome) => (true, outcome.message, outcome.actual_rpm),
            Err(error) => (false, error.to_string(), None),
        };
        Self {
            ok,
            message,
            is_root: true,
            protocol_version: Some(PROTOCOL_VERSION),
            actual_rpm,
        }
    }
    pub fn compatible(&self) -> bool {
        self.ok && self.is_root && self.protocol_version == Some(PROTOCOL_VERSION)
    }
}
struct CommandOutcome {
    message: String,
    actual_rpm: Option<u16>,
}
impl From<&str> for CommandOutcome {
    fn from(message: &str) -> Self {
        Self {
            message: message.into(),
            actual_rpm: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HelperClient {
    path: PathBuf,
}
impl Default for HelperClient {
    fn default() -> Self {
        Self {
            path: SOCKET_PATH.into(),
        }
    }
}
impl HelperClient {
    pub fn status(&self) -> HelperResponse {
        if self.path == Path::new(SOCKET_PATH)
            && fs::symlink_metadata(&self.path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            // Migration diagnostics may read the legacy endpoint, never mutate it.
            let mut legacy = Self {
                path: LEGACY_SOCKET_PATH.into(),
            }
            .send(
                HelperCommand::Status,
                None,
                None,
                None,
                Duration::from_secs(1),
            );
            legacy.ok = false;
            if legacy.is_root {
                legacy.message = "helper requires installation at the secure runtime path".into();
            }
            return legacy;
        }
        self.send(
            HelperCommand::Status,
            None,
            None,
            None,
            Duration::from_secs(1),
        )
    }
    pub fn heartbeat(&self) -> HelperResponse {
        self.send(
            HelperCommand::Heartbeat,
            None,
            None,
            None,
            Duration::from_secs(2),
        )
    }
    pub fn set_fan_mode(&self, id: i64, mode: FanMode) -> HelperResponse {
        self.write_command(
            HelperCommand::SetFanMode,
            Some(id),
            Some(mode),
            None,
            Duration::from_secs(15),
        )
    }
    pub fn set_fan_rpm(&self, id: i64, rpm: i64) -> HelperResponse {
        self.write_command(
            HelperCommand::SetFanRPM,
            Some(id),
            None,
            Some(rpm),
            Duration::from_secs(15),
        )
    }
    pub fn reset_all(&self) -> HelperResponse {
        self.write_command(
            HelperCommand::ResetAll,
            None,
            None,
            None,
            Duration::from_secs(5),
        )
    }
    fn write_command(
        &self,
        command: HelperCommand,
        fan_id: Option<i64>,
        mode: Option<FanMode>,
        rpm: Option<i64>,
        timeout: Duration,
    ) -> HelperResponse {
        self.send(command, fan_id, mode, rpm, timeout)
    }
    fn send(
        &self,
        command: HelperCommand,
        fan_id: Option<i64>,
        mode: Option<FanMode>,
        rpm: Option<i64>,
        timeout: Duration,
    ) -> HelperResponse {
        let request = HelperRequest {
            command,
            protocol_version: (command != HelperCommand::Status).then_some(PROTOCOL_VERSION),
            fan_id,
            mode,
            rpm,
        };
        let result = (|| -> Result<HelperResponse> {
            let metadata = fs::symlink_metadata(&self.path)?;
            if !metadata.file_type().is_socket() || metadata.uid() != 0 {
                return Err(Error(
                    "helper socket is not a root-owned Unix socket".into(),
                ));
            }
            let mut stream = connect_with_timeout(&self.path, timeout)?;
            if peer_identity(&stream)?.uid != 0 {
                return Err(Error("helper peer is not root".into()));
            }
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            disable_sigpipe(&stream)?;
            exchange_on_stream(&mut stream, &request, timeout)
        })();
        result.unwrap_or_else(|error| HelperResponse::failed(error.to_string()))
    }
}

fn status_request() -> HelperRequest {
    HelperRequest {
        command: HelperCommand::Status,
        protocol_version: None,
        fan_id: None,
        mode: None,
        rpm: None,
    }
}
fn read_response(stream: &mut impl Read, timeout: Duration) -> Result<HelperResponse> {
    let response: HelperResponse = serde_json::from_slice(&read_frame(stream, timeout)?)
        .map_err(|error| Error(error.to_string()))?;
    if !response.is_root {
        return Err(Error("helper did not attest root service".into()));
    }
    Ok(response)
}
// The caller authenticates this socket's kernel peer once. Negotiation and the
// mutation remain bound to that same connection, even if its path is replaced.
fn exchange_on_stream(
    stream: &mut (impl Read + Write),
    request: &HelperRequest,
    timeout: Duration,
) -> Result<HelperResponse> {
    if request.command != HelperCommand::Status {
        write_frame(stream, &status_request())?;
        let status = read_response(stream, timeout)?;
        if !status.compatible() {
            return Ok(HelperResponse {
                ok: false,
                message: if status.protocol_version != Some(PROTOCOL_VERSION) {
                    "helper requires an update".into()
                } else {
                    status.message
                },
                ..status
            });
        }
    }
    write_frame(stream, request)?;
    let response = read_response(stream, timeout)?;
    if request.command != HelperCommand::Status
        && response.protocol_version != Some(PROTOCOL_VERSION)
    {
        return Err(Error("helper requires an update".into()));
    }
    Ok(response)
}

fn connect_with_timeout(path: &Path, timeout: Duration) -> Result<UnixStream> {
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(Error("invalid Unix socket path".into()));
    }
    address.sun_family = libc::AF_UNIX as _;
    for (destination, source) in address.sun_path.iter_mut().zip(bytes) {
        *destination = *source as libc::c_char;
    }
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as u8;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_nonblocking(true)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    disable_sigpipe(&stream)?;
    if unsafe { libc::connect(fd, &address as *const _ as _, length as libc::socklen_t) } != 0 {
        let error = std::io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::EINPROGRESS | libc::EAGAIN)) {
            return Err(error.into());
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error("helper connect timeout".into()));
            }
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLOUT,
                revents: 0,
            };
            let result = unsafe {
                libc::poll(
                    &mut poll,
                    1,
                    remaining.as_millis().clamp(1, i32::MAX as u128) as i32,
                )
            };
            if result < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            if result < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if result == 0 {
                return Err(Error("helper connect timeout".into()));
            }
            let mut socket_error = 0i32;
            let mut error_length = std::mem::size_of_val(&socket_error) as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    &mut socket_error as *mut _ as _,
                    &mut error_length,
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            if socket_error != 0 {
                return Err(std::io::Error::from_raw_os_error(socket_error).into());
            }
            break;
        }
    }
    stream.set_nonblocking(false)?;
    Ok(stream)
}

fn write_frame<T: Serialize>(stream: &mut impl Write, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| Error(error.to_string()))?;
    if bytes.len() > MAX_FRAME {
        return Err(Error("frame too large".into()));
    }
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    Ok(())
}
fn read_frame(stream: &mut impl Read, timeout: Duration) -> Result<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut byte = [0];
    loop {
        if Instant::now() >= deadline {
            return Err(Error("frame deadline exceeded".into()));
        }
        match stream.read(&mut byte) {
            Ok(0) => return Err(Error("incomplete frame".into())),
            Ok(_) if byte[0] == b'\n' => return Ok(bytes),
            Ok(_) => {
                if bytes.len() == MAX_FRAME {
                    return Err(Error("frame too large".into()));
                }
                bytes.push(byte[0]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Peer {
    uid: u32,
    pid: i32,
}
#[cfg(target_os = "macos")]
fn peer_identity(stream: &UnixStream) -> Result<Peer> {
    let mut uid = 0;
    let mut gid = 0;
    let mut pid: libc::pid_t = 0;
    let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
    let fd = stream.as_raw_fd();
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // LOCAL_PEERPID and SOL_LOCAL are verified against the active macOS SDK's sys/un.h.
    if unsafe { libc::getsockopt(fd, 0, 2, &mut pid as *mut _ as _, &mut size) } != 0
        || size as usize != std::mem::size_of_val(&pid)
    {
        return Err(Error("peer process identity unavailable".into()));
    }
    Ok(Peer { uid, pid })
}
#[cfg(not(target_os = "macos"))]
fn peer_identity(_: &UnixStream) -> Result<Peer> {
    Err(Error("privileged helper requires macOS".into()))
}
fn disable_sigpipe(stream: &UnixStream) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let yes: libc::c_int = 1;
        if unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                &yes as *const _ as _,
                std::mem::size_of_val(&yes) as _,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = stream;
    Ok(())
}
fn authorized_peer(peer: Peer) -> bool {
    if !process_alive(peer.pid) {
        return false;
    }
    if peer.uid == 0 {
        return true;
    }
    fs::metadata("/dev/console")
        .map(|m| m.uid() == peer.uid)
        .unwrap_or(false)
}
fn safe_directory(owner: u32, mode: u32, is_directory: bool, has_acl: bool) -> bool {
    is_directory && owner == 0 && mode & 0o022 == 0 && !has_acl
}
#[cfg(target_os = "macos")]
fn directory_has_acl(path: &str) -> Result<bool> {
    use std::ffi::c_void;
    use std::os::unix::fs::OpenOptionsExt;
    extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut c_void;
        fn acl_get_entry(
            acl: *mut c_void,
            entry_id: libc::c_int,
            entry: *mut *mut c_void,
        ) -> libc::c_int;
        fn acl_free(acl: *mut c_void) -> libc::c_int;
    }
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)?;
    let metadata = directory.metadata()?;
    if !safe_directory(metadata.uid(), metadata.mode(), metadata.is_dir(), false) {
        return Err(Error("unsafe directory for ACL query".into()));
    }
    let acl = unsafe { acl_get_fd_np(directory.as_raw_fd(), 0x100) }; // SDK ACL_TYPE_EXTENDED.
    if acl.is_null() {
        let error = std::io::Error::last_os_error();
        // Darwin reports an absent extended ACL as ENOENT. The owned, validated
        // directory fd remains open, so this cannot mean a missing path object.
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(false)
        } else {
            Err(error.into())
        };
    }
    let mut entry = std::ptr::null_mut();
    let status = unsafe { acl_get_entry(acl, 0, &mut entry) }; // ACL_FIRST_ENTRY.
    let error = std::io::Error::last_os_error();
    unsafe {
        acl_free(acl);
    }
    match status {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(error.into()),
    }
}
#[cfg(not(target_os = "macos"))]
fn directory_has_acl(_: &str) -> Result<bool> {
    Err(Error("privileged helper requires macOS".into()))
}

struct Lease {
    owner: Peer,
    renewed: Instant,
}
struct Engine {
    smc: Smc,
    authorize_peer: Arc<dyn Fn(Peer) -> bool + Send + Sync>,
    lease: Option<Lease>,
    sleeping: bool,
    pending_handback: bool,
}
impl Engine {
    fn handle(&mut self, request: HelperRequest, peer: Peer) -> Result<CommandOutcome> {
        // Framing and waiting for this mutex can outlive a console-user switch.
        // Check again before a request can renew/create a lease or access SMC.
        if !(self.authorize_peer)(peer) {
            return Err(Error("unauthorized peer after request wait".into()));
        }
        if request.command != HelperCommand::Status
            && request.protocol_version != Some(PROTOCOL_VERSION)
        {
            return Err(Error(
                "request protocol version is missing or incompatible".into(),
            ));
        }
        self.expire_lease(Instant::now())?;
        match request.command {
            HelperCommand::Status => Ok("ready".into()),
            HelperCommand::ResetAll => {
                self.pending_handback = true;
                self.smc.reset_all()?;
                self.lease = None;
                self.pending_handback = false;
                Ok("reset".into())
            }
            HelperCommand::Heartbeat => {
                if let Some(lease) = &mut self.lease {
                    if lease.owner != peer {
                        return Err(Error("manual control belongs to another process".into()));
                    }
                    lease.renewed = Instant::now();
                }
                Ok("alive".into())
            }
            HelperCommand::SetFanMode | HelperCommand::SetFanRPM => {
                if self.sleeping {
                    return Err(Error("system is sleeping; write refused".into()));
                }
                if let Some(lease) = &self.lease {
                    if lease.owner != peer {
                        return Err(Error("manual control belongs to another process".into()));
                    }
                }
                let id = request
                    .fan_id
                    .ok_or_else(|| Error("missing fanId".into()))?;
                if !(self.authorize_peer)(peer) {
                    return Err(Error("peer authorization changed before write".into()));
                }
                // SMC calls/retries can also outlive a switch. Recheck immediately
                // before every custom write, while automatic recovery stays allowed.
                let authorize = self.authorize_peer.clone();
                self.smc
                    .set_cancellation_check(move || shutdown_requested() || !authorize(peer));
                // Establish the lease before attempting a write so partial failures are watched.
                self.lease = Some(Lease {
                    owner: peer,
                    renewed: Instant::now(),
                });
                match request.command {
                    HelperCommand::SetFanMode => {
                        let mode = request.mode.ok_or_else(|| Error("missing mode".into()))?;
                        self.smc.set_fan_mode(id, mode)?;
                        Ok("mode set".into())
                    }
                    _ => {
                        let rpm = request.rpm.ok_or_else(|| Error("missing rpm".into()))?;
                        let actual = self.smc.set_fan_rpm(id, rpm)?;
                        Ok(CommandOutcome {
                            message: format!("rpm set: {actual}"),
                            actual_rpm: Some(actual),
                        })
                    }
                }
            }
        }
    }
    fn expire_lease(&mut self, now: Instant) -> Result<()> {
        let expired = self.lease.as_ref().is_some_and(|lease| {
            lease_expired(
                lease.renewed,
                now,
                (self.authorize_peer)(lease.owner),
                process_alive(lease.owner.pid),
            )
        });
        if expired || self.pending_handback {
            // A failed reset retains the expired lease so every watchdog tick retries.
            self.pending_handback = true;
            self.smc.reset_all()?;
            self.lease = None;
            self.pending_handback = false;
            eprintln!("[FanControlHelper] lease expired; returned fans to system control");
        }
        Ok(())
    }
}
// Each connection permits at most Status + one command; each frame has its own
// bounded size/deadline. A mutation cannot bypass negotiation on a fresh stream.
fn serve_session(
    stream: &mut (impl Read + Write),
    engine: &Mutex<Engine>,
    peer: Peer,
    mut identify_peer: impl FnMut() -> Result<Peer>,
) -> Result<()> {
    let mut negotiated = false;
    for _ in 0..2 {
        let mut is_status = false;
        let result = (|| -> Result<CommandOutcome> {
            let bytes = read_frame(stream, Duration::from_secs(2))?;
            let request: HelperRequest =
                serde_json::from_slice(&bytes).map_err(|_| Error("bad request".into()))?;
            is_status = request.command == HelperCommand::Status;
            if !is_status && !negotiated {
                return Err(Error(
                    "status negotiation required on this connection".into(),
                ));
            }
            let mut engine = engine.lock().unwrap_or_else(|p| p.into_inner());
            if shutdown_requested() {
                return Err(Error("shutting down".into()));
            }
            if identify_peer()? != peer {
                return Err(Error("peer identity changed during request wait".into()));
            }
            engine.handle(request, peer)
        })();
        let continue_session = is_status && result.is_ok();
        write_frame(stream, &HelperResponse::daemon(result))?;
        if !continue_session {
            break;
        }
        negotiated = true;
    }
    Ok(())
}
// macOS accept() inherits O_NONBLOCK from the listener. Without clearing it, a
// frame that has not arrived yet fails with WouldBlock and drops the session.
fn prepare_accepted_stream(stream: &UnixStream) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    Ok(())
}
fn lease_expired(renewed: Instant, now: Instant, authorized: bool, process_running: bool) -> bool {
    now.saturating_duration_since(renewed) >= LEASE_DURATION || !authorized || !process_running
}
fn process_alive(pid: i32) -> bool {
    pid > 0
        && (unsafe { libc::kill(pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}
extern "C" fn on_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
fn shutdown_requested() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}
fn initialize_with_power<T>(
    mut smc: Smc,
    authorize_peer: Arc<dyn Fn(Peer) -> bool + Send + Sync>,
    register: impl FnOnce(&Arc<Mutex<Engine>>) -> Result<T>,
) -> Result<(Arc<Mutex<Engine>>, T)> {
    // Confirm recovery from a previous daemon before any fallible power setup.
    // A failed hand-back exits with an error so launchd will retry startup.
    smc.reset_all()?;
    let engine = Arc::new(Mutex::new(Engine {
        smc,
        authorize_peer,
        lease: None,
        sleeping: false,
        pending_handback: false,
    }));
    let power = register(&engine)?;
    Ok((engine, power))
}

/// Run only from the same executable's `--helper` entry point; never from GUI startup.
pub fn run_helper() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(Error("helper requires root".into()));
    }
    SHUTDOWN.store(false, Ordering::SeqCst);
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let socket = Path::new(SOCKET_PATH);
    // Every ancestor is root-owned and rejects group/world write and symlinks.
    for directory in [
        "/Library",
        "/Library/PrivilegedHelperTools",
        RUNTIME_DIRECTORY,
    ] {
        let parent = fs::symlink_metadata(directory)?;
        if !safe_directory(
            parent.uid(),
            parent.mode(),
            parent.file_type().is_dir(),
            directory_has_acl(directory)?,
        ) {
            return Err(Error(format!(
                "unsafe socket parent directory: {directory}"
            )));
        }
    }
    match fs::symlink_metadata(socket) {
        Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == 0 => {
            fs::remove_file(socket)?
        }
        Ok(_) => {
            return Err(Error(
                "refusing to replace unexpected helper socket path".into(),
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o666))?;
    listener.set_nonblocking(true)?;
    let mut smc = Smc::open()?;
    smc.set_cancellation_check(shutdown_requested);
    let (engine, power) = initialize_with_power(smc, Arc::new(authorized_peer), |engine| {
        let power_engine = engine.clone();
        crate::PowerMonitor::register(move |event| {
            let mut engine = power_engine.lock().unwrap_or_else(|p| p.into_inner());
            match event {
                crate::PowerEvent::WillSleep => {
                    engine.sleeping = true;
                    engine.pending_handback = true;
                    match engine.smc.reset_all() {
                        Ok(()) => {
                            engine.lease = None;
                            engine.pending_handback = false;
                        }
                        Err(error) => eprintln!(
                        "[FanControlHelper] sleep handback failed; watchdog will retry: {error}"
                    ),
                    }
                }
                crate::PowerEvent::WillWake | crate::PowerEvent::DidWake => {
                    engine.sleeping = false;
                }
            }
        })
    })?;
    let watchdog_engine = engine.clone();
    let watchdog = std::thread::spawn(move || {
        while !SHUTDOWN.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(500));
            let mut engine = watchdog_engine.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(error) = engine.expire_lease(Instant::now()) {
                eprintln!("[FanControlHelper] watchdog handback failed: {error}");
            }
        }
    });
    eprintln!("[FanControlHelper] ready protocol={PROTOCOL_VERSION}");
    let mut accept_error = None;
    while !SHUTDOWN.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = disable_sigpipe(&stream);
                let result = (|| -> Result<()> {
                    prepare_accepted_stream(&stream)?;
                    let peer = peer_identity(&stream)?;
                    if !authorized_peer(peer) {
                        return Err(Error("unauthorized peer".into()));
                    }
                    let identity_stream = stream.try_clone()?;
                    serve_session(&mut stream, &engine, peer, || {
                        peer_identity(&identity_stream)
                    })
                })();
                if let Err(error) = result {
                    let _ = write_frame(&mut stream, &HelperResponse::daemon(Err(error)));
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::Interrupted =>
            {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(error) => {
                accept_error = Some(error);
                break;
            }
        }
    }
    SHUTDOWN.store(true, Ordering::SeqCst);
    let _ = fs::remove_file(socket);
    let reset = engine
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .smc
        .reset_all();
    let _ = watchdog.join();
    drop(power);
    reset?;
    if let Some(error) = accept_error {
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smc::{KeyValue, SmcTransport};
    use std::collections::HashMap;
    use std::io::Cursor;
    #[test]
    fn accepted_stream_waits_for_a_delayed_second_frame() {
        let path = std::env::temp_dir().join(format!("fan-accept-{}.sock", std::process::id()));
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut client = UnixStream::connect(&path).unwrap();
        let mut server = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("{error}"),
            }
        };
        prepare_accepted_stream(&server).unwrap();
        let writer = std::thread::spawn(move || {
            client.write_all(b"first\n").unwrap();
            std::thread::sleep(Duration::from_millis(300));
            client.write_all(b"second\n").unwrap();
            client
        });
        assert_eq!(
            read_frame(&mut server, Duration::from_secs(2)).unwrap(),
            b"first"
        );
        assert_eq!(
            read_frame(&mut server, Duration::from_secs(2)).unwrap(),
            b"second"
        );
        drop(writer.join().unwrap());
        fs::remove_file(&path).unwrap();
    }
    #[test]
    fn socket_directory_rejects_unprivileged_write_or_acl_grants() {
        assert!(safe_directory(0, 0o755, true, false));
        assert!(!safe_directory(0, 0o775, true, false));
        assert!(!safe_directory(501, 0o755, true, false));
        assert!(!safe_directory(0, 0o755, false, false));
        assert!(!safe_directory(0, 0o755, true, true));
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn missing_directory_cannot_be_confused_with_missing_acl() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("fan-nonexistent-{}-{unique}", std::process::id()));
        assert!(!path.exists());
        assert!(directory_has_acl(path.to_str().unwrap()).is_err());
    }
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "opt-in, read-only existing system directory ACL probe"]
    fn native_runtime_parent_acl_readonly_probe() {
        for path in ["/Library", "/Library/PrivilegedHelperTools"] {
            let metadata = fs::symlink_metadata(path).unwrap();
            let has_acl = directory_has_acl(path).unwrap();
            assert!(safe_directory(
                metadata.uid(),
                metadata.mode(),
                metadata.is_dir(),
                has_acl
            ));
            println!(
                "{path}: root owned, mode {:o}, extended ACL present: {has_acl}",
                metadata.mode() & 0o777
            );
        }
    }
    #[test]
    fn console_user_switch_or_dead_owner_expires_live_lease() {
        let now = Instant::now();
        assert!(!lease_expired(now, now, true, true));
        assert!(lease_expired(now, now, false, true));
        assert!(lease_expired(now, now, true, false));
    }
    #[test]
    fn failed_sleep_handback_remains_pending_for_watchdog_retry() {
        let owner = Peer {
            uid: 0,
            pid: std::process::id() as i32,
        };
        let mut engine = Engine {
            smc: Smc::with_transport(Box::new(Mock {
                keys: HashMap::new(),
                writes: Arc::new(Mutex::new(Vec::new())),
            })),
            authorize_peer: Arc::new(authorized_peer),
            lease: Some(Lease {
                owner,
                renewed: Instant::now(),
            }),
            sleeping: true,
            pending_handback: true,
        };
        assert!(engine.expire_lease(Instant::now()).is_err());
        assert!(engine.pending_handback);
        assert!(engine.lease.is_some());
        let write = HelperRequest {
            command: HelperCommand::SetFanRPM,
            protocol_version: Some(PROTOCOL_VERSION),
            fan_id: Some(0),
            rpm: Some(2000),
            mode: None,
        };
        assert!(engine.handle(write, owner).is_err());
    }
    #[test]
    fn legacy_swift_wire_keys_and_numeric_mode_are_preserved() {
        let request: HelperRequest =
            serde_json::from_str(r#"{"command":"setFanRPM","fanId":0,"rpm":1800}"#).unwrap();
        assert_eq!(request.command, HelperCommand::SetFanRPM);
        let request: HelperRequest =
            serde_json::from_str(r#"{"command":"setFanMode","fanId":0,"mode":1}"#).unwrap();
        assert_eq!(request.mode, Some(FanMode::Forced));
        let response = serde_json::to_value(HelperResponse::daemon(Ok("ready".into()))).unwrap();
        assert_eq!(response["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["isRoot"], true);
        let applied = serde_json::to_value(HelperResponse::daemon(Ok(CommandOutcome {
            message: "rpm set".into(),
            actual_rpm: Some(1499),
        })))
        .unwrap();
        assert_eq!(applied["actualRPM"], 1499);
        let legacy: HelperResponse = serde_json::from_str(
            r#"{"ok":true,"message":"ready","isRoot":true,"protocolVersion":4}"#,
        )
        .unwrap();
        assert_eq!(legacy.actual_rpm, None);
        assert!(
            serde_json::from_str::<HelperRequest>(r#"{"command":"setFanMode","mode":3}"#).is_err()
        );
    }
    #[test]
    fn frames_reject_oversized_and_unterminated_data() {
        let mut valid = Cursor::new(b"{}\nignored".to_vec());
        assert_eq!(
            read_frame(&mut valid, Duration::from_secs(1)).unwrap(),
            b"{}"
        );
        let mut partial = Cursor::new(b"{}".to_vec());
        assert!(read_frame(&mut partial, Duration::from_secs(1)).is_err());
        let mut oversized = Cursor::new(vec![b'x'; MAX_FRAME + 1]);
        assert!(read_frame(&mut oversized, Duration::from_secs(1)).is_err());
        let mut data = Cursor::new(b"{}\n".to_vec());
        assert!(read_frame(&mut data, Duration::ZERO).is_err());
    }
    #[test]
    fn unix_connect_is_bounded_and_peer_identity_comes_from_kernel() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fan-platform-test-{}-{unique}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let stream = connect_with_timeout(&path, Duration::from_secs(1)).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        #[cfg(target_os = "macos")]
        {
            let peer = peer_identity(&accepted).unwrap();
            assert_eq!(peer.pid, std::process::id() as i32);
            assert_eq!(peer.uid, unsafe { libc::geteuid() });
        }
        drop(accepted);
        drop(stream);
        drop(listener);
        fs::remove_file(&path).unwrap();
        assert!(connect_with_timeout(&path, Duration::from_millis(10)).is_err());
        assert!(
            connect_with_timeout(Path::new(&"a".repeat(512)), Duration::from_millis(10)).is_err()
        );
    }
    struct Mock {
        keys: HashMap<String, KeyValue>,
        writes: Arc<Mutex<Vec<String>>>,
    }
    impl SmcTransport for Mock {
        fn read(&mut self, key: &str) -> Result<KeyValue> {
            self.keys
                .get(key)
                .cloned()
                .ok_or_else(|| Error("missing".into()))
        }
        fn read_optional(&mut self, key: &str) -> Result<Option<KeyValue>> {
            Ok(self.keys.get(key).cloned())
        }
        fn write(&mut self, key: &str, value: &KeyValue) -> Result<()> {
            self.writes.lock().unwrap().push(key.into());
            self.keys.insert(key.into(), value.clone());
            Ok(())
        }
        fn keys(&mut self) -> Result<Vec<String>> {
            Ok(self.keys.keys().cloned().collect())
        }
    }
    fn custom_request(command: HelperCommand) -> HelperRequest {
        HelperRequest {
            command,
            protocol_version: Some(PROTOCOL_VERSION),
            fan_id: Some(0),
            mode: Some(FanMode::Forced),
            rpm: Some(2000),
        }
    }
    fn empty_engine(
        authorize_peer: Arc<dyn Fn(Peer) -> bool + Send + Sync>,
    ) -> (Engine, Arc<Mutex<Vec<String>>>) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        (
            Engine {
                smc: Smc::with_transport(Box::new(Mock {
                    keys: HashMap::new(),
                    writes: writes.clone(),
                })),
                authorize_peer,
                lease: None,
                sleeping: false,
                pending_handback: false,
            },
            writes,
        )
    }
    struct MemoryStream {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for MemoryStream {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(bytes)
        }
    }
    impl Write for MemoryStream {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn frames<T: Serialize>(values: &[T]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend(serde_json::to_vec(value).unwrap());
            bytes.push(b'\n');
        }
        bytes
    }
    #[test]
    fn incompatible_or_missing_request_versions_never_reach_smc() {
        let peer = Peer {
            uid: 501,
            pid: std::process::id() as i32,
        };
        for command in [
            HelperCommand::SetFanRPM,
            HelperCommand::SetFanMode,
            HelperCommand::ResetAll,
            HelperCommand::Heartbeat,
        ] {
            for version in [None, Some(PROTOCOL_VERSION - 1), Some(PROTOCOL_VERSION + 1)] {
                let (mut engine, writes) = empty_engine(Arc::new(|_| true));
                engine.pending_handback = true;
                let mut request = custom_request(command);
                request.protocol_version = version;
                assert!(engine
                    .handle(request, peer)
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("protocol version"));
                assert!(engine.lease.is_none());
                assert!(writes.lock().unwrap().is_empty());
                assert!(engine.pending_handback);
            }
        }
    }
    #[test]
    fn legacy_status_or_eof_never_sends_a_mutation() {
        let request = custom_request(HelperCommand::SetFanRPM);
        let mut legacy = HelperResponse::daemon(Ok("ready".into()));
        legacy.protocol_version = Some(PROTOCOL_VERSION - 1);
        for input in [frames(&[legacy]), Vec::new()] {
            let mut stream = MemoryStream {
                input: Cursor::new(input),
                output: Vec::new(),
            };
            assert!(
                !exchange_on_stream(&mut stream, &request, Duration::from_secs(1))
                    .is_ok_and(|response| response.ok)
            );
            let emitted: HelperRequest =
                serde_json::from_slice(stream.output.strip_suffix(b"\n").unwrap()).unwrap();
            assert_eq!(emitted.command, HelperCommand::Status);
            assert_eq!(emitted.protocol_version, None);
        }
    }
    #[test]
    fn server_requires_negotiation_and_rejects_legacy_mutation_after_status() {
        let peer = Peer {
            uid: 501,
            pid: std::process::id() as i32,
        };
        let mut legacy = custom_request(HelperCommand::SetFanRPM);
        legacy.protocol_version = None;
        for requests in [
            vec![custom_request(HelperCommand::SetFanRPM)],
            vec![status_request(), legacy],
        ] {
            let (engine, writes) = empty_engine(Arc::new(|_| true));
            let mut stream = MemoryStream {
                input: Cursor::new(frames(&requests)),
                output: Vec::new(),
            };
            serve_session(&mut stream, &Mutex::new(engine), peer, || Ok(peer)).unwrap();
            let responses: Vec<HelperResponse> = stream
                .output
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice(line).unwrap())
                .collect();
            assert!(!responses.last().unwrap().ok);
            assert!(writes.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn socket_path_replacement_cannot_move_negotiated_write_to_new_server() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("fan-session-{}-{unique}.sock", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        let mut client = UnixStream::connect(&path).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let (status_tx, status_rx) = std::sync::mpsc::channel();
        let (replaced_tx, replaced_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let status: HelperRequest =
                serde_json::from_slice(&read_frame(&mut stream, Duration::from_secs(1)).unwrap())
                    .unwrap();
            assert_eq!(status.command, HelperCommand::Status);
            status_tx.send(()).unwrap();
            replaced_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            write_frame(&mut stream, &HelperResponse::daemon(Ok("ready".into()))).unwrap();
            let mutation: HelperRequest =
                serde_json::from_slice(&read_frame(&mut stream, Duration::from_secs(1)).unwrap())
                    .unwrap();
            assert_eq!(mutation.protocol_version, Some(PROTOCOL_VERSION));
            assert_eq!(mutation.command, HelperCommand::SetFanRPM);
            write_frame(
                &mut stream,
                &HelperResponse::daemon(Ok(CommandOutcome {
                    message: "bound connection".into(),
                    actual_rpm: Some(2000),
                })),
            )
            .unwrap();
        });
        let client = std::thread::spawn(move || {
            exchange_on_stream(
                &mut client,
                &custom_request(HelperCommand::SetFanRPM),
                Duration::from_secs(1),
            )
        });
        status_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        replacement.set_nonblocking(true).unwrap();
        replaced_tx.send(()).unwrap();
        assert_eq!(client.join().unwrap().unwrap().actual_rpm, Some(2000));
        server.join().unwrap();
        assert_eq!(
            replacement.accept().err().unwrap().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(replacement);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn startup_handback_precedes_power_registration_failure() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let key = |byte| KeyValue {
            data_type: "ui8 ".into(),
            size: 1,
            bytes: {
                let mut bytes = [0; 32];
                bytes[0] = byte;
                bytes
            },
        };
        let smc = Smc::with_transport(Box::new(Mock {
            keys: HashMap::from([("FNum".into(), key(1)), ("F0md".into(), key(1))]),
            writes: writes.clone(),
        }));
        let result = initialize_with_power(smc, Arc::new(|_| true), |engine| {
            assert!(!engine.lock().unwrap().pending_handback);
            Err::<(), _>(Error("mock power registration failure".into()))
        });
        assert!(result.is_err());
        assert!(writes.lock().unwrap().contains(&"F0md".into()));
        let registered = AtomicBool::new(false);
        let smc = Smc::with_transport(Box::new(Mock {
            keys: HashMap::new(),
            writes,
        }));
        assert!(initialize_with_power(smc, Arc::new(|_| true), |_| {
            registered.store(true, Ordering::SeqCst);
            Ok(())
        })
        .is_err());
        assert!(!registered.load(Ordering::SeqCst));
    }
    #[test]
    fn console_switch_after_frame_rejects_custom_requests_without_creating_lease() {
        let peer = Peer {
            uid: 501,
            pid: std::process::id() as i32,
        };
        for command in [HelperCommand::SetFanRPM, HelperCommand::SetFanMode] {
            let allowed = Arc::new(AtomicBool::new(true));
            let state = allowed.clone();
            let authorization: Arc<dyn Fn(Peer) -> bool + Send + Sync> =
                Arc::new(move |_| state.load(Ordering::SeqCst));
            let (mut engine, writes) = empty_engine(authorization.clone());
            assert!(authorization(peer)); // Accepted before reading the frame.
            let mut bytes = serde_json::to_vec(&custom_request(command)).unwrap();
            bytes.push(b'\n');
            let frame = read_frame(&mut Cursor::new(bytes), Duration::from_secs(1)).unwrap();
            allowed.store(false, Ordering::SeqCst);
            let request = serde_json::from_slice(&frame).unwrap();
            assert!(engine
                .handle(request, peer)
                .err()
                .unwrap()
                .to_string()
                .contains("unauthorized"));
            assert!(engine.lease.is_none());
            assert!(writes.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn console_switch_while_waiting_for_mutex_cannot_start_control() {
        let peer = Peer {
            uid: 501,
            pid: std::process::id() as i32,
        };
        let allowed = Arc::new(AtomicBool::new(true));
        let state = allowed.clone();
        let authorization: Arc<dyn Fn(Peer) -> bool + Send + Sync> =
            Arc::new(move |_| state.load(Ordering::SeqCst));
        let (engine, writes) = empty_engine(authorization.clone());
        let engine = Arc::new(Mutex::new(engine));
        let held_lock = engine.lock().unwrap();
        let queued_engine = engine.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            assert!(authorization(peer));
            tx.send(()).unwrap();
            queued_engine
                .lock()
                .unwrap()
                .handle(custom_request(HelperCommand::SetFanRPM), peer)
        });
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        allowed.store(false, Ordering::SeqCst);
        drop(held_lock);
        assert!(worker.join().unwrap().is_err());
        assert!(engine.lock().unwrap().lease.is_none());
        assert!(writes.lock().unwrap().is_empty());
    }
    #[test]
    fn revoked_peer_cannot_renew_existing_lease() {
        let peer = Peer {
            uid: 501,
            pid: std::process::id() as i32,
        };
        let (mut engine, writes) = empty_engine(Arc::new(|_| false));
        let renewed = Instant::now();
        engine.lease = Some(Lease {
            owner: peer,
            renewed,
        });
        assert!(engine
            .handle(
                HelperRequest {
                    command: HelperCommand::Heartbeat,
                    protocol_version: Some(PROTOCOL_VERSION),
                    fan_id: None,
                    mode: None,
                    rpm: None,
                },
                peer
            )
            .is_err());
        assert_eq!(engine.lease.as_ref().unwrap().renewed, renewed);
        assert!(writes.lock().unwrap().is_empty());
    }
    #[test]
    fn authorization_revoked_during_smc_reads_cancels_before_custom_write() {
        struct RevokingRead {
            inner: Mock,
            allowed: Arc<AtomicBool>,
        }
        impl SmcTransport for RevokingRead {
            fn read(&mut self, key: &str) -> Result<KeyValue> {
                if key == "F0Mn" {
                    self.allowed.store(false, Ordering::SeqCst);
                }
                self.inner.read(key)
            }
            fn read_optional(&mut self, key: &str) -> Result<Option<KeyValue>> {
                self.inner.read_optional(key)
            }
            fn write(&mut self, key: &str, value: &KeyValue) -> Result<()> {
                self.inner.write(key, value)
            }
            fn keys(&mut self) -> Result<Vec<String>> {
                self.inner.keys()
            }
        }
        fn value(kind: &str, bytes: &[u8]) -> KeyValue {
            let mut value = KeyValue {
                data_type: kind.into(),
                size: bytes.len() as u32,
                bytes: [0; 32],
            };
            value.bytes[..bytes.len()].copy_from_slice(bytes);
            value
        }
        let allowed = Arc::new(AtomicBool::new(true));
        let state = allowed.clone();
        let authorization = Arc::new(move |_: Peer| state.load(Ordering::SeqCst));
        let (mut engine, writes) = empty_engine(authorization);
        engine.smc = Smc::with_transport(Box::new(RevokingRead {
            inner: Mock {
                keys: HashMap::from([
                    ("FNum".into(), value("ui8 ", &[1])),
                    ("F0md".into(), value("ui8 ", &[3])),
                    ("F0Mn".into(), value("flt ", &1200f32.to_le_bytes())),
                    ("F0Mx".into(), value("flt ", &6000f32.to_le_bytes())),
                ]),
                writes: writes.clone(),
            },
            allowed,
        }));
        let peer = Peer {
            uid: 501,
            pid: std::process::id() as i32,
        };
        assert!(engine
            .handle(custom_request(HelperCommand::SetFanRPM), peer)
            .is_err());
        assert!(writes.lock().unwrap().is_empty());
        // Even after revocation, daemon-owned automatic recovery remains available.
        engine.expire_lease(Instant::now()).unwrap();
        assert!(engine.lease.is_none());
    }
    #[test]
    fn watchdog_resets_expired_lease_and_another_pid_cannot_extend_it() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut keys = HashMap::new();
        keys.insert(
            "FNum".into(),
            KeyValue {
                data_type: "ui8 ".into(),
                size: 1,
                bytes: {
                    let mut b = [0; 32];
                    b[0] = 1;
                    b
                },
            },
        );
        keys.insert(
            "F0md".into(),
            KeyValue {
                data_type: "ui8 ".into(),
                size: 1,
                bytes: {
                    let mut b = [0; 32];
                    b[0] = 1;
                    b
                },
            },
        );
        let owner = Peer {
            uid: unsafe { libc::getuid() },
            pid: std::process::id() as i32,
        };
        let mut engine = Engine {
            smc: Smc::with_transport(Box::new(Mock {
                keys,
                writes: writes.clone(),
            })),
            authorize_peer: Arc::new(authorized_peer),
            lease: Some(Lease {
                owner,
                renewed: Instant::now(),
            }),
            sleeping: false,
            pending_handback: false,
        };
        let heartbeat = HelperRequest {
            command: HelperCommand::Heartbeat,
            protocol_version: Some(PROTOCOL_VERSION),
            fan_id: None,
            mode: None,
            rpm: None,
        };
        assert!(engine
            .handle(
                heartbeat,
                Peer {
                    pid: owner.pid + 1,
                    ..owner
                }
            )
            .is_err());
        engine
            .expire_lease(Instant::now() + LEASE_DURATION)
            .unwrap();
        assert!(engine.lease.is_none());
        assert!(writes.lock().unwrap().contains(&"F0md".into()));
    }
}
