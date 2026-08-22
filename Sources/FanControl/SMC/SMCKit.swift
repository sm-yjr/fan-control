import Foundation
import IOKit

enum SMCDataType: String {
    case UI8 = "ui8 "
    case UI16 = "ui16"
    case UI32 = "ui32"
    case SP1E = "sp1e"
    case SP3C = "sp3c"
    case SP4B = "sp4b"
    case SP5A = "sp5a"
    case SPA5 = "spa5"
    case SP69 = "sp69"
    case SP78 = "sp78"
    case SP87 = "sp87"
    case SP96 = "sp96"
    case SPB4 = "spb4"
    case SPF0 = "spf0"
    case FLT = "flt "
    case FPE2 = "fpe2"
    case FP2E = "fp2e"
    case FDS = "{fds"
}

enum SMCSelector: UInt8 {
    case kernelIndex = 2
    case readBytes = 5
    case writeBytes = 6
    case readIndex = 8
    case readKeyInfo = 9
}

enum FanMode: Int, Codable {
    case automatic = 0
    case forced = 1

    var label: String {
        switch self {
        case .automatic: "Auto"
        case .forced: "Manual"
        }
    }
}

struct SMCKeyData {
    typealias Bytes = (UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8,
                       UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8,
                       UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8,
                       UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8,
                       UInt8, UInt8, UInt8, UInt8)

    struct Version {
        var major: CUnsignedChar = 0
        var minor: CUnsignedChar = 0
        var build: CUnsignedChar = 0
        var reserved: CUnsignedChar = 0
        var release: CUnsignedShort = 0
    }

    struct LimitData {
        var version: UInt16 = 0
        var length: UInt16 = 0
        var cpuPLimit: UInt32 = 0
        var gpuPLimit: UInt32 = 0
        var memPLimit: UInt32 = 0
    }

    struct KeyInfo {
        var dataSize: IOByteCount32 = 0
        var dataType: UInt32 = 0
        var dataAttributes: UInt8 = 0
    }

    var key: UInt32 = 0
    var vers = Version()
    var pLimitData = LimitData()
    var keyInfo = KeyInfo()
    var padding: UInt16 = 0
    var result: UInt8 = 0
    var status: UInt8 = 0
    var data8: UInt8 = 0
    var data32: UInt32 = 0
    var bytes: Bytes = (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

struct SMCValue {
    var key: String
    var dataSize: UInt32 = 0
    var dataType: String = ""
    var bytes: [UInt8] = Array(repeating: 0, count: 32)

    init(_ key: String) {
        self.key = key
    }
}

// MARK: - Extensions

extension FourCharCode {
    init(fromString str: String) {
        precondition(str.count == 4)
        self = str.utf8.reduce(0) { sum, character in
            sum << 8 | UInt32(character)
        }
    }

    func toString() -> String {
        String(describing: UnicodeScalar(self >> 24 & 0xff)!) +
        String(describing: UnicodeScalar(self >> 16 & 0xff)!) +
        String(describing: UnicodeScalar(self >> 8  & 0xff)!) +
        String(describing: UnicodeScalar(self       & 0xff)!)
    }
}

extension Float {
    init?(_ bytes: [UInt8]) {
        self = bytes.withUnsafeBytes {
            $0.load(fromByteOffset: 0, as: Self.self)
        }
    }

    var asBytes: [UInt8] {
        withUnsafeBytes(of: self, Array.init)
    }
}

// MARK: - SMC Client

final class SMCKit {
    private struct CachedKeyInfo {
        let dataSize: UInt32
        let dataType: String
    }

    static let shared = SMCKit()
    private var conn: io_connect_t = 0
    private var keyInfoCache: [String: CachedKeyInfo] = [:]
    private var fanModeKeyIsLower: Bool?
    private var forcedModeFans: Set<Int> = []
    private var lastUnlockAttemptAt: [Int: Date] = [:]
    private let unlockRetryCooldown: TimeInterval = 30
    private let queueKey = DispatchSpecificKey<Void>()
    private let queue = DispatchQueue(label: "FanControl.SMC", qos: .utility)

    init() {
        queue.setSpecific(key: queueKey, value: ())

        var iterator: io_iterator_t = 0
        let matching: CFMutableDictionary = IOServiceMatching("AppleSMC")
        var result = IOServiceGetMatchingServices(kIOMainPortDefault, matching, &iterator)
        guard result == kIOReturnSuccess else { return }

        let device = IOIteratorNext(iterator)
        IOObjectRelease(iterator)
        guard device != 0 else { return }

        result = IOServiceOpen(device, mach_task_self_, 0, &conn)
        IOObjectRelease(device)
    }

    deinit {
        IOServiceClose(conn)
    }

    func performAsync(_ work: @escaping (SMCKit) -> Void) {
        queue.async { work(self) }
    }

    func performSync<T>(_ work: (SMCKit) -> T) -> T {
        if DispatchQueue.getSpecific(key: queueKey) != nil {
            return work(self)
        }
        return queue.sync { work(self) }
    }

    // MARK: - Read

    func getValue(_ key: String) -> Double? {
        var val = SMCValue(key)
        guard read(&val) == kIOReturnSuccess else { return nil }
        guard val.dataSize > 0 else { return nil }
        if val.bytes.allSatisfy({ $0 == 0 }) && key != "FS! " && !key.hasSuffix("md") && !key.hasSuffix("Md") {
            return nil
        }

        switch val.dataType {
        case SMCDataType.UI8.rawValue:
            return Double(val.bytes[0])
        case SMCDataType.UI16.rawValue:
            return Double(UInt16(val.bytes[0]) << 8 | UInt16(val.bytes[1]))
        case SMCDataType.UI32.rawValue:
            return Double(UInt32(val.bytes[0]) << 24 | UInt32(val.bytes[1]) << 16 | UInt32(val.bytes[2]) << 8 | UInt32(val.bytes[3]))
        case SMCDataType.SP1E.rawValue:
            return Double(UInt16(val.bytes[0]) << 8 | UInt16(val.bytes[1])) / 16384
        case SMCDataType.SP3C.rawValue:
            return Double(UInt16(val.bytes[0]) << 8 | UInt16(val.bytes[1])) / 4096
        case SMCDataType.SP4B.rawValue:
            return Double(UInt16(val.bytes[0]) << 8 | UInt16(val.bytes[1])) / 2048
        case SMCDataType.SP5A.rawValue:
            return Double(UInt16(val.bytes[0]) << 8 | UInt16(val.bytes[1])) / 1024
        case SMCDataType.SP69.rawValue:
            return Double(UInt16(val.bytes[0]) << 8 | UInt16(val.bytes[1])) / 512
        case SMCDataType.SP78.rawValue:
            return Double(Int(val.bytes[0]) << 8 | Int(val.bytes[1])) / 256
        case SMCDataType.SP87.rawValue:
            return Double(Int(val.bytes[0]) << 8 | Int(val.bytes[1])) / 128
        case SMCDataType.SP96.rawValue:
            return Double(Int(val.bytes[0]) << 8 | Int(val.bytes[1])) / 64
        case SMCDataType.SPA5.rawValue:
            return Double(UInt16(val.bytes[0]) << 8 | UInt16(val.bytes[1])) / 32
        case SMCDataType.SPB4.rawValue:
            return Double(Int(val.bytes[0]) << 8 | Int(val.bytes[1])) / 16
        case SMCDataType.SPF0.rawValue:
            return Double(Int(val.bytes[0]) << 8 | Int(val.bytes[1]))
        case SMCDataType.FLT.rawValue:
            guard let f = Float(val.bytes) else { return nil }
            return Double(f)
        case SMCDataType.FPE2.rawValue:
            return Double((Int(val.bytes[0]) << 6) + (Int(val.bytes[1]) >> 2))
        default:
            return nil
        }
    }

    func getStringValue(_ key: String) -> String? {
        var val = SMCValue(key)
        guard read(&val) == kIOReturnSuccess, val.dataSize > 0 else { return nil }
        guard val.bytes.contains(where: { $0 != 0 }) else { return nil }

        if val.dataType == SMCDataType.FDS.rawValue {
            return String(val.bytes[4...15].map { Character(UnicodeScalar($0)) })
                .trimmingCharacters(in: .whitespaces)
        }
        return nil
    }

    func getAllKeys() -> [String] {
        guard let keysNum = getValue("#KEY"), keysNum.isFinite, keysNum >= 0 else { return [] }
        var list: [String] = []

        for i in 0...Int(keysNum) {
            var input = SMCKeyData()
            var output = SMCKeyData()
            input.data8 = SMCSelector.readIndex.rawValue
            input.data32 = UInt32(i)

            guard call(SMCSelector.kernelIndex.rawValue, input: &input, output: &output) == kIOReturnSuccess else {
                continue
            }
            list.append(output.key.toString())
        }
        return list
    }

    // MARK: - Fan Control

    func fanModeKey(_ id: Int) -> String {
        if fanModeKeyIsLower == nil {
            var probe = SMCValue("F0md")
            fanModeKeyIsLower = read(&probe) == kIOReturnSuccess && probe.dataSize > 0
        }
        return fanModeKeyIsLower! ? "F\(id)md" : "F\(id)Md"
    }

    /// Applies a fan mode change and reports whether the hardware really
    /// accepted it.
    ///
    /// Switching back to automatic is a hardware-safety path: the target
    /// (`F<n>Tg`) is only cleared after the mode register reads back as
    /// automatic. If the mode write fails while the fan is still forced,
    /// writing target 0 would stop a fan that the system is not controlling,
    /// so the failure is propagated instead.
    @discardableResult
    func setFanMode(_ id: Int, mode: FanMode) -> Bool {
        debugLog("[FanControl] smc.setFanMode fan=\(id) mode=\(mode)")

        // Validate before any "F\(id)..." key is interpolated: an invalid id
        // would not just fail the request, it would crash the daemon.
        guard FanIDBounds.isValidFanId(id, fanCount: getValue("FNum")) else {
            debugLog("[FanControl] smc.setFanMode fan=\(id) result=failed reason=invalidFanId")
            return false
        }

        if mode == .forced {
            if unlockFanControl(fanId: id) {
                forcedModeFans.insert(id)
                debugLog("[FanControl] smc.setFanMode forced fan=\(id) result=ok")
                return true
            } else {
                debugLog("[FanControl] smc.setFanMode forced fan=\(id) result=failed")
                return false
            }
        }

        // No optional path around the mode register: a transient getValue
        // failure used to skip verification entirely and still zero the
        // target. The register read itself is now mandatory, and any failed
        // step returns before the target is touched.
        let modeKey = fanModeKey(id)
        var modeVal = SMCValue(modeKey)
        guard read(&modeVal) == kIOReturnSuccess else {
            debugLog("[FanControl] smc.setFanMode auto fan=\(id) result=failed reason=readMode")
            return false
        }

        let wasForced = modeVal.bytes[0] != 0
        if wasForced {
            modeVal.bytes[0] = 0
            guard writeWithRetry(modeVal) else {
                // Hardware is still forced; keep the cache saying so.
                forcedModeFans.insert(id)
                debugLog("[FanControl] smc.setFanMode auto fan=\(id) result=failed reason=writeMode")
                return false
            }
        }

        // Read the register back instead of trusting the write or the
        // process-local cache; only a fan that is confirmed automatic may
        // have its target cleared.
        var verifyVal = SMCValue(modeKey)
        guard read(&verifyVal) == kIOReturnSuccess else {
            if wasForced { forcedModeFans.insert(id) }
            debugLog("[FanControl] smc.setFanMode auto fan=\(id) result=failed reason=verifyRead")
            return false
        }
        guard verifyVal.bytes[0] == 0 else {
            forcedModeFans.insert(id)
            debugLog("[FanControl] smc.setFanMode auto fan=\(id) result=failed reason=stillForced modeByte=\(verifyVal.bytes[0])")
            return false
        }

        // Only now is the fan confirmed system-controlled.
        forcedModeFans.remove(id)

        var targetValue = SMCValue("F\(id)Tg")
        guard read(&targetValue) == kIOReturnSuccess else {
            debugLog("[FanControl] smc.setFanMode auto fan=\(id) result=failed reason=readTarget")
            return false
        }

        let bytes = Float(0).asBytes
        targetValue.bytes[0] = bytes[0]
        targetValue.bytes[1] = bytes[1]
        targetValue.bytes[2] = bytes[2]
        targetValue.bytes[3] = bytes[3]
        let ok = writeWithRetry(targetValue)
        debugLog("[FanControl] smc.setFanMode auto fan=\(id) result=\(ok ? "ok" : "failed reason=writeTarget")")
        return ok
    }

    @discardableResult
    func setFanSpeed(_ id: Int, speed: Int) -> Bool {
        debugLog("[FanControl] smc.setFanSpeed fan=\(id) requested=\(speed)")

        // Validate before any "F\(id)..." key is interpolated: an invalid id
        // would not just fail the request, it would crash the daemon.
        guard FanIDBounds.isValidFanId(id, fanCount: getValue("FNum")) else {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=failed reason=invalidFanId")
            return false
        }

        // Fail closed unless the live Mn/Mx bounds are both readable and
        // sane: an RPM write that is not bounded on both sides must never
        // reach the encoder. This also rejects negative requests before the
        // FPE2 UInt8 conversion could trap, and clamps positive requests up
        // to the minimum so a 1 RPM write can never be accepted below it.
        guard let boundedSpeed = FanRPMBounds.validatedRPM(
            requestedRPM: speed,
            minimumRPM: getValue("F\(id)Mn"),
            maximumRPM: getValue("F\(id)Mx"),
            allowFanOff: true
        ) else {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) requested=\(speed) result=failed reason=bounds")
            return false
        }
        if boundedSpeed != speed {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) clampToMax=\(boundedSpeed)")
        }

        // Sleep resets the hardware fan mode to automatic without restarting
        // the privileged helper. Always verify the SMC register so the
        // process-local cache cannot suppress the unlock after wake.
        let cachedAsForced = forcedModeFans.contains(id)
        var modeVal = SMCValue(fanModeKey(id))
        guard read(&modeVal) == kIOReturnSuccess else {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=failed reason=readMode")
            return false
        }

        let hardwareIsForced = modeVal.bytes[0] == 1
        debugLog(
            "[FanControl] smc.setFanSpeed fan=\(id) modeByte=\(modeVal.bytes[0]) cachedForced=\(cachedAsForced)"
        )

        if !hardwareIsForced {
            forcedModeFans.remove(id)

            if cachedAsForced {
                // The SMC reset independently, normally because the machine
                // slept. A stale cooldown must not postpone wake recovery.
                lastUnlockAttemptAt[id] = nil
                debugLog("[FanControl] smc.setFanSpeed fan=\(id) detectedHardwareModeReset")
            }

            guard canAttemptUnlock(fanId: id) else {
                debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=failed reason=unlockCooldown")
                return false
            }
            lastUnlockAttemptAt[id] = Date()
            guard unlockFanControl(fanId: id) else {
                debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=failed reason=unlock")
                return false
            }
        }
        forcedModeFans.insert(id)

        // From here on the hardware is forced (either we just unlocked it or
        // it already was). Every remaining path where the new target is not
        // confirmed must hand the fan back to system control — removing the
        // process-local cache entry alone would leave the hardware forced at
        // a stale target.
        var value = SMCValue("F\(id)Tg")
        guard read(&value) == kIOReturnSuccess else {
            fallBackToAutomatic(fanId: id, reason: "readTarget")
            return false
        }
        debugLog("[FanControl] smc.setFanSpeed fan=\(id) targetType=\(value.dataType)")

        guard FanTargetWritePolicy.isEncodableTargetType(value.dataType) else {
            fallBackToAutomatic(fanId: id, reason: "unknownTargetType(\(value.dataType))")
            return false
        }

        // boundedSpeed is guaranteed non-negative and within the encodable
        // range by FanRPMBounds, so these conversions cannot trap.
        if value.dataType == SMCDataType.FLT.rawValue {
            let bytes = Float(boundedSpeed).asBytes
            value.bytes[0] = bytes[0]
            value.bytes[1] = bytes[1]
            value.bytes[2] = bytes[2]
            value.bytes[3] = bytes[3]
        } else {
            value.bytes[0] = UInt8(boundedSpeed >> 6)
            value.bytes[1] = UInt8((boundedSpeed << 2) ^ ((boundedSpeed >> 6) << 8))
        }

        if writeWithRetry(value) {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) target=\(boundedSpeed) result=ok")
            return true
        } else {
            fallBackToAutomatic(fanId: id, reason: "writeTarget")
            return false
        }
    }

    /// Best-effort return of a fan to system control after a forced-mode
    /// target write could not be confirmed. `setFanMode(.automatic)`
    /// verifies the mode register itself and manages the forced-mode cache;
    /// the fallback outcome is logged either way.
    private func fallBackToAutomatic(fanId: Int, reason: String) {
        let restored = setFanMode(fanId, mode: .automatic)
        debugLog(
            "[FanControl] smc.setFanSpeed fan=\(fanId) fallback=automatic reason=\(reason) "
                + "result=\(restored ? "ok" : "failed")"
        )
    }

    @discardableResult
    func resetFanControl() -> Bool {
        forcedModeFans.removeAll()
        var value = SMCValue("Ftst")
        let result = read(&value)
        if result == kIOReturnSuccess && value.dataSize > 0 {
            if value.bytes[0] == 0 { return true }
            value.bytes[0] = 0
            return writeWithRetry(value)
        }

        // Shared FanIDBounds validation: a corrupt FNum fails closed and
        // can never interpolate keys that do not exist.
        guard let fanLimit = FanIDBounds.validFanCount(getValue("FNum")) else { return false }
        var success = true
        for i in 0..<fanLimit {
            let modeKey = fanModeKey(i)
            var modeVal = SMCValue(modeKey)
            guard read(&modeVal) == kIOReturnSuccess else { continue }
            if modeVal.bytes[0] == 0 { continue }
            modeVal.bytes[0] = 0
            if !writeWithRetry(modeVal) { success = false }
        }
        return success
    }

    // MARK: - Private

    @discardableResult
    private func writeWithRetry(_ value: SMCValue, maxAttempts: Int = 10, delayMicros: UInt32 = 50_000) -> Bool {
        for attempt in 0..<maxAttempts {
            if write(value) == kIOReturnSuccess { return true }
            if attempt < maxAttempts - 1 { usleep(delayMicros) }
        }
        return false
    }

    @discardableResult
    private func unlockFanControl(fanId: Int) -> Bool {
        debugLog("[FanControl] smc.unlock fan=\(fanId) start")
        let modeKey = fanModeKey(fanId)
        var modeVal = SMCValue(modeKey)
        guard read(&modeVal) == kIOReturnSuccess else {
            debugLog("[FanControl] smc.unlock fan=\(fanId) result=failed reason=readMode")
            return false
        }

        modeVal.bytes[0] = 1
        if write(modeVal) == kIOReturnSuccess {
            debugLog("[FanControl] smc.unlock fan=\(fanId) result=ok method=direct")
            return true
        }

        var ftstVal = SMCValue("Ftst")
        guard read(&ftstVal) == kIOReturnSuccess, ftstVal.dataSize > 0 else {
            debugLog("[FanControl] smc.unlock fan=\(fanId) result=failed reason=readFtst")
            return false
        }

        if ftstVal.bytes[0] == 1 {
            let ok = retryModeWrite(fanId: fanId, maxAttempts: 20)
            debugLog("[FanControl] smc.unlock fan=\(fanId) result=\(ok ? "ok" : "failed") method=retryExistingFtst")
            return ok
        }

        ftstVal.bytes[0] = 1
        guard writeWithRetry(ftstVal, maxAttempts: 100) else {
            debugLog("[FanControl] smc.unlock fan=\(fanId) result=failed reason=writeFtst")
            return false
        }
        usleep(3_000_000)
        let ok = retryModeWrite(fanId: fanId, maxAttempts: 300)
        debugLog("[FanControl] smc.unlock fan=\(fanId) result=\(ok ? "ok" : "failed") method=retryAfterFtst")
        return ok
    }

    private func retryModeWrite(fanId: Int, maxAttempts: Int) -> Bool {
        let modeKey = fanModeKey(fanId)
        var modeVal = SMCValue(modeKey)
        guard read(&modeVal) == kIOReturnSuccess else { return false }
        modeVal.bytes[0] = 1
        return writeWithRetry(modeVal, maxAttempts: maxAttempts, delayMicros: 100_000)
    }

    private func canAttemptUnlock(fanId: Int) -> Bool {
        guard let last = lastUnlockAttemptAt[fanId] else { return true }
        return Date().timeIntervalSince(last) >= unlockRetryCooldown
    }

    private func read(_ value: UnsafeMutablePointer<SMCValue>) -> kern_return_t {
        var input = SMCKeyData()
        var output = SMCKeyData()

        input.key = FourCharCode(fromString: value.pointee.key)
        if let cached = keyInfoCache[value.pointee.key] {
            value.pointee.dataSize = cached.dataSize
            value.pointee.dataType = cached.dataType
            input.keyInfo.dataSize = IOByteCount32(cached.dataSize)
        } else {
            input.data8 = SMCSelector.readKeyInfo.rawValue
            let infoResult = call(
                SMCSelector.kernelIndex.rawValue,
                input: &input,
                output: &output
            )
            guard infoResult == kIOReturnSuccess else { return infoResult }

            let dataSize = UInt32(output.keyInfo.dataSize)
            let dataType = output.keyInfo.dataType.toString()
            value.pointee.dataSize = dataSize
            value.pointee.dataType = dataType
            input.keyInfo.dataSize = output.keyInfo.dataSize
            keyInfoCache[value.pointee.key] = CachedKeyInfo(
                dataSize: dataSize,
                dataType: dataType
            )
        }
        input.data8 = SMCSelector.readBytes.rawValue

        let result = call(
            SMCSelector.kernelIndex.rawValue,
            input: &input,
            output: &output
        )
        guard result == kIOReturnSuccess else {
            keyInfoCache[value.pointee.key] = nil
            return result
        }

        memcpy(&value.pointee.bytes, &output.bytes, min(Int(value.pointee.dataSize), value.pointee.bytes.count))
        return kIOReturnSuccess
    }

    private func write(_ value: SMCValue) -> kern_return_t {
        var input = SMCKeyData()
        var output = SMCKeyData()

        input.key = FourCharCode(fromString: value.key)
        input.data8 = SMCSelector.writeBytes.rawValue
        input.keyInfo.dataSize = IOByteCount32(value.dataSize)
        input.bytes = (value.bytes[0], value.bytes[1], value.bytes[2], value.bytes[3],
                       value.bytes[4], value.bytes[5], value.bytes[6], value.bytes[7],
                       value.bytes[8], value.bytes[9], value.bytes[10], value.bytes[11],
                       value.bytes[12], value.bytes[13], value.bytes[14], value.bytes[15],
                       value.bytes[16], value.bytes[17], value.bytes[18], value.bytes[19],
                       value.bytes[20], value.bytes[21], value.bytes[22], value.bytes[23],
                       value.bytes[24], value.bytes[25], value.bytes[26], value.bytes[27],
                       value.bytes[28], value.bytes[29], value.bytes[30], value.bytes[31])

        let result = call(SMCSelector.kernelIndex.rawValue, input: &input, output: &output)
        guard result == kIOReturnSuccess else { return result }
        if output.result != 0x00 { return kIOReturnError }
        return kIOReturnSuccess
    }

    private func call(_ index: UInt8, input: inout SMCKeyData, output: inout SMCKeyData) -> kern_return_t {
        let inputSize = MemoryLayout<SMCKeyData>.stride
        var outputSize = MemoryLayout<SMCKeyData>.stride
        return IOConnectCallStructMethod(conn, UInt32(index), &input, inputSize, &output, &outputSize)
    }
}
