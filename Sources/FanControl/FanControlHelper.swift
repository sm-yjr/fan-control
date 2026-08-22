import Foundation
import Darwin

enum FanHelperConstants {
    static let label = "com.local.fan-control.helper"
    // Version 4: setFanMode/setFanRPM responses now report the real SMC
    // write result instead of an unconditional ok:true, and switching to
    // automatic verifies the mode register before clearing the fan target.
    // Installed helpers must be replaced, so the version bump is required.
    static let protocolVersion = 4
    static let socketPath = "/var/run/com.local.fan-control.helper.sock"
    static let helperToolPath = "/Library/PrivilegedHelperTools/\(label)"
    static let launchDaemonPath = "/Library/LaunchDaemons/\(label).plist"
    // Daemon stdout/stderr must never point at world-writable, predictable
    // paths such as /tmp: any local account could pre-plant a symlink or
    // FIFO there and have launchd open it as root. The installer creates
    // this directory and file as root-owned regular files instead.
    static let helperLogDirectory = "/Library/Logs/FanControl"
    static let helperLogPath = "/Library/Logs/FanControl/helper.log"
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
            let connectError = errno
            let description = String(cString: strerror(connectError))
            return FanHelperResponse(
                ok: false,
                message: "helper unavailable (\(connectError): \(description))",
                isRoot: false
            )
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
    /// Every helper write request is issued from this single serial queue so
    /// the helper receives requests in issue order. A hand-back's
    /// automatic/reset is always enqueued after the last RPM write, and the
    /// generation counter stops completions from enqueued writes from
    /// scheduling new ones — together that guarantees no stale RPM request
    /// can land after a reset and re-force a fan. In root mode the SMC queue
    /// provides the same serialization.
    static let requestQueue = DispatchQueue(label: "FanControl.HelperRequests")

    /// The completion receives whether the write actually landed: the SMC
    /// result in root mode, or the helper's response otherwise. Callers must
    /// not treat a failed write as committed state.
    ///
    /// Completions run on `completionQueue`. UI state updates keep the
    /// default main queue; the termination hand-back supplies its own queue
    /// because it waits while the main thread is blocked.
    static func setFanMode(
        _ fanId: Int,
        mode: FanMode,
        completionQueue: DispatchQueue = .main,
        completion: ((Bool) -> Void)? = nil
    ) {
        if geteuid() == 0 {
            SMCKit.shared.performAsync { smc in
                let ok = smc.setFanMode(fanId, mode: mode)
                completionQueue.async { completion?(ok) }
            }
            return
        }

        requestQueue.async {
            debugLog("[FanControl] helperClient.setFanMode fan=\(fanId) mode=\(mode)")
            let response = FanControlHelperClient.setFanMode(fanId, mode: mode)
            debugLog("[FanControl] helperClient.setFanMode fan=\(fanId) ok=\(response.ok) message=\(response.message)")
            completionQueue.async { completion?(response.ok) }
        }
    }

    static func setFanRPM(
        _ fanId: Int,
        rpm: Int,
        completionQueue: DispatchQueue = .main,
        completion: ((Bool) -> Void)? = nil
    ) {
        if geteuid() == 0 {
            SMCKit.shared.performAsync { smc in
                let ok = smc.setFanSpeed(fanId, speed: rpm)
                completionQueue.async { completion?(ok) }
            }
            return
        }

        requestQueue.async {
            debugLog("[FanControl] helperClient.setFanRPM fan=\(fanId) rpm=\(rpm)")
            let response = FanControlHelperClient.setFanRPM(fanId, rpm: rpm)
            debugLog("[FanControl] helperClient.setFanRPM fan=\(fanId) rpm=\(rpm) ok=\(response.ok) message=\(response.message)")
            completionQueue.async { completion?(response.ok) }
        }
    }

    static func resetAll(
        completionQueue: DispatchQueue = .main,
        completion: ((Bool) -> Void)? = nil
    ) {
        if geteuid() == 0 {
            SMCKit.shared.performAsync { smc in
                let ok = smc.resetFanControl()
                completionQueue.async { completion?(ok) }
            }
            return
        }

        requestQueue.async {
            debugLog("[FanControl] helperClient.resetAll")
            let response = FanControlHelperClient.resetAll()
            debugLog("[FanControl] helperClient.resetAll ok=\(response.ok) message=\(response.message)")
            completionQueue.async { completion?(response.ok) }
        }
    }
}

enum FanControlHelperDaemon {
    private static let shutdownQueue = DispatchQueue(label: "FanControlHelper.Shutdown")
    private static var signalSources: [DispatchSourceSignal] = []
    // The accept loop, request handlers, and signal path run on different
    // threads; the shutdown flag is guarded by a lock, never read raw.
    private static let stateLock = NSLock()
    private static var shutdownRequestedState = false

    private static func markShutdownRequested() -> Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        if shutdownRequestedState { return false }
        shutdownRequestedState = true
        return true
    }

    static var isShutdownRequested: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return shutdownRequestedState
    }

    static func run() -> Never {
        debugLog("[FanControlHelper] starting euid=\(geteuid()) socket=\(FanHelperConstants.socketPath)")
        guard geteuid() == 0 else {
            debugLog("[FanControlHelper] refusing non-root launch")
            exit(1)
        }

        signal(SIGPIPE, SIG_IGN)
        installSignalSources()

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
            // A connected-but-silent peer must not wedge the single-threaded
            // accept loop forever; bound the read so handle() always returns.
            var receiveTimeout = timeval(tv_sec: 10, tv_usec: 0)
            setsockopt(
                clientFD,
                SOL_SOCKET,
                SO_RCVTIMEO,
                &receiveTimeout,
                socklen_t(MemoryLayout<timeval>.size)
            )
            autoreleasepool {
                handle(clientFD: clientFD)
            }
            close(clientFD)
        }
    }

    private static func installSignalSources() {
        // Ignore the default dispositions first: a POSIX signal handler must
        // not run Swift runtime, IOKit, socket, or logging code. The work is
        // deferred to a normal dispatch queue via DispatchSourceSignal.
        signal(SIGTERM, SIG_IGN)
        signal(SIGINT, SIG_IGN)
        for signalNumber in [SIGTERM, SIGINT] {
            let source = DispatchSource.makeSignalSource(signal: signalNumber, queue: shutdownQueue)
            source.setEventHandler {
                performShutdown()
            }
            source.resume()
            signalSources.append(source)
        }
    }

    private static func performShutdown() {
        guard markShutdownRequested() else { return }
        debugLog("[FanControlHelper] signal received, resetting fan control before exit")
        // Remove the socket before resetting so no new client can connect.
        // Requests already in flight are ordered against the reset by the
        // serial SMC queue: a write that started first lands before the
        // reset runs, and every request handled after the flag is set is
        // refused inside its SMC closure.
        unlink(FanHelperConstants.socketPath)
        let ok = SMCKit.shared.performSync { $0.resetFanControl() }
        debugLog("[FanControlHelper] shutdown reset result=\(ok)")
        exit(0)
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
            if isShutdownRequested {
                writeResponse(
                    FanHelperResponse(
                        ok: false,
                        message: "shutting down",
                        isRoot: true,
                        protocolVersion: FanHelperConstants.protocolVersion
                    ),
                    to: clientFD
                )
            } else {
                writeResponse(
                    FanHelperResponse(
                        ok: true,
                        message: "ready",
                        isRoot: true,
                        protocolVersion: FanHelperConstants.protocolVersion
                    ),
                    to: clientFD
                )
            }
        case .setFanMode:
            guard let fanId = request.fanId, let mode = request.mode else {
                writeResponse(FanHelperResponse(ok: false, message: "missing fan mode", isRoot: true), to: clientFD)
                return
            }
            let modeOK = SMCKit.shared.performSync { smc -> Bool in
                // Checked inside the serial SMC closure so the ordering is
                // definitive: once the shutdown reset is queued, every later
                // mode write is refused instead of re-forcing a fan.
                guard !isShutdownRequested else {
                    debugLog("[FanControlHelper] reject setFanMode during shutdown")
                    return false
                }
                return smc.setFanMode(fanId, mode: mode)
            }
            writeResponse(
                FanHelperResponse(
                    ok: modeOK,
                    message: modeOK ? "mode set" : (isShutdownRequested ? "shutting down" : "mode change failed"),
                    isRoot: true
                ),
                to: clientFD
            )
        case .setFanRPM:
            guard let fanId = request.fanId, let rpm = request.rpm else {
                writeResponse(FanHelperResponse(ok: false, message: "missing rpm", isRoot: true), to: clientFD)
                return
            }
            let rpmOK = SMCKit.shared.performSync { smc -> Bool in
                guard !isShutdownRequested else {
                    debugLog("[FanControlHelper] reject setFanRPM during shutdown")
                    return false
                }
                return smc.setFanSpeed(fanId, speed: rpm)
            }
            writeResponse(
                FanHelperResponse(
                    ok: rpmOK,
                    message: rpmOK ? "rpm set" : (isShutdownRequested ? "shutting down" : "rpm write failed"),
                    isRoot: true
                ),
                to: clientFD
            )
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
