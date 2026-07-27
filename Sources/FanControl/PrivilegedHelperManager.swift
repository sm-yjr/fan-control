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
            <string>/tmp/fan-control-helper.log</string>
            <key>StandardErrorPath</key>
            <string>/tmp/fan-control-helper.log</string>
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
        let plistURL = URL(fileURLWithPath: "/tmp/\(FanHelperConstants.label).plist")

        do {
            try plist.write(to: plistURL, atomically: true, encoding: .utf8)
        } catch {
            return FanHelperResponse(ok: false, message: error.localizedDescription, isRoot: false)
        }

        let stagedHelperPath = "\(FanHelperConstants.helperToolPath).installing"
        let script = ([
            "set -e",
            "/usr/bin/install -d -m 755 -o root -g wheel /Library/PrivilegedHelperTools",
            "/bin/rm -f \(shellQuote(stagedHelperPath))",
            "/usr/bin/install -m 755 -o root -g wheel \(shellQuote(bundledHelperPath)) \(shellQuote(stagedHelperPath))",
            "/usr/bin/codesign --verify --strict \(shellQuote(stagedHelperPath))",
            "/usr/bin/xattr -d com.apple.quarantine \(shellQuote(stagedHelperPath)) >/dev/null 2>&1 || true",
            "/usr/bin/install -m 644 -o root -g wheel \(shellQuote(plistURL.path)) \(shellQuote(FanHelperConstants.launchDaemonPath))",
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
