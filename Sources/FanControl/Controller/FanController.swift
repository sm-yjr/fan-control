import Foundation
import Darwin

struct FanState: Codable, Identifiable {
    var id: Int { fanId }
    let fanId: Int
    var mode: FanControlMode = .automatic
    var curveConfig: FanCurveConfig?

    var lastTemperature: Double = 0
    var lastSpeedPercent: Double = 0
    var wasRising: Bool = true

    init(fanId: Int) {
        self.fanId = fanId
    }

    private enum CodingKeys: String, CodingKey {
        case fanId
        case mode
        case curveConfig
        case lastTemperature
        case lastSpeedPercent
        case wasRising
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        fanId = try container.decode(Int.self, forKey: .fanId)
        mode = try container.decodeIfPresent(FanControlMode.self, forKey: .mode) ?? .automatic
        curveConfig = try container.decodeIfPresent(FanCurveConfig.self, forKey: .curveConfig)
        lastTemperature = try container.decodeIfPresent(Double.self, forKey: .lastTemperature) ?? 0
        lastSpeedPercent = try container.decodeIfPresent(Double.self, forKey: .lastSpeedPercent) ?? 0
        wasRising = try container.decodeIfPresent(Bool.self, forKey: .wasRising) ?? true
    }
}

@Observable
final class FanController {
    var fanStates: [FanState] = []
    var isActive: Bool = false

    private var sensorManager: SensorManager
    private var loadedStates: [FanState] = []
    private var lastTargetRPM: [Int: Int] = [:]
    private var lastWriteAt: [Int: Date] = [:]
    private var fanWriteInFlight: Set<Int> = []
    private var pendingTargetRPM: [Int: Int] = [:]
    private var fanOffEnteredAt: [Int: Date] = [:]
    private var fanStartedAt: [Int: Date] = [:]
    private var lastModeReconcileAt: [Int: Date] = [:]
    private var rpmMismatchStartedAt: [Int: Date] = [:]
    private var lastRPMReconcileAt: [Int: Date] = [:]
    private var manualWriteTimers: [Int: Timer] = [:]
    private var configSaveTimer: Timer?
    private var needsConfigMigrationWrite = false
    private let configPersistenceQueue = DispatchQueue(
        label: "FanControl.ConfigPersistence",
        qos: .utility
    )
    private let manualWriteDelay: TimeInterval = 0.25
    private let configSaveDelay: TimeInterval = 0.35
    private let minRPMDelta = 75
    private let minWriteInterval: TimeInterval = 5
    private let minModeReconcileInterval: TimeInterval = 30
    private let minRPMMismatchDuration: TimeInterval = 10
    private let minRPMReconcileInterval: TimeInterval = 15
    private let maxRampUpRPMPerSecond: Double = 350
    private let maxRampDownRPMPerSecond: Double = 250
    private let inputDirectionDeadband: Double = 0.5
    private let minFanOffResidence: TimeInterval = 90
    private let minFanRunResidenceAfterOff: TimeInterval = 180

    init(sensorManager: SensorManager) {
        self.sensorManager = sensorManager
        loadConfig()
    }

    var pollingActivity: FanPollingActivity {
        if fanStates.contains(where: {
            if case .curve = $0.mode { return true }
            return false
        }) {
            return .curve
        }
        if fanStates.contains(where: {
            if case .manual = $0.mode { return true }
            return false
        }) {
            return .manual
        }
        return .automatic
    }

    func start() {
        isActive = true
        syncFans(sensorManager.fans)
        handleSensorUpdate()
    }

    func stop(completion: (() -> Void)? = nil) {
        flushPendingConfigSave()
        isActive = false
        let states = fanStates
        cancelAllManualWrites()
        fanWriteInFlight.removeAll()
        pendingTargetRPM.removeAll()
        fanOffEnteredAt.removeAll()
        fanStartedAt.removeAll()
        lastModeReconcileAt.removeAll()
        rpmMismatchStartedAt.removeAll()
        lastRPMReconcileAt.removeAll()

        let group = DispatchGroup()
        for state in states {
            group.enter()
            FanControlWriter.setFanMode(state.fanId, mode: .automatic) {
                group.leave()
            }
        }
        group.enter()
        FanControlWriter.resetAll {
            group.leave()
        }

        DispatchQueue.global(qos: .utility).async {
            _ = group.wait(timeout: .now() + 3)
            for state in states {
                debugLog("[FanControl] stop reset fan=\(state.fanId)")
            }
            if let completion {
                DispatchQueue.main.async { completion() }
            }
        }
    }

    func setMode(_ mode: FanControlMode, forFan fanId: Int) {
        guard let idx = fanStates.firstIndex(where: { $0.fanId == fanId }) else { return }
        let oldMode = fanStates[idx].mode
        fanStates[idx].mode = mode
        debugLog("[FanControl] setMode fan=\(fanId) mode=\(mode)")

        switch mode {
        case .automatic:
            cancelManualWrite(forFan: fanId)
            setAutomatic(fanId: fanId)
        case .manual(let rpm):
            if case .manual = oldMode {
                scheduleManualSpeedWrite(fanId: fanId, rpm: rpm)
            } else {
                cancelManualWrite(forFan: fanId)
                scheduleFanSpeedWrite(fanId: fanId, rpm: rpm, force: true)
            }
        case .curve:
            cancelManualWrite(forFan: fanId)
            break
        }

        saveConfig()
    }

    func setCurveConfig(_ config: FanCurveConfig, forFan fanId: Int) {
        guard let idx = fanStates.firstIndex(where: { $0.fanId == fanId }) else { return }
        let sourceChanged = fanStates[idx].curveConfig?.sensorKey != config.sensorKey
        cancelManualWrite(forFan: fanId)
        fanStates[idx].curveConfig = config
        fanStates[idx].mode = .curve(configId: config.id)
        if sourceChanged {
            fanStates[idx].lastTemperature = 0
            fanStates[idx].lastSpeedPercent = 0
            fanStates[idx].wasRising = true
        }
        saveConfig()
    }

    func resetCurve(forFan fanId: Int) {
        guard let idx = fanStates.firstIndex(where: { $0.fanId == fanId }) else { return }
        cancelManualWrite(forFan: fanId)
        let sensorKey = fanStates[idx].curveConfig?.sensorKey ?? defaultSensorKey
        let config = FanCurveConfig.defaultCurve(sensorKey: sensorKey)
        fanStates[idx].curveConfig = config
        fanStates[idx].mode = .curve(configId: config.id)
        fanStates[idx].lastTemperature = 0
        fanStates[idx].lastSpeedPercent = 0
        fanStates[idx].wasRising = true
        lastTargetRPM[fanId] = nil
        lastWriteAt[fanId] = nil
        fanOffEnteredAt[fanId] = nil
        fanStartedAt[fanId] = nil
        rpmMismatchStartedAt[fanId] = nil
        lastRPMReconcileAt[fanId] = nil
        saveConfig()
        handleSensorUpdate()
    }

    func prepareForSleep() {
        debugLog("[FanControl] prepareForSleep")
        cancelAllManualWrites()
        fanWriteInFlight.removeAll()
        pendingTargetRPM.removeAll()
    }

    func reapplyConfiguredModes(reason: String) {
        guard isActive else { return }
        debugLog("[FanControl] reapplyConfiguredModes reason=\(reason) fans=\(fanStates.count)")
        cancelAllManualWrites()
        fanWriteInFlight.removeAll()
        pendingTargetRPM.removeAll()
        lastTargetRPM.removeAll()
        lastWriteAt.removeAll()
        lastModeReconcileAt.removeAll()
        rpmMismatchStartedAt.removeAll()
        lastRPMReconcileAt.removeAll()

        syncFans(sensorManager.fans)

        for state in fanStates {
            switch state.mode {
            case .automatic:
                debugLog("[FanControl] reapply fan=\(state.fanId) mode=automatic")
                FanControlWriter.setFanMode(state.fanId, mode: .automatic)
            case .manual(let rpm):
                debugLog("[FanControl] reapply fan=\(state.fanId) mode=manual rpm=\(rpm)")
                fanOffEnteredAt[state.fanId] = nil
                fanStartedAt[state.fanId] = nil
                scheduleFanSpeedWrite(fanId: state.fanId, rpm: rpm, force: true)
            case .curve:
                debugLog("[FanControl] reapply fan=\(state.fanId) mode=curve")
                fanStatesApplyCurveReset(fanId: state.fanId)
            }
        }

        handleSensorUpdate()
    }

    func syncFans(_ fans: [FanInfo]) {
        let fanIds = Set(fans.map(\.id))
        fanStates.removeAll { !fanIds.contains($0.fanId) }

        for fan in fans where !fanStates.contains(where: { $0.fanId == fan.id }) {
            let saved = loadedStates.first { $0.fanId == fan.id }
            fanStates.append(saved ?? FanState(fanId: fan.id))
        }

        if needsConfigMigrationWrite, !fanStates.isEmpty {
            saveConfig()
            needsConfigMigrationWrite = false
        }
    }

    func handleSensorUpdate() {
        guard isActive else { return }
        syncFans(sensorManager.fans)

        for i in fanStates.indices {
            let fanId = fanStates[i].fanId
            guard fanId < sensorManager.fans.count else { continue }
            let fan = sensorManager.fans[fanId]
            let shouldForceReconcile = shouldForceModeReconcile(fanId: fanId, observedMode: fan.mode)

            switch fanStates[i].mode {
            case .automatic:
                continue
            case .manual(let rpm):
                let shouldForceRPM = shouldForceRPMReconcile(fanId: fanId, fan: fan, desiredRPM: rpm)
                if shouldForceReconcile || shouldForceRPM {
                    debugLog("[FanControl] reconcileFanMode fan=\(fanId) desired=manual observed=automatic rpm=\(rpm)")
                    scheduleFanSpeedWrite(fanId: fanId, rpm: rpm, force: true)
                }
                continue
            case .curve(let configId):
                guard let config = fanStates[i].curveConfig, config.id == configId else { continue }

                guard let controlInput = sensorManager.curveInputValue(for: config.sensorKey) else {
                    continue
                }

                let inputDelta = controlInput - fanStates[i].lastTemperature
                let isRising: Bool
                if inputDelta > inputDirectionDeadband {
                    isRising = true
                } else if inputDelta < -inputDirectionDeadband {
                    isRising = false
                } else {
                    isRising = fanStates[i].wasRising
                }

                let requestedSpeedPercent = config.interpolateWithHysteresis(
                    temperature: controlInput,
                    lastSpeed: fanStates[i].lastSpeedPercent,
                    isRising: isRising
                )
                let speedPercent = safetyAdjustedSpeed(
                    requestedSpeedPercent,
                    controlInput: controlInput,
                    sensorKey: config.sensorKey
                )

                let desiredRPM = curveTargetRPM(fan: fan, speedPercent: speedPercent)
                let shouldForceRPM = shouldForceRPMReconcile(fanId: fanId, fan: fan, desiredRPM: desiredRPM)

                if shouldForceReconcile {
                    debugLog("[FanControl] reconcileFanMode fan=\(fanId) desired=curve observed=automatic")
                }
                applyCurveTarget(
                    fanId: fanId,
                    fan: fan,
                    speedPercent: speedPercent,
                    controlInput: controlInput,
                    force: shouldForceReconcile || shouldForceRPM
                )

                fanStates[i].lastTemperature = controlInput
                fanStates[i].lastSpeedPercent = speedPercent
                fanStates[i].wasRising = isRising
            }
        }
    }

    private func setAutomatic(fanId: Int) {
        lastTargetRPM[fanId] = nil
        lastWriteAt[fanId] = nil
        fanOffEnteredAt[fanId] = nil
        fanStartedAt[fanId] = nil
        lastModeReconcileAt[fanId] = nil
        rpmMismatchStartedAt[fanId] = nil
        lastRPMReconcileAt[fanId] = nil
        FanControlWriter.setFanMode(fanId, mode: .automatic)
    }

    private func fanStatesApplyCurveReset(fanId: Int) {
        guard let idx = fanStates.firstIndex(where: { $0.fanId == fanId }) else { return }
        fanStates[idx].lastTemperature = 0
        fanStates[idx].lastSpeedPercent = 0
        fanStates[idx].wasRising = true
        lastTargetRPM[fanId] = nil
        lastWriteAt[fanId] = nil
        lastModeReconcileAt[fanId] = nil
        rpmMismatchStartedAt[fanId] = nil
        lastRPMReconcileAt[fanId] = nil
    }

    private func applyCurveTarget(
        fanId: Int,
        fan: FanInfo,
        speedPercent: Double,
        controlInput: Double,
        force: Bool = false
    ) {
        let now = Date()
        let wantsFanOff = FanCurveConfig.isFanOffSpeed(speedPercent)
        let isCurrentlyOff = isFanOff(fanId: fanId, fan: fan)
        let thermallyUrgent = sensorManager.systemThermalPressure >= .serious
            || sensorManager.hottestSiliconTemperature >= 96
            || controlInput >= 90
            || speedPercent >= 70
        let bypassRamp = shouldBypassRampLimit(
            fan: fan,
            speedPercent: speedPercent,
            thermallyUrgent: thermallyUrgent
        )

        if wantsFanOff {
            if let startedAt = fanStartedAt[fanId],
               now.timeIntervalSince(startedAt) < minFanRunResidenceAfterOff,
               !thermallyUrgent {
                let remaining = minFanRunResidenceAfterOff - now.timeIntervalSince(startedAt)
                let holdRPM = minimumRunningRPM(for: fan)
                debugLog("[FanControl] holdFanRunning fan=\(fanId) rpm=\(holdRPM) remaining=\(String(format: "%.1f", remaining))s")
                scheduleFanSpeedWrite(fanId: fanId, rpm: holdRPM, force: force, bypassRampLimit: bypassRamp)
                return
            }

            if !isCurrentlyOff {
                debugLog("[FanControl] enterFanOff fan=\(fanId) minResidence=\(Int(minFanOffResidence))s")
                fanOffEnteredAt[fanId] = now
            } else if fanOffEnteredAt[fanId] == nil {
                fanOffEnteredAt[fanId] = now
            }
            fanStartedAt[fanId] = nil
            scheduleFanSpeedWrite(fanId: fanId, rpm: 0, force: force, allowBelowMin: true, bypassRampLimit: bypassRamp)
            return
        }

        let targetRPM = curveTargetRPM(fan: fan, speedPercent: speedPercent)

        if isCurrentlyOff,
           !thermallyUrgent,
           let offEnteredAt = fanOffEnteredAt[fanId],
           now.timeIntervalSince(offEnteredAt) < minFanOffResidence {
            let remaining = minFanOffResidence - now.timeIntervalSince(offEnteredAt)
            debugLog("[FanControl] holdFanOff fan=\(fanId) requestedRPM=\(targetRPM) remaining=\(String(format: "%.1f", remaining))s")
            scheduleFanSpeedWrite(fanId: fanId, rpm: 0, force: force, allowBelowMin: true, bypassRampLimit: bypassRamp)
            return
        }

        if isCurrentlyOff {
            debugLog("[FanControl] leaveFanOff fan=\(fanId) targetRPM=\(targetRPM) minRun=\(Int(minFanRunResidenceAfterOff))s")
            fanStartedAt[fanId] = now
            fanOffEnteredAt[fanId] = nil
        }
        scheduleFanSpeedWrite(fanId: fanId, rpm: targetRPM, force: force, bypassRampLimit: bypassRamp)
    }

    private func scheduleFanSpeedWrite(
        fanId: Int,
        rpm: Int,
        force: Bool = false,
        allowBelowMin: Bool = false,
        bypassRampLimit: Bool = false
    ) {
        let targetRPM = clampRPM(rpm, forFan: fanId, allowBelowMin: allowBelowMin)
        let clampedRPM = applyRampLimit(
            fanId: fanId,
            targetRPM: targetRPM,
            allowBelowMin: allowBelowMin,
            bypass: bypassRampLimit
        )
        guard force || shouldWrite(fanId: fanId, rpm: clampedRPM) else {
            debugLog("[FanControl] skipWrite fan=\(fanId) rpm=\(clampedRPM) reason=throttled")
            return
        }

        if fanWriteInFlight.contains(fanId) {
            pendingTargetRPM[fanId] = clampedRPM
            debugLog("[FanControl] queuePending fan=\(fanId) rpm=\(clampedRPM)")
            return
        }

        let previousTargetRPM = lastTargetRPM[fanId]
        fanWriteInFlight.insert(fanId)
        lastTargetRPM[fanId] = clampedRPM
        lastWriteAt[fanId] = Date()
        recordFanTransition(fanId: fanId, previousRPM: previousTargetRPM, rpm: clampedRPM)
        debugLog("[FanControl] scheduleWrite fan=\(fanId) rpm=\(clampedRPM) force=\(force)")

        FanControlWriter.setFanRPM(fanId, rpm: clampedRPM) { [weak self] in
            guard let self else { return }
            self.fanWriteInFlight.remove(fanId)
            if let pending = self.pendingTargetRPM.removeValue(forKey: fanId) {
                self.scheduleFanSpeedWrite(fanId: fanId, rpm: pending, allowBelowMin: pending == 0)
            }
        }
    }

    private func scheduleManualSpeedWrite(fanId: Int, rpm: Int) {
        cancelManualWrite(forFan: fanId)
        let timer = Timer(timeInterval: manualWriteDelay, repeats: false) { [weak self] _ in
            guard let self,
                  let state = self.fanStates.first(where: { $0.fanId == fanId }),
                  case .manual(let currentRPM) = state.mode,
                  currentRPM == rpm else {
                return
            }
            self.manualWriteTimers[fanId] = nil
            self.scheduleFanSpeedWrite(fanId: fanId, rpm: rpm, force: true)
        }
        manualWriteTimers[fanId] = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    private func cancelManualWrite(forFan fanId: Int) {
        manualWriteTimers.removeValue(forKey: fanId)?.invalidate()
    }

    private func cancelAllManualWrites() {
        for timer in manualWriteTimers.values {
            timer.invalidate()
        }
        manualWriteTimers.removeAll()
    }

    private func shouldWrite(fanId: Int, rpm: Int) -> Bool {
        guard let previous = lastTargetRPM[fanId] else { return true }
        if previous == rpm { return false }

        let delta = abs(previous - rpm)
        let elapsed = Date().timeIntervalSince(lastWriteAt[fanId] ?? .distantPast)
        return delta >= minRPMDelta || elapsed >= minWriteInterval
    }

    private func shouldForceModeReconcile(fanId: Int, observedMode: FanMode) -> Bool {
        guard observedMode == .automatic else { return false }
        let now = Date()
        if let last = lastModeReconcileAt[fanId],
           now.timeIntervalSince(last) < minModeReconcileInterval {
            return false
        }
        lastModeReconcileAt[fanId] = now
        return true
    }

    private func shouldForceRPMReconcile(fanId: Int, fan: FanInfo, desiredRPM: Int) -> Bool {
        let effectiveDesiredRPM = lastTargetRPM[fanId] ?? desiredRPM
        let tolerance = rpmTolerance(for: effectiveDesiredRPM)
        let delta = abs(Int(fan.currentSpeed) - effectiveDesiredRPM)
        let now = Date()

        guard delta > tolerance else {
            rpmMismatchStartedAt[fanId] = nil
            return false
        }

        if rpmMismatchStartedAt[fanId] == nil {
            rpmMismatchStartedAt[fanId] = now
            debugLog("[FanControl] rpmMismatchStart fan=\(fanId) desired=\(effectiveDesiredRPM) rawDesired=\(desiredRPM) actual=\(Int(fan.currentSpeed)) delta=\(delta)")
            return false
        }

        guard now.timeIntervalSince(rpmMismatchStartedAt[fanId] ?? now) >= minRPMMismatchDuration else {
            return false
        }

        if let last = lastRPMReconcileAt[fanId],
           now.timeIntervalSince(last) < minRPMReconcileInterval {
            return false
        }

        lastRPMReconcileAt[fanId] = now
        debugLog("[FanControl] reconcileRPM fan=\(fanId) desired=\(effectiveDesiredRPM) rawDesired=\(desiredRPM) actual=\(Int(fan.currentSpeed)) delta=\(delta)")
        return true
    }

    private func curveTargetRPM(fan: FanInfo, speedPercent: Double) -> Int {
        if FanCurveConfig.isFanOffSpeed(speedPercent) { return 0 }
        return Int(fan.minSpeed + (speedPercent / 100.0) * (fan.maxSpeed - fan.minSpeed))
    }

    private func rpmTolerance(for desiredRPM: Int) -> Int {
        if desiredRPM == 0 { return 100 }
        return max(250, Int(Double(desiredRPM) * 0.12))
    }

    private func applyRampLimit(fanId: Int, targetRPM: Int, allowBelowMin: Bool, bypass: Bool) -> Int {
        guard let fan = sensorManager.fans.first(where: { $0.id == fanId }) else { return targetRPM }

        let now = Date()
        let previousRPM = lastTargetRPM[fanId] ?? Int(fan.currentSpeed)
        let elapsed = now.timeIntervalSince(lastWriteAt[fanId] ?? now)
        let controlMode = fanStates.first(where: { $0.fanId == fanId })?.mode ?? .automatic
        let limitedRPM = FanSpeedWritePolicy.targetRPM(
            requestedRPM: targetRPM,
            previousRPM: previousRPM,
            elapsed: elapsed,
            controlMode: controlMode,
            maximumRampUpPerSecond: maxRampUpRPMPerSecond,
            maximumRampDownPerSecond: maxRampDownRPMPerSecond,
            bypassRampLimit: bypass,
            preservesStartFromStopped: previousRPM == 0 && targetRPM > 0 && !allowBelowMin
        )
        guard limitedRPM != targetRPM else { return targetRPM }

        let clamped = clampRPM(limitedRPM, forFan: fanId, allowBelowMin: allowBelowMin)
        debugLog("[FanControl] timeHysteresis fan=\(fanId) desired=\(targetRPM) limited=\(clamped) previous=\(previousRPM) elapsed=\(String(format: "%.1f", elapsed))s")
        return clamped
    }

    private func shouldBypassRampLimit(
        fan: FanInfo,
        speedPercent: Double,
        thermallyUrgent: Bool
    ) -> Bool {
        thermallyUrgent || speedPercent >= 90 || fan.currentSpeed >= fan.maxSpeed * 0.95
    }

    private func safetyAdjustedSpeed(
        _ requestedSpeed: Double,
        controlInput: Double,
        sensorKey: String
    ) -> Double {
        switch sensorManager.systemThermalPressure {
        case .critical:
            return 100
        case .serious:
            return max(requestedSpeed, 70)
        case .nominal, .fair:
            break
        }

        if sensorManager.hottestSiliconTemperature >= 96 {
            return max(requestedSpeed, 80)
        }

        if CurveInput.isThermalDemand(sensorKey) {
            if controlInput >= 90 { return max(requestedSpeed, 75) }
            if controlInput >= 75 { return max(requestedSpeed, 45) }
        } else if controlInput >= 95 {
            return max(requestedSpeed, 80)
        }
        return requestedSpeed
    }

    private func clampRPM(_ rpm: Int, forFan fanId: Int, allowBelowMin: Bool = false) -> Int {
        guard let fan = sensorManager.fans.first(where: { $0.id == fanId }) else { return rpm }
        let minRPM = allowBelowMin ? 0 : Int(fan.minSpeed)
        let maxRPM = Int(fan.maxSpeed)
        return min(max(rpm, minRPM), maxRPM)
    }

    private func isFanOff(fanId: Int, fan: FanInfo) -> Bool {
        if lastTargetRPM[fanId] == 0 { return true }
        if fanOffEnteredAt[fanId] != nil { return true }
        return fan.currentSpeed <= 50
    }

    private func minimumRunningRPM(for fan: FanInfo) -> Int {
        max(1, Int(fan.minSpeed))
    }

    private func recordFanTransition(fanId: Int, previousRPM: Int?, rpm: Int) {
        let now = Date()
        if rpm == 0 {
            if fanOffEnteredAt[fanId] == nil {
                fanOffEnteredAt[fanId] = now
            }
            fanStartedAt[fanId] = nil
        } else if previousRPM == 0 || fanOffEnteredAt[fanId] != nil {
            fanStartedAt[fanId] = now
            fanOffEnteredAt[fanId] = nil
        }
    }

    private var defaultSensorKey: String {
        sensorManager.preferredCurveSensorKey
    }

    // MARK: - Persistence

    private var configURL: URL {
        let dir = Self.configHomeDirectory
            .appendingPathComponent(".config/fan-control", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("config.json")
    }

    private static var configHomeDirectory: URL {
        if geteuid() == 0,
           let sudoUser = ProcessInfo.processInfo.environment["SUDO_USER"],
           sudoUser != "root",
           let passwd = getpwnam(sudoUser) {
            return URL(fileURLWithPath: String(cString: passwd.pointee.pw_dir), isDirectory: true)
        }
        return FileManager.default.homeDirectoryForCurrentUser
    }

    func saveConfig() {
        configSaveTimer?.invalidate()
        let timer = Timer(timeInterval: configSaveDelay, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.configSaveTimer = nil
            let states = self.fanStates
            let destination = self.configURL
            self.configPersistenceQueue.async {
                Self.writeConfig(states, to: destination)
            }
        }
        configSaveTimer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    private func flushPendingConfigSave() {
        configSaveTimer?.invalidate()
        configSaveTimer = nil
        let states = fanStates
        let destination = configURL
        configPersistenceQueue.sync {
            Self.writeConfig(states, to: destination)
        }
    }

    private static func writeConfig(_ states: [FanState], to destination: URL) {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .prettyPrinted
        if let data = try? encoder.encode(states) {
            try? data.write(to: destination, options: .atomic)
        }
    }

    func loadConfig() {
        guard let data = try? Data(contentsOf: configURL),
              let loaded = try? JSONDecoder().decode([FanState].self, from: data) else { return }

        var migratedStates = loaded
        for index in migratedStates.indices {
            guard let config = migratedStates[index].curveConfig,
                  let migrated = config.migratedLegacyDefault() else {
                continue
            }
            migratedStates[index].curveConfig = migrated
            if case .curve = migratedStates[index].mode {
                migratedStates[index].mode = .curve(configId: migrated.id)
            }
            needsConfigMigrationWrite = true
        }

        loadedStates = migratedStates
        for saved in migratedStates {
            if let idx = fanStates.firstIndex(where: { $0.fanId == saved.fanId }) {
                fanStates[idx].mode = saved.mode
                fanStates[idx].curveConfig = saved.curveConfig
            }
        }
    }
}
