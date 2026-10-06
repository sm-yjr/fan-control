//! Generates a reviewable privileged install transaction in root-owned directories.
use crate::helper::{
    HelperClient, HelperResponse, DAEMON_PATH, LABEL, LEGACY_SOCKET_PATH, LOG_DIRECTORY, LOG_PATH,
    RUNTIME_DIRECTORY, SOCKET_PATH, TOOL_PATH,
};
use crate::{Error, Result};
use std::path::Path;
use std::time::{Duration, Instant};

pub fn launch_daemon_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{TOOL_PATH}</string><string>--helper</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>{LOG_PATH}</string>
<key>StandardErrorPath</key><string>{LOG_PATH}</string>
<key>ProcessType</key><string>Interactive</string>
<key>ExitTimeOut</key><integer>60</integer>
</dict></plist>
"#
    )
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
/// Rejects a directory that anyone besides its owner can write. Earlier
/// releases created root-owned directories such as 744; those stay valid.
fn directory_mode_check(quoted_dir: &str) -> String {
    format!("fan_dir_mode=$(/usr/bin/stat -f %Lp {quoted_dir}) || exit 72; [ $((0$fan_dir_mode & 022)) -eq 0 ] || exit 72")
}
fn apple_script_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn install_script(bundled_helper: &Path) -> Result<String> {
    if !bundled_helper.is_absolute() {
        return Err(Error("bundled helper path must be absolute".into()));
    }
    let source = bundled_helper
        .to_str()
        .ok_or_else(|| Error("helper path is not UTF-8".into()))?;
    if source.contains(['\0', '\n', '\r']) {
        return Err(Error("invalid helper path".into()));
    }
    let tool_stage = format!("{TOOL_PATH}.installing");
    let plist_stage = format!("{DAEMON_PATH}.installing");
    let mut lines = vec!["set -eu".to_string()];
    // Validate every directory that root will write through. A pre-existing symlink,
    // wrong owner, or writable directory is rejected rather than silently repaired.
    for dir in [
        "/Library",
        "/Library/PrivilegedHelperTools",
        RUNTIME_DIRECTORY,
        "/Library/LaunchDaemons",
        "/Library/Logs",
        LOG_DIRECTORY,
    ] {
        let dir = shell_quote(dir);
        lines.push(format!("if [ -L {dir} ]; then exit 70; fi"));
        lines.push(format!(
            "if [ ! -d {dir} ]; then /usr/bin/install -d -m 755 -o root -g wheel {dir}; fi"
        ));
        lines.push(format!(
            "[ \"$(/usr/bin/stat -f %u {dir})\" = 0 ] || exit 71"
        ));
        lines.push(directory_mode_check(&dir));
        lines.push(format!("fan_dir_acl=$(/bin/ls -lde {dir}) || exit 72; [ -z \"$(/usr/bin/printf '%s\\n' \"$fan_dir_acl\" | /usr/bin/tail -n +2)\" ] || exit 72"));
    }
    lines.extend([
        format!(
            "/usr/bin/codesign --verify --strict {}",
            shell_quote(source)
        ),
        format!("/bin/rm -f {}", shell_quote(&tool_stage)),
        format!(
            "/usr/bin/install -m 755 -o root -g wheel {} {}",
            shell_quote(source),
            shell_quote(&tool_stage)
        ),
        format!(
            "/usr/bin/codesign --verify --strict {}",
            shell_quote(&tool_stage)
        ),
        format!(
            "/usr/bin/xattr -d com.apple.quarantine {} >/dev/null 2>&1 || true",
            shell_quote(&tool_stage)
        ),
        format!("/bin/rm -f {}", shell_quote(&plist_stage)),
        format!(
            "/bin/cat > {} <<'FAN_CONTROL_RUST_PLIST'\n{}FAN_CONTROL_RUST_PLIST",
            shell_quote(&plist_stage),
            launch_daemon_plist()
        ),
        format!("/usr/sbin/chown root:wheel {}", shell_quote(&plist_stage)),
        format!("/bin/chmod 644 {}", shell_quote(&plist_stage)),
        format!(
            "/usr/bin/plutil -lint {} >/dev/null",
            shell_quote(&plist_stage)
        ),
        format!("[ ! -L {} ] || exit 73", shell_quote(LOG_PATH)),
        format!("if [ -e {log} ]; then [ -f {log} ] || exit 73; [ \"$(/usr/bin/stat -f %u {log})\" = 0 ] || exit 74; /bin/chmod 640 {log}; else /usr/bin/install -m 640 -o root -g wheel /dev/null {log}; fi", log = shell_quote(LOG_PATH)),
        format!("if /bin/launchctl print system/{LABEL} >/dev/null 2>&1; then /bin/launchctl bootout system/{LABEL} || {{ echo 'helper stop failed; installed helper was preserved' >&2; exit 75; }}; fi"),
        format!("attempt=0; while /bin/launchctl print system/{LABEL} >/dev/null 2>&1; do attempt=$((attempt + 1)); [ \"$attempt\" -lt 350 ] || {{ echo 'helper did not stop; installed helper was preserved' >&2; exit 76; }}; /bin/sleep 0.2; done"),
        checked_socket_removal(LEGACY_SOCKET_PATH),
        checked_socket_removal(SOCKET_PATH),
        format!(
            "/bin/mv -f {} {}",
            shell_quote(&tool_stage),
            shell_quote(TOOL_PATH)
        ),
        format!(
            "/bin/mv -f {} {}",
            shell_quote(&plist_stage),
            shell_quote(DAEMON_PATH)
        ),
        format!(
            "/bin/launchctl bootstrap system {}",
            shell_quote(DAEMON_PATH)
        ),
        format!("/bin/launchctl enable system/{LABEL}"),
    ]);
    Ok(lines.join("\n"))
}

/// `bundled_helper` is the app's signed Contents/Library/LaunchServices helper.
/// This function deliberately invokes the native administrator authorization UI.
pub fn install_bundled_helper(bundled_helper: &Path) -> HelperResponse {
    let result = (|| -> Result<HelperResponse> {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::symlink_metadata(bundled_helper)?;
        if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(Error("bundled executable helper is missing".into()));
        }
        let script = install_script(bundled_helper)?;
        let output = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                &format!(
                    "do shell script {} with administrator privileges",
                    apple_script_string(&script)
                ),
            ])
            .output()?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(Error(if message.is_empty() {
                "helper install cancelled or failed".into()
            } else {
                message
            }));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let client = HelperClient::default();
        let mut status;
        loop {
            status = client.status();
            if status.compatible() {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(Error(format!(
            "helper installed but not ready: {}",
            status.message
        )))
    })();
    result.unwrap_or_else(|error| HelperResponse::failed(error.to_string()))
}

/// Reversible preparation belongs to the GUI: pause its control worker before
/// calling this function. A failed hand-back always refuses privileged removal.
pub fn uninstall_helper() -> HelperResponse {
    let result = uninstall_workflow(
        || HelperClient::default().reset_all(),
        || {
            let script = uninstall_script();
            let output = std::process::Command::new("/usr/bin/osascript")
                .args([
                    "-e",
                    &format!(
                        "do shell script {} with administrator privileges",
                        apple_script_string(&script)
                    ),
                ])
                .output()?;
            if !output.status.success() {
                let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                return Err(Error(if message.is_empty() {
                    "helper uninstall cancelled or failed; fans were returned to system control"
                        .into()
                } else {
                    format!("helper uninstall failed after system hand-back: {message}")
                }));
            }
            Ok(())
        },
        || Ok(helper_paths_absent()? && !launchd_service_present()?),
    );
    result.unwrap_or_else(|error| HelperResponse::failed(error.to_string()))
}
fn uninstall_workflow(
    reset: impl FnOnce() -> HelperResponse,
    authorized_remove: impl FnOnce() -> Result<()>,
    mut verified_absent: impl FnMut() -> Result<bool>,
) -> Result<HelperResponse> {
    if verified_absent()? {
        return Ok(removed_response());
    }
    let reset = reset();
    if !reset.compatible() {
        return Err(Error(format!(
            "uninstall refused: automatic hand-back could not be confirmed: {}",
            reset.message
        )));
    }
    authorized_remove()?;
    if !verified_absent()? {
        return Err(Error(
            "uninstall incomplete: helper service or a known path remains".into(),
        ));
    }
    Ok(removed_response())
}
fn removed_response() -> HelperResponse {
    HelperResponse {
        ok: true,
        message: "helper removed; system control restored; diagnostic logs retained".into(),
        is_root: false,
        protocol_version: None,
        actual_rpm: None,
    }
}
fn helper_paths_absent() -> Result<bool> {
    for path in [
        TOOL_PATH,
        DAEMON_PATH,
        SOCKET_PATH,
        LEGACY_SOCKET_PATH,
        RUNTIME_DIRECTORY,
    ] {
        match std::fs::symlink_metadata(path) {
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}
fn launchd_service_present() -> Result<bool> {
    let output = std::process::Command::new("/bin/launchctl")
        .args(["print", &format!("system/{LABEL}")])
        .output()?;
    if output.status.success() {
        return Ok(true);
    }
    let error = String::from_utf8_lossy(&output.stderr);
    if error.contains("Could not find service")
        || error.contains("Could not find specified service")
    {
        Ok(false)
    } else {
        Err(Error(format!(
            "could not verify launchd removal: {}",
            error.trim()
        )))
    }
}

pub fn uninstall_script() -> String {
    let mut lines = vec!["set -eu".into()];
    for dir in [
        "/Library",
        "/Library/PrivilegedHelperTools",
        RUNTIME_DIRECTORY,
        "/Library/LaunchDaemons",
    ] {
        let dir = shell_quote(dir);
        lines.push(format!("[ ! -L {dir} ] || exit 70"));
        lines.push(format!("if [ -e {dir} ]; then [ -d {dir} ] || exit 70; [ \"$(/usr/bin/stat -f %u {dir})\" = 0 ] || exit 71; {}; fi", directory_mode_check(&dir)));
        lines.push(format!("if [ -e {dir} ]; then fan_dir_acl=$(/bin/ls -lde {dir}) || exit 72; [ -z \"$(/usr/bin/printf '%s\\n' \"$fan_dir_acl\" | /usr/bin/tail -n +2)\" ] || exit 72; fi"));
    }
    for path in [TOOL_PATH, DAEMON_PATH] {
        let path = shell_quote(path);
        lines.push(format!("[ ! -L {path} ] || exit 73"));
        lines.push(format!("if [ -e {path} ]; then [ -f {path} ] || exit 73; [ \"$(/usr/bin/stat -f %u {path})\" = 0 ] || exit 74; fi"));
    }
    for path in [SOCKET_PATH, LEGACY_SOCKET_PATH] {
        let socket = shell_quote(path);
        lines.push(format!("[ ! -L {socket} ] || exit 73"));
        lines.push(format!("if [ -e {socket} ]; then [ -S {socket} ] || exit 73; [ \"$(/usr/bin/stat -f %u {socket})\" = 0 ] || exit 74; fi"));
    }
    lines.push(format!("if /bin/launchctl print system/{LABEL} >/dev/null 2>&1; then /bin/launchctl bootout system/{LABEL} || {{ echo 'helper stop failed; service files were preserved' >&2; exit 75; }}; fi"));
    lines.push(format!("attempt=0; while /bin/launchctl print system/{LABEL} >/dev/null 2>&1; do attempt=$((attempt + 1)); [ \"$attempt\" -lt 350 ] || exit 75; /bin/sleep 0.2; done"));
    // Only these exact paths and the empty runtime directory are removed.
    // Legacy cleanup only unlinks a confirmed root-owned socket after stop;
    // the group-writable legacy directory is never used for a new service.
    lines.push(checked_socket_removal(LEGACY_SOCKET_PATH));
    for path in [DAEMON_PATH, TOOL_PATH, SOCKET_PATH] {
        lines.push(format!("/bin/rm -f {}", shell_quote(path)));
    }
    let runtime = shell_quote(RUNTIME_DIRECTORY);
    lines.push(format!("if [ -d {runtime} ]; then /bin/rmdir {runtime} || {{ echo 'runtime directory is not empty; unexpected files were preserved' >&2; exit 76; }}; fi"));
    for path in [
        DAEMON_PATH,
        TOOL_PATH,
        SOCKET_PATH,
        LEGACY_SOCKET_PATH,
        RUNTIME_DIRECTORY,
    ] {
        let path = shell_quote(path);
        lines.push(format!("[ ! -e {path} ] && [ ! -L {path} ] || exit 76"));
    }
    lines.push(format!(
        "if /bin/launchctl print system/{LABEL} >/dev/null 2>&1; then exit 77; fi"
    ));
    lines.join("\n")
}
fn checked_socket_removal(path: &str) -> String {
    let socket = shell_quote(path);
    format!("[ ! -L {socket} ] || exit 73; if [ -e {socket} ]; then [ -S {socket} ] || exit 73; [ \"$(/usr/bin/stat -f %u {socket})\" = 0 ] || exit 74; /bin/rm -f {socket}; fi")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_handback_never_reaches_authorization_or_removal() {
        let result = uninstall_workflow(
            || HelperResponse::failed("mock SMC failure"),
            || panic!("removal must not run"),
            || Ok(false),
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("automatic hand-back could not be confirmed"));
    }
    #[test]
    fn removal_requires_verified_final_state_and_preserves_cancel_failure() {
        let reset = || HelperResponse {
            ok: true,
            message: "reset".into(),
            is_root: true,
            protocol_version: Some(crate::PROTOCOL_VERSION),
            actual_rpm: None,
        };
        let result = uninstall_workflow(
            reset,
            || Err(Error("mock administrator cancelled".into())),
            || Ok(false),
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("administrator cancelled"));
        let result = uninstall_workflow(reset, || Ok(()), || Ok(false));
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("uninstall incomplete"));
        let mut checks = 0;
        let result = uninstall_workflow(
            reset,
            || Ok(()),
            || {
                checks += 1;
                Ok(checks > 1)
            },
        )
        .unwrap();
        assert!(result.ok);
        assert!(!result.is_root);
        assert_eq!(checks, 2);
    }
    #[test]
    fn privileged_shell_script_parses_without_executing() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let script = install_script(Path::new(
            "/Applications/Fan Control.app/Contents/Library/LaunchServices/helper",
        ))
        .unwrap();
        let mut parser = Command::new("/bin/sh")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        parser
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = parser.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    fn directory_check_accepts_legacy_modes_and_rejects_shared_write() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("fan-mode-check-{}", std::process::id()));
        for (mode, accepted) in [
            (0o755, true),
            (0o744, true),
            (0o700, true),
            (0o555, true),
            (0o775, false),
            (0o757, false),
            (0o777, false),
        ] {
            let dir = root.join(format!("{mode:o}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
            let status = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(directory_mode_check(&shell_quote(dir.to_str().unwrap())))
                .status()
                .unwrap();
            assert_eq!(status.success(), accepted, "mode {mode:o}");
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn privileged_script_quotes_source_and_never_stages_in_tmp() {
        let script = install_script(Path::new(
            "/Applications/用户's Fan.app/Contents/Library/LaunchServices/helper",
        ))
        .unwrap();
        assert!(script.contains("用户'\\''s Fan.app"));
        assert!(!script.contains("/tmp"));
        assert!(script.contains("[ -L '/Library/Logs/FanControl' ]"));
        assert!(script.contains("/usr/bin/stat -f %u"));
        assert!(script.contains("/usr/bin/plutil -lint"));
        let verify = script.find("codesign --verify").unwrap();
        let bootout = script.find("launchctl bootout").unwrap();
        assert!(verify < bootout);
        assert!(script.find("FAN_CONTROL_RUST_PLIST").unwrap() < bootout);
    }
    #[test]
    fn apple_script_escaping_cannot_turn_path_into_code() {
        let script = "say \"x\" \\ test\nnext";
        assert_eq!(
            apple_script_string(script),
            "\"say \\\"x\\\" \\\\ test\nnext\""
        );
        assert!(install_script(Path::new("relative")).is_err());
        assert!(install_script(Path::new("/a\nmalicious")).is_err());
    }
    #[test]
    fn daemon_uses_root_owned_regular_log_and_standalone_helper() {
        let plist = launch_daemon_plist();
        assert!(plist.contains(LOG_PATH));
        assert!(!plist.contains("/tmp"));
        assert!(plist.contains(TOOL_PATH));
        assert!(plist.contains("--helper"));
        assert!(plist.contains("<key>ExitTimeOut</key><integer>60</integer>"));
    }
    #[test]
    fn uninstall_is_scoped_and_log_preserving_and_verifies_absence() {
        let script = uninstall_script();
        let removed: Vec<_> = script
            .lines()
            .filter(|line| line.starts_with("/bin/rm"))
            .collect();
        assert_eq!(
            removed,
            vec![
                format!("/bin/rm -f {}", shell_quote(DAEMON_PATH)),
                format!("/bin/rm -f {}", shell_quote(TOOL_PATH)),
                format!("/bin/rm -f {}", shell_quote(SOCKET_PATH))
            ]
        );
        assert!(!script.contains(LOG_DIRECTORY));
        assert!(!script.contains("rm -r"));
        assert!(!script.contains("install -d"));
        assert!(script.find("launchctl bootout").unwrap() < script.find("/bin/rm").unwrap());
        assert!(script.contains("exit 76"));
        assert!(script.contains("exit 77"));
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut parser = Command::new("/bin/sh")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        parser
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = parser.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    fn failed_bootout_is_explicit_before_any_tool_or_socket_replacement() {
        let script = install_script(Path::new("/Applications/Fan.app/helper")).unwrap();
        let bootout = script.find("launchctl bootout").unwrap();
        let preservation = script
            .find("helper stop failed; installed helper was preserved")
            .unwrap();
        let replace = script.find("/bin/mv -f").unwrap();
        let socket_remove = script
            .find(&format!("/bin/rm -f {}", shell_quote(SOCKET_PATH)))
            .unwrap();
        assert!(bootout < preservation && preservation < socket_remove && socket_remove < replace);
        assert!(!script.contains(&format!("/bin/rm -f {}", shell_quote(LOG_PATH))));
        assert!(!script.contains("bootout system '/Library/LaunchDaemons/com.local.fan-control.helper.plist' >/dev/null 2>&1 || true"));
        assert!(uninstall_script().contains("helper stop failed; service files were preserved"));
    }
    #[test]
    fn secure_runtime_migration_only_unlinks_legacy_socket_after_stop() {
        let script = install_script(Path::new("/Applications/Fan.app/helper")).unwrap();
        assert!(script.contains(&format!(
            "install -d -m 755 -o root -g wheel {}",
            shell_quote(RUNTIME_DIRECTORY)
        )));
        assert!(script.contains("fan_dir_acl=$(/bin/ls -lde"));
        let stopped = script
            .find("helper did not stop; installed helper was preserved")
            .unwrap();
        let old_cleanup = script
            .find(&checked_socket_removal(LEGACY_SOCKET_PATH))
            .unwrap();
        assert!(stopped < old_cleanup && old_cleanup < script.find("/bin/mv -f").unwrap());
        let removal = uninstall_script();
        assert!(removal.contains(&format!("/bin/rmdir {}", shell_quote(RUNTIME_DIRECTORY))));
        assert!(removal.contains("unexpected files were preserved"));
        assert!(!removal.contains("rm -r"));
        assert!(!removal.contains("/bin/rmdir '/Library/PrivilegedHelperTools'"));
    }
}
