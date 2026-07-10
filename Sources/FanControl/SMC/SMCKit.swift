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
    static let shared = SMCKit()
    private var conn: io_connect_t = 0
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
        guard let keysNum = getValue("#KEY") else { return [] }
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

    func setFanMode(_ id: Int, mode: FanMode) {
        debugLog("[FanControl] smc.setFanMode fan=\(id) mode=\(mode)")
        if mode == .forced {
            if unlockFanControl(fanId: id) {
                forcedModeFans.insert(id)
                debugLog("[FanControl] smc.setFanMode forced fan=\(id) result=ok")
            } else {
                debugLog("[FanControl] smc.setFanMode forced fan=\(id) result=failed")
            }
        } else {
            forcedModeFans.remove(id)
            let modeKey = fanModeKey(id)
            if getValue(modeKey) != nil {
                var modeVal = SMCValue(modeKey)
                guard read(&modeVal) == kIOReturnSuccess else { return }
                if modeVal.bytes[0] != 0 {
                    modeVal.bytes[0] = 0
                    writeWithRetry(modeVal)
                }
            }

            var targetValue = SMCValue("F\(id)Tg")
            guard read(&targetValue) == kIOReturnSuccess else { return }

            let bytes = Float(0).asBytes
            targetValue.bytes[0] = bytes[0]
            targetValue.bytes[1] = bytes[1]
            targetValue.bytes[2] = bytes[2]
            targetValue.bytes[3] = bytes[3]
            writeWithRetry(targetValue)
        }
    }

    func setFanSpeed(_ id: Int, speed: Int) {
        debugLog("[FanControl] smc.setFanSpeed fan=\(id) requested=\(speed)")
        if let maxSpeed = getValue("F\(id)Mx"), speed > Int(maxSpeed) {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) clampToMax=\(Int(maxSpeed))")
            return setFanSpeed(id, speed: Int(maxSpeed))
        }

        if !forcedModeFans.contains(id) {
            var modeVal = SMCValue(fanModeKey(id))
            guard read(&modeVal) == kIOReturnSuccess else {
                debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=failed reason=readMode")
                return
            }
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) modeByte=\(modeVal.bytes[0])")
            if modeVal.bytes[0] != 1 {
                guard canAttemptUnlock(fanId: id) else {
                    debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=skipped reason=unlockCooldown")
                    return
                }
                lastUnlockAttemptAt[id] = Date()
                guard unlockFanControl(fanId: id) else {
                    debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=failed reason=unlock")
                    return
                }
            }
            forcedModeFans.insert(id)
        }

        var value = SMCValue("F\(id)Tg")
        guard read(&value) == kIOReturnSuccess else {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) result=failed reason=readTarget")
            return
        }
        debugLog("[FanControl] smc.setFanSpeed fan=\(id) targetType=\(value.dataType)")

        if value.dataType == SMCDataType.FLT.rawValue {
            let bytes = Float(speed).asBytes
            value.bytes[0] = bytes[0]
            value.bytes[1] = bytes[1]
            value.bytes[2] = bytes[2]
            value.bytes[3] = bytes[3]
        } else if value.dataType == SMCDataType.FPE2.rawValue {
            value.bytes[0] = UInt8(speed >> 6)
            value.bytes[1] = UInt8((speed << 2) ^ ((speed >> 6) << 8))
        }

        if writeWithRetry(value) {
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) target=\(speed) result=ok")
        } else {
            forcedModeFans.remove(id)
            debugLog("[FanControl] smc.setFanSpeed fan=\(id) target=\(speed) result=failed reason=writeTarget")
        }
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

        guard let count = getValue("FNum") else { return false }
        var success = true
        for i in 0..<Int(count) {
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
        input.data8 = SMCSelector.readKeyInfo.rawValue

        var result = call(SMCSelector.kernelIndex.rawValue, input: &input, output: &output)
        guard result == kIOReturnSuccess else { return result }

        value.pointee.dataSize = UInt32(output.keyInfo.dataSize)
        value.pointee.dataType = output.keyInfo.dataType.toString()
        input.keyInfo.dataSize = output.keyInfo.dataSize
        input.data8 = SMCSelector.readBytes.rawValue

        result = call(SMCSelector.kernelIndex.rawValue, input: &input, output: &output)
        guard result == kIOReturnSuccess else { return result }

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
