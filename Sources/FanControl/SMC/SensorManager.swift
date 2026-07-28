import Foundation

struct TemperatureSensor: Identifiable {
    let id: String
    let key: String
    let name: String
    let group: SensorGroup
    var value: Double = 0

    enum SensorGroup: String {
        case cpu = "CPU"
        case gpu = "GPU"
        case system = "System"
        case other = "Other"
    }
}

struct FanInfo: Identifiable {
    let id: Int
    let key: String
    var name: String
    var minSpeed: Double
    var maxSpeed: Double
    var currentSpeed: Double = 0
    var mode: FanMode = .automatic
}

private struct KnownSensor {
    let key: String
    let name: String
    let group: TemperatureSensor.SensorGroup
}

// M3 sensors confirmed from Stats app and SMC key scan
private let knownSensors: [KnownSensor] = [
    // CPU - M3
    KnownSensor(key: "Te05", name: "CPU E-core 1", group: .cpu),
    KnownSensor(key: "Te0L", name: "CPU E-core 2", group: .cpu),
    KnownSensor(key: "Te0P", name: "CPU E-core 3", group: .cpu),
    KnownSensor(key: "Te0S", name: "CPU E-core 4", group: .cpu),
    KnownSensor(key: "Tf04", name: "CPU P-core 1", group: .cpu),
    KnownSensor(key: "Tf09", name: "CPU P-core 2", group: .cpu),
    KnownSensor(key: "Tf0A", name: "CPU P-core 3", group: .cpu),
    KnownSensor(key: "Tf0B", name: "CPU P-core 4", group: .cpu),
    KnownSensor(key: "Tf0D", name: "CPU P-core 5", group: .cpu),
    KnownSensor(key: "Tf0E", name: "CPU P-core 6", group: .cpu),
    KnownSensor(key: "Tf44", name: "CPU P-core 7", group: .cpu),
    KnownSensor(key: "Tf49", name: "CPU P-core 8", group: .cpu),
    KnownSensor(key: "Tf4A", name: "CPU P-core 9", group: .cpu),
    KnownSensor(key: "Tf4B", name: "CPU P-core 10", group: .cpu),
    KnownSensor(key: "Tf4D", name: "CPU P-core 11", group: .cpu),
    KnownSensor(key: "Tf4E", name: "CPU P-core 12", group: .cpu),

    // M1/M2 CPU
    KnownSensor(key: "Tp09", name: "CPU E-core 1", group: .cpu),
    KnownSensor(key: "Tp0T", name: "CPU E-core 2", group: .cpu),
    KnownSensor(key: "Tp01", name: "CPU P-core 1", group: .cpu),
    KnownSensor(key: "Tp05", name: "CPU P-core 2", group: .cpu),
    KnownSensor(key: "Tp0D", name: "CPU P-core 3", group: .cpu),
    KnownSensor(key: "Tp0H", name: "CPU P-core 4", group: .cpu),
    KnownSensor(key: "Tp0L", name: "CPU P-core 5", group: .cpu),
    KnownSensor(key: "Tp0P", name: "CPU P-core 6", group: .cpu),
    KnownSensor(key: "Tp0X", name: "CPU P-core 7", group: .cpu),
    KnownSensor(key: "Tp0b", name: "CPU P-core 8", group: .cpu),

    // M2 extra E-cores
    KnownSensor(key: "Tp1h", name: "CPU E-core 1", group: .cpu),
    KnownSensor(key: "Tp1t", name: "CPU E-core 2", group: .cpu),
    KnownSensor(key: "Tp1p", name: "CPU E-core 3", group: .cpu),
    KnownSensor(key: "Tp1l", name: "CPU E-core 4", group: .cpu),

    // GPU - M3
    KnownSensor(key: "Tf14", name: "GPU 1", group: .gpu),
    KnownSensor(key: "Tf18", name: "GPU 2", group: .gpu),
    KnownSensor(key: "Tf19", name: "GPU 3", group: .gpu),
    KnownSensor(key: "Tf1A", name: "GPU 4", group: .gpu),
    KnownSensor(key: "Tf24", name: "GPU 5", group: .gpu),
    KnownSensor(key: "Tf28", name: "GPU 6", group: .gpu),
    KnownSensor(key: "Tf29", name: "GPU 7", group: .gpu),
    KnownSensor(key: "Tf2A", name: "GPU 8", group: .gpu),

    // GPU - M1
    KnownSensor(key: "Tg05", name: "GPU 1", group: .gpu),
    KnownSensor(key: "Tg0D", name: "GPU 2", group: .gpu),
    KnownSensor(key: "Tg0L", name: "GPU 3", group: .gpu),
    KnownSensor(key: "Tg0T", name: "GPU 4", group: .gpu),

    // GPU - M2
    KnownSensor(key: "Tg0f", name: "GPU 1", group: .gpu),
    KnownSensor(key: "Tg0j", name: "GPU 2", group: .gpu),

    // Common
    KnownSensor(key: "TC0P", name: "CPU Proximity", group: .cpu),
    KnownSensor(key: "TC0D", name: "CPU Diode", group: .cpu),
    KnownSensor(key: "TCAD", name: "CPU Package", group: .cpu),
    KnownSensor(key: "TG0P", name: "GPU Proximity", group: .gpu),
    KnownSensor(key: "TG0D", name: "GPU Diode", group: .gpu),
    KnownSensor(key: "Tm0P", name: "Mainboard", group: .system),
    KnownSensor(key: "TaLP", name: "Airflow Left", group: .system),
    KnownSensor(key: "TaRF", name: "Airflow Right", group: .system),
    KnownSensor(key: "TH0x", name: "NAND", group: .system),
    KnownSensor(key: "TB1T", name: "Battery 1", group: .system),
    KnownSensor(key: "TB2T", name: "Battery 2", group: .system),
    KnownSensor(key: "TW0P", name: "Airport", group: .system),
]

@Observable
final class SensorManager {
    var fans: [FanInfo] = []
    var temperatures: [TemperatureSensor] = []
    var averageCPU: Double = 0
    var averageGPU: Double = 0
    var hottestCPU: Double = 0
    var hottestGPU: Double = 0
    var thermalDemand: Double = 0
    var sustainedSiliconTemperature: Double = 0
    var chassisTemperature: Double = 0
    var chassisRisePerMinute: Double = 0
    var systemThermalPressure: SystemThermalPressure = .nominal
    var thermalDemandUsesChassisSensor = false
    var thermalDemandAvailable = false

    private let smc = SMCKit.shared
    private var timer: Timer?
    private var pollingActive = false
    private var defaultPollingInterval: TimeInterval = 2
    private var pollingIntervalProvider: (() -> TimeInterval)?
    private var isRefreshInFlight = false
    private var didDiscoverSensors = false
    private var thermalDemandEstimator = ThermalDemandEstimator()
    private var lastThermalSampleUptime: TimeInterval?
    private var lastLoggedThermalDemandBucket: Int?
    var onSnapshotUpdated: (() -> Void)?

    init() {}

    var preferredCurveSensorKey: String {
        if thermalDemandAvailable { return CurveInput.thermalDemandKey }
        if averageCPU > 0 { return "Average CPU" }
        return temperatures.first?.key ?? ""
    }

    var hottestSiliconTemperature: Double {
        max(hottestCPU, hottestGPU)
    }

    func startPolling(
        interval: TimeInterval = 2.0,
        intervalProvider: (() -> TimeInterval)? = nil
    ) {
        timer?.invalidate()
        timer = nil
        pollingActive = true
        defaultPollingInterval = max(0.5, interval)
        pollingIntervalProvider = intervalProvider
        updateReadings()
    }

    func stopPolling() {
        pollingActive = false
        timer?.invalidate()
        timer = nil
    }

    func reschedulePolling() {
        guard pollingActive else { return }
        scheduleNextPoll()
    }

    private func scheduleNextPoll() {
        guard pollingActive else { return }
        timer?.invalidate()

        let interval = max(
            0.5,
            pollingIntervalProvider?() ?? defaultPollingInterval
        )
        let timer = Timer(timeInterval: interval, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.timer = nil
            self.updateReadings()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    private func discoverSensors() {
        guard !isRefreshInFlight else { return }
        isRefreshInFlight = true

        smc.performAsync { [weak self] smc in
            let snapshot = Self.discoverSnapshot(using: smc)
            DispatchQueue.main.async {
                guard let self else { return }
                self.apply(snapshot)
                self.didDiscoverSensors = true
                self.isRefreshInFlight = false
                self.onSnapshotUpdated?()
                self.scheduleNextPoll()
            }
        }
    }

    func updateReadings() {
        guard didDiscoverSensors else {
            discoverSensors()
            return
        }
        guard !isRefreshInFlight else { return }
        isRefreshInFlight = true

        let fanInputs = fans
        let temperatureInputs = temperatures

        smc.performAsync { [weak self] smc in
            let snapshot = Self.updateSnapshot(
                using: smc,
                fans: fanInputs,
                temperatures: temperatureInputs
            )
            DispatchQueue.main.async {
                guard let self else { return }
                self.apply(snapshot)
                self.isRefreshInFlight = false
                self.onSnapshotUpdated?()
                self.scheduleNextPoll()
            }
        }
    }

    private func apply(_ snapshot: SensorSnapshot) {
        fans = snapshot.fans
        temperatures = snapshot.temperatures
        averageCPU = snapshot.averageCPU
        averageGPU = snapshot.averageGPU
        hottestCPU = snapshot.hottestCPU
        hottestGPU = snapshot.hottestGPU

        let now = ProcessInfo.processInfo.systemUptime
        let elapsed = lastThermalSampleUptime.map { now - $0 } ?? 0
        lastThermalSampleUptime = now
        let pressure = SystemThermalPressure(ProcessInfo.processInfo.thermalState)
        let reading = thermalDemandEstimator.update(
            siliconTemperature: snapshot.siliconTemperature,
            chassisTemperature: snapshot.chassisTemperature,
            emergencySiliconTemperature: snapshot.emergencySiliconTemperature,
            pressure: pressure,
            elapsed: elapsed
        )
        thermalDemand = reading.demandPercent
        sustainedSiliconTemperature = reading.sustainedSiliconTemperature
        chassisTemperature = reading.chassisTemperature
        chassisRisePerMinute = reading.chassisRisePerMinute
        systemThermalPressure = reading.pressure
        thermalDemandUsesChassisSensor = reading.usesChassisSensor
        thermalDemandAvailable = snapshot.siliconTemperature != nil || snapshot.chassisTemperature != nil

        let demandBucket = Int(reading.demandPercent / 5) * 5
        if demandBucket != lastLoggedThermalDemandBucket {
            lastLoggedThermalDemandBucket = demandBucket
            debugLog(
                "[FanControl] thermalDemand percent=\(String(format: "%.1f", reading.demandPercent)) "
                    + "silicon=\(String(format: "%.1f", reading.sustainedSiliconTemperature)) "
                    + "chassis=\(String(format: "%.1f", reading.chassisTemperature)) "
                    + "risePerMinute=\(String(format: "%.2f", reading.chassisRisePerMinute)) "
                    + "pressure=\(reading.pressure.displayName) "
                    + "usesChassis=\(reading.usesChassisSensor)"
            )
        }
    }

    func curveInputValue(for sensorKey: String) -> Double? {
        switch sensorKey {
        case CurveInput.thermalDemandKey:
            thermalDemandAvailable ? thermalDemand : nil
        case "Average CPU":
            averageCPU > 0 ? averageCPU : nil
        case "Average GPU":
            averageGPU > 0 ? averageGPU : nil
        case "Hottest CPU":
            hottestCPU > 0 ? hottestCPU : nil
        case "Hottest GPU":
            hottestGPU > 0 ? hottestGPU : nil
        default:
            temperatures.first(where: { $0.key == sensorKey })?.value
        }
    }

    private static func discoverSnapshot(using smc: SMCKit) -> SensorSnapshot {
        let allKeys = Set(smc.getAllKeys())
        var fans: [FanInfo] = []
        var temperatures: [TemperatureSensor] = []

        // Discover fans
        if let count = smc.getValue("FNum") {
            for i in 0..<Int(count) {
                var name = smc.getStringValue("F\(i)ID")
                if name == nil && Int(count) == 2 {
                    name = i == 0 ? "Left Fan" : "Right Fan"
                }

                let modeKey = smc.fanModeKey(i)
                let modeRaw = Int(smc.getValue(modeKey) ?? 0)
                let mode: FanMode = modeRaw == 1 ? .forced : .automatic

                fans.append(FanInfo(
                    id: i,
                    key: "F\(i)Ac",
                    name: name ?? "Fan #\(i)",
                    minSpeed: smc.getValue("F\(i)Mn") ?? 0,
                    maxSpeed: smc.getValue("F\(i)Mx") ?? 1,
                    currentSpeed: smc.getValue("F\(i)Ac") ?? 0,
                    mode: mode
                ))
            }
        }

        // Discover temperature sensors — match known list against available keys
        for known in knownSensors {
            guard allKeys.contains(known.key) else { continue }
            if let val = smc.getValue(known.key), val > 0, val < 120 {
                temperatures.append(TemperatureSensor(
                    id: known.key,
                    key: known.key,
                    name: known.name,
                    group: known.group,
                    value: val
                ))
            }
        }

        // Pick up unknown T-keys
        let knownKeys = Set(knownSensors.map(\.key))
        for key in allKeys where key.hasPrefix("T") && !knownKeys.contains(key) {
            if let val = smc.getValue(key), val > 0, val < 120 {
                temperatures.append(TemperatureSensor(
                    id: key,
                    key: key,
                    name: key,
                    group: .other,
                    value: val
                ))
            }
        }

        temperatures.sort { $0.name < $1.name }
        return makeSnapshot(fans: fans, temperatures: temperatures)
    }

    private static func updateSnapshot(
        using smc: SMCKit,
        fans fanInputs: [FanInfo],
        temperatures temperatureInputs: [TemperatureSensor]
    ) -> SensorSnapshot {
        var fans = fanInputs
        var temperatures = temperatureInputs

        for i in fans.indices {
            fans[i].currentSpeed = smc.getValue(fans[i].key) ?? 0
            let modeKey = smc.fanModeKey(fans[i].id)
            let modeRaw = Int(smc.getValue(modeKey) ?? 0)
            fans[i].mode = modeRaw == 1 ? .forced : .automatic
        }

        for i in temperatures.indices {
            if let val = smc.getValue(temperatures[i].key), val > 0, val < 120 {
                temperatures[i].value = val
            }
        }

        return makeSnapshot(fans: fans, temperatures: temperatures)
    }

    private static func makeSnapshot(
        fans: [FanInfo],
        temperatures: [TemperatureSensor]
    ) -> SensorSnapshot {
        let cpuTemps = temperatures.filter { $0.group == .cpu }.map(\.value)
        let gpuTemps = temperatures.filter { $0.group == .gpu }.map(\.value)

        var averageCPU: Double = 0
        var averageGPU: Double = 0
        var hottestCPU: Double = 0
        var hottestGPU: Double = 0

        if !cpuTemps.isEmpty {
            averageCPU = cpuTemps.reduce(0, +) / Double(cpuTemps.count)
            hottestCPU = cpuTemps.max() ?? 0
        }
        if !gpuTemps.isEmpty {
            averageGPU = gpuTemps.reduce(0, +) / Double(gpuTemps.count)
            hottestGPU = gpuTemps.max() ?? 0
        }

        let siliconTemperature = [averageCPU, averageGPU]
            .filter { $0 > 0 }
            .max()
        let emergencySiliconTemperature = [hottestCPU, hottestGPU]
            .filter { $0 > 0 }
            .max()
        let chassisValues = temperatures
            .filter {
                $0.group == .system
                    && $0.key != "TW0P"
            }
            .map(\.value)
        let chassisTemperature = ThermalDemandEstimator.representativeTemperature(chassisValues)

        return SensorSnapshot(
            fans: fans,
            temperatures: temperatures,
            averageCPU: averageCPU,
            averageGPU: averageGPU,
            hottestCPU: hottestCPU,
            hottestGPU: hottestGPU,
            siliconTemperature: siliconTemperature,
            emergencySiliconTemperature: emergencySiliconTemperature,
            chassisTemperature: chassisTemperature
        )
    }
}

private struct SensorSnapshot {
    let fans: [FanInfo]
    let temperatures: [TemperatureSensor]
    let averageCPU: Double
    let averageGPU: Double
    let hottestCPU: Double
    let hottestGPU: Double
    let siliconTemperature: Double?
    let emergencySiliconTemperature: Double?
    let chassisTemperature: Double?
}
