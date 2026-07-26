import Foundation
import Darwin

enum FanHelperConstants {
    static let label = "com.local.fan-control.helper"
    static let protocolVersion = 2
    static let socketPath = "/var/run/com.local.fan-control.helper.sock"
    static let helperToolPath = "/Library/PrivilegedHelperTools/\(label)"
    static let launchDaemonPath = "/Library/LaunchDaemons/\(label).plist"
}

struct FanHelperRequest: Codable {
    enum Command: String, Codable {
        case status
        case setFanMode
        case setFanRPM
        case resetAll
    }

    var command: Command
    var fanId: Int?
    var mode: FanMode?
    var rpm: Int?
}

struct FanHelperResponse: Codable {
    var ok: Bool
    var message: String
    var isRoot: Bool
    var protocolVersion: Int? = nil
}

enum FanControlHelperClient {
    static func status(timeout: TimeInterval = 1.0) -> FanHelperResponse {
        send(FanHelperRequest(command: .status), timeout: timeout)
    }

    static func setFanMode(_ fanId: Int, mode: FanMode) -> FanHelperResponse {
        send(FanHelperRequest(command: .setFanMode, fanId: fanId, mode: mode))
    }

    static func setFanRPM(_ fanId: Int, rpm: Int) -> FanHelperResponse {
        send(FanHelperRequest(command: .setFanRPM, fanId: fanId, rpm: rpm))
    }

    static func resetAll() -> FanHelperResponse {
        send(FanHelperRequest(command: .resetAll))
    }

    private static func send(_ request: FanHelperRequest, timeout: TimeInterval = 15.0) -> FanHelperResponse {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            return FanHelperResponse(ok: false, message: "socket failed", isRoot: false)
        }
        defer { close(fd) }

        var tv = timeval(tv_sec: Int(timeout), tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))

        let connected = withUnixSocketAddress(path: FanHelperConstants.socketPath) { addr, len in
            connect(fd, addr, len)
        }
        guard connected == 0 else {
            return FanHelperResponse(ok: false, message: "helper unavailable", isRoot: false)
        }

        do {
            var payload = try JSONEncoder().encode(request)
            payload.append(0x0a)
            let wrote = payload.withUnsafeBytes { buffer -> Int in
                guard let base = buffer.baseAddress else { return -1 }
                return Darwin.write(fd, base, buffer.count)
            }
            guard wrote == payload.count else {
                return FanHelperResponse(ok: false, message: "write failed", isRoot: false)
            }

            let data = readLineData(from: fd)
            guard !data.isEmpty else {
                return FanHelperResponse(ok: false, message: "empty helper response", isRoot: false)
            }
            return try JSONDecoder().decode(FanHelperResponse.self, from: data)
        } catch {
            return FanHelperResponse(ok: false, message: error.localizedDescription, isRoot: false)
        }
    }

    static func readLineData(from fd: Int32) -> Data {
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 1024)

        while true {
            let count = buffer.withUnsafeMutableBytes { raw -> Int in
                guard let base = raw.baseAddress else { return -1 }
                return Darwin.read(fd, base, raw.count)
            }
            if count <= 0 { break }
            if let newlineIndex = buffer[..<count].firstIndex(of: 0x0a) {
                data.append(buffer, count: newlineIndex)
                break
            }
            data.append(buffer, count: count)
            if data.count > 4096 { break }
        }
        return data
    }
}

enum FanControlWriter {
    static func setFanMode(_ fanId: Int, mode: FanMode, completion: (() -> Void)? = nil) {
        if geteuid() == 0 {
            SMCKit.shared.performAsync { smc in
                smc.setFanMode(fanId, mode: mode)
                DispatchQueue.main.async { completion?() }
            }
            return
        }

        DispatchQueue.global(qos: .utility).async {
            debugLog("[FanControl] helperClient.setFanMode fan=\(fanId) mode=\(mode)")
            let response = FanControlHelperClient.setFanMode(fanId, mode: mode)
            debugLog("[FanControl] helperClient.setFanMode fan=\(fanId) ok=\(response.ok) message=\(response.message)")
            DispatchQueue.main.async { completion?() }
        }
    }

    static func setFanRPM(_ fanId: Int, rpm: Int, completion: (() -> Void)? = nil) {
        if geteuid() == 0 {
            SMCKit.shared.performAsync { smc in
                smc.setFanSpeed(fanId, speed: rpm)
                DispatchQueue.main.async { completion?() }
            }
            return
        }

        DispatchQueue.global(qos: .utility).async {
            debugLog("[FanControl] helperClient.setFanRPM fan=\(fanId) rpm=\(rpm)")
            let response = FanControlHelperClient.setFanRPM(fanId, rpm: rpm)
            debugLog("[FanControl] helperClient.setFanRPM fan=\(fanId) rpm=\(rpm) ok=\(response.ok) message=\(response.message)")
            DispatchQueue.main.async { completion?() }
        }
    }

    static func resetAll(completion: (() -> Void)? = nil) {
        if geteuid() == 0 {
            SMCKit.shared.performAsync { smc in
                _ = smc.resetFanControl()
                DispatchQueue.main.async { completion?() }
            }
            return
        }

        DispatchQueue.global(qos: .utility).async {
            debugLog("[FanControl] helperClient.resetAll")
            let response = FanControlHelperClient.resetAll()
            debugLog("[FanControl] helperClient.resetAll ok=\(response.ok) message=\(response.message)")
            DispatchQueue.main.async { completion?() }
        }
    }
}

enum FanControlHelperDaemon {
    static func run() -> Never {
        debugLog("[FanControlHelper] starting euid=\(geteuid()) socket=\(FanHelperConstants.socketPath)")
        guard geteuid() == 0 else {
            debugLog("[FanControlHelper] refusing non-root launch")
            exit(1)
        }

        signal(SIGTERM) { _ in
            _ = SMCKit.shared.resetFanControl()
            unlink(FanHelperConstants.socketPath)
            exit(0)
        }
        signal(SIGPIPE, SIG_IGN)
        signal(SIGINT) { _ in
            _ = SMCKit.shared.resetFanControl()
            unlink(FanHelperConstants.socketPath)
            exit(0)
        }

        unlink(FanHelperConstants.socketPath)

        let serverFD = socket(AF_UNIX, SOCK_STREAM, 0)
        guard serverFD >= 0 else {
            debugLog("[FanControlHelper] socket failed")
            exit(1)
        }

        let bound = withUnixSocketAddress(path: FanHelperConstants.socketPath) { addr, len in
            bind(serverFD, addr, len)
        }
        guard bound == 0 else {
            debugLog("[FanControlHelper] bind failed errno=\(errno)")
            close(serverFD)
            exit(1)
        }

        chmod(FanHelperConstants.socketPath, S_IRUSR | S_IWUSR | S_IRGRP | S_IWGRP | S_IROTH | S_IWOTH)

        guard listen(serverFD, 16) == 0 else {
            debugLog("[FanControlHelper] listen failed errno=\(errno)")
            close(serverFD)
            exit(1)
        }

        while true {
            let clientFD = accept(serverFD, nil, nil)
            guard clientFD >= 0 else { continue }
            handle(clientFD: clientFD)
            close(clientFD)
        }
    }

    private static func handle(clientFD: Int32) {
        guard isAuthorizedPeer(clientFD) else {
            debugLog("[FanControlHelper] reject unauthorized peer")
            writeResponse(FanHelperResponse(ok: false, message: "unauthorized peer", isRoot: true), to: clientFD)
            return
        }

        let data = FanControlHelperClient.readLineData(from: clientFD)
        guard let request = try? JSONDecoder().decode(FanHelperRequest.self, from: data) else {
            writeResponse(FanHelperResponse(ok: false, message: "bad request", isRoot: true), to: clientFD)
            return
        }

        debugLog("[FanControlHelper] request command=\(request.command.rawValue) fan=\(request.fanId ?? -1) rpm=\(request.rpm ?? -1)")

        switch request.command {
        case .status:
            writeResponse(
                FanHelperResponse(
                    ok: true,
                    message: "ready",
                    isRoot: true,
                    protocolVersion: FanHelperConstants.protocolVersion
                ),
                to: clientFD
            )
        case .setFanMode:
            guard let fanId = request.fanId, let mode = request.mode else {
                writeResponse(FanHelperResponse(ok: false, message: "missing fan mode", isRoot: true), to: clientFD)
                return
            }
            SMCKit.shared.performSync { $0.setFanMode(fanId, mode: mode) }
            writeResponse(FanHelperResponse(ok: true, message: "mode set", isRoot: true), to: clientFD)
        case .setFanRPM:
            guard let fanId = request.fanId, let rpm = request.rpm else {
                writeResponse(FanHelperResponse(ok: false, message: "missing rpm", isRoot: true), to: clientFD)
                return
            }
            SMCKit.shared.performSync { $0.setFanSpeed(fanId, speed: rpm) }
            writeResponse(FanHelperResponse(ok: true, message: "rpm set", isRoot: true), to: clientFD)
        case .resetAll:
            let ok = SMCKit.shared.performSync { $0.resetFanControl() }
            writeResponse(FanHelperResponse(ok: ok, message: ok ? "reset" : "reset failed", isRoot: true), to: clientFD)
        }
    }

    private static func isAuthorizedPeer(_ fd: Int32) -> Bool {
        var peerUID = uid_t()
        var peerGID = gid_t()
        guard getpeereid(fd, &peerUID, &peerGID) == 0 else { return false }
        if peerUID == 0 { return true }

        var consoleStat = stat()
        guard stat("/dev/console", &consoleStat) == 0 else { return false }
        return peerUID == consoleStat.st_uid
    }

    private static func writeResponse(_ response: FanHelperResponse, to fd: Int32) {
        guard var data = try? JSONEncoder().encode(response) else { return }
        data.append(0x0a)
        data.withUnsafeBytes { buffer in
            guard let base = buffer.baseAddress else { return }
            _ = Darwin.write(fd, base, buffer.count)
        }
    }
}

@discardableResult
private func withUnixSocketAddress<T>(path: String, _ body: (UnsafePointer<sockaddr>, socklen_t) -> T) -> T {
    var addr = sockaddr_un()
    addr.sun_family = sa_family_t(AF_UNIX)

    let maxPathLength = MemoryLayout.size(ofValue: addr.sun_path)
    path.withCString { pathPtr in
        withUnsafeMutablePointer(to: &addr.sun_path) { sunPathPtr in
            sunPathPtr.withMemoryRebound(to: CChar.self, capacity: maxPathLength) { rebound in
                memset(rebound, 0, maxPathLength)
                strncpy(rebound, pathPtr, maxPathLength - 1)
            }
        }
    }

    return withUnsafePointer(to: &addr) { ptr in
        ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPtr in
            body(sockaddrPtr, socklen_t(MemoryLayout<sockaddr_un>.size))
        }
    }
}
