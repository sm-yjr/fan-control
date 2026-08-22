import Foundation

enum PrivilegedHelperManager {
    private static let helperStartupTimeout: TimeInterval = 10
    private static let helperStatusPollInterval: TimeInterval = 0.2

    static func makeLaunchDaemonPlist() -> String {
        """
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0">
        <dict>
            <key>Label</key>
            <string>\(FanHelperConstants.label)</string>
            <key>ProgramArguments</key>
            <array>
                <string>\(xmlEscape(FanHelperConstants.helperToolPath))</string>
                <string>--helper</string>
            </array>
            <key>RunAtLoad</key>
            <true/>
            <key>KeepAlive</key>
            <true/>
            <key>StandardOutPath</key>
            <string>\(xmlEscape(FanHelperConstants.helperLogPath))</string>
            <key>StandardErrorPath</key>
            <string>\(xmlEscape(FanHelperConstants.helperLogPath))</string>
        </dict>
        </plist>
        """
    }

    static func installCurrentAppHelper() -> FanHelperResponse {
        let bundledHelperURL = Bundle.main.bundleURL
            .appendingPathComponent("Contents", isDirectory: true)
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("LaunchServices", isDirectory: true)
            .appendingPathComponent(FanHelperConstants.label, isDirectory: false)
        let bundledHelperPath = bundledHelperURL.path

        guard FileManager.default.isExecutableFile(atPath: bundledHelperPath) else {
            return FanHelperResponse(
                ok: false,
                message: "bundled privileged helper is missing",
                isRoot: false
            )
        }

        let plist = makeLaunchDaemonPlist()

        // The daemon plist is written by the privileged script itself through
        // a heredoc, directly into /Library/LaunchDaemons. Nothing security
        // relevant is staged in world-writable /tmp anymore, and the root
        // script never follows an attacker-planted file: both target
        // directories are root-owned, so non-root accounts cannot plant
        // symlinks inside them.
        let plistMarker = "FAN_CONTROL_HELPER_PLIST"
        let installPlist =
            "/bin/cat > \(shellQuote(FanHelperConstants.launchDaemonPath)) <<'\(plistMarker)'\n"
            + "\(plist)\n"
            + "\(plistMarker)\n"
            + "/usr/sbin/chown root:wheel \(shellQuote(FanHelperConstants.launchDaemonPath))\n"
            + "/bin/chmod 644 \(shellQuote(FanHelperConstants.launchDaemonPath))"

        let stagedHelperPath = "\(FanHelperConstants.helperToolPath).installing"
        let script = ([
            "set -e",
            "/usr/bin/install -d -m 755 -o root -g wheel /Library/PrivilegedHelperTools",
            // Root-owned log location: launchd opens StandardOutPath as root
            // on every daemon start, so it must be a regular file in a
            // directory only root can write.
            "/usr/bin/install -d -m 755 -o root -g wheel \(shellQuote(FanHelperConstants.helperLogDirectory))",
            "/bin/rm -f \(shellQuote(FanHelperConstants.helperLogPath))",
            // Remove the predictable /tmp log left behind by older installs.
            "/bin/rm -f /tmp/fan-control-helper.log",
            "/usr/bin/install -m 640 -o root -g wheel /dev/null \(shellQuote(FanHelperConstants.helperLogPath))",
            "/bin/rm -f \(shellQuote(stagedHelperPath))",
            "/usr/bin/install -m 755 -o root -g wheel \(shellQuote(bundledHelperPath)) \(shellQuote(stagedHelperPath))",
            "/usr/bin/codesign --verify --strict \(shellQuote(stagedHelperPath))",
            "/usr/bin/xattr -d com.apple.quarantine \(shellQuote(stagedHelperPath)) >/dev/null 2>&1 || true",
            installPlist,
            "/bin/launchctl bootout system \(shellQuote(FanHelperConstants.launchDaemonPath)) >/dev/null 2>&1 || true",
            "/bin/mv -f \(shellQuote(stagedHelperPath)) \(shellQuote(FanHelperConstants.helperToolPath))",
            "/bin/rm -f \(shellQuote(FanHelperConstants.socketPath))",
            "/bin/launchctl bootstrap system \(shellQuote(FanHelperConstants.launchDaemonPath))",
            "/bin/launchctl enable system/\(FanHelperConstants.label)"
        ]).joined(separator: "; ")

        debugLog("[FanControl] installHelper start executable=\(bundledHelperPath)")
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        process.arguments = [
            "-e",
            "do shell script \(appleScriptString(script)) with administrator privileges"
        ]

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        do {
            try process.run()
            process.waitUntilExit()
            let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            guard process.terminationStatus == 0 else {
                debugLog("[FanControl] installHelper failed status=\(process.terminationStatus) output=\(output)")
                return FanHelperResponse(ok: false, message: output.isEmpty ? "install cancelled or failed" : output, isRoot: false)
            }
        } catch {
            debugLog("[FanControl] installHelper failed error=\(error.localizedDescription)")
            return FanHelperResponse(ok: false, message: error.localizedDescription, isRoot: false)
        }

        let status = waitForCompatibleHelper()
        debugLog("[FanControl] installHelper status ok=\(status.ok) message=\(status.message)")
        guard status.ok else {
            return FanHelperResponse(
                ok: false,
                message: "installed but helper did not become ready: \(status.message)",
                isRoot: false
            )
        }
        guard status.protocolVersion == FanHelperConstants.protocolVersion else {
            return FanHelperResponse(
                ok: false,
                message: "installed helper version could not be verified",
                isRoot: false
            )
        }
        return status
    }

    private static func waitForCompatibleHelper() -> FanHelperResponse {
        let deadline = ProcessInfo.processInfo.systemUptime + helperStartupTimeout
        var lastStatus = FanHelperResponse(
            ok: false,
            message: "helper unavailable",
            isRoot: false
        )

        repeat {
            let status = FanControlHelperClient.status(timeout: 1.0)
            lastStatus = status
            if status.ok,
               status.protocolVersion == FanHelperConstants.protocolVersion {
                return status
            }

            let remaining = deadline - ProcessInfo.processInfo.systemUptime
            if remaining <= 0 {
                break
            }
            Thread.sleep(forTimeInterval: min(helperStatusPollInterval, remaining))
        } while true

        return lastStatus
    }

    private static func shellQuote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    private static func appleScriptString(_ value: String) -> String {
        "\"" + value
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"") + "\""
    }

    private static func xmlEscape(_ value: String) -> String {
        value
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "'", with: "&apos;")
    }
}
