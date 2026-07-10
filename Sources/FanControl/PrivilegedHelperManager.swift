import Foundation

enum PrivilegedHelperManager {
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
        guard let executablePath = Bundle.main.executablePath else {
            return FanHelperResponse(ok: false, message: "missing executable path", isRoot: false)
        }

        let plist = makeLaunchDaemonPlist()
        let plistURL = URL(fileURLWithPath: "/tmp/\(FanHelperConstants.label).plist")

        do {
            try plist.write(to: plistURL, atomically: true, encoding: .utf8)
        } catch {
            return FanHelperResponse(ok: false, message: error.localizedDescription, isRoot: false)
        }

        let script = ([
            "set -e",
            "/usr/bin/install -d -m 755 -o root -g wheel /Library/PrivilegedHelperTools",
            "/usr/bin/install -m 755 -o root -g wheel \(shellQuote(executablePath)) \(shellQuote(FanHelperConstants.helperToolPath))",
            "/usr/bin/install -m 644 -o root -g wheel \(shellQuote(plistURL.path)) \(shellQuote(FanHelperConstants.launchDaemonPath))",
            "/bin/launchctl bootout system \(shellQuote(FanHelperConstants.launchDaemonPath)) >/dev/null 2>&1 || true",
            "/bin/launchctl bootstrap system \(shellQuote(FanHelperConstants.launchDaemonPath))",
            "/bin/launchctl enable system/\(FanHelperConstants.label)"
        ]).joined(separator: "; ")

        debugLog("[FanControl] installHelper start executable=\(executablePath)")
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

        Thread.sleep(forTimeInterval: 0.3)
        let status = FanControlHelperClient.status(timeout: 2.0)
        debugLog("[FanControl] installHelper status ok=\(status.ok) message=\(status.message)")
        return status.ok ? status : FanHelperResponse(ok: false, message: "installed but helper did not respond: \(status.message)", isRoot: false)
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
