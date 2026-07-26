import Foundation

enum CurveInput {
    static let thermalDemandKey = "Thermal Demand"

    static func isThermalDemand(_ sensorKey: String) -> Bool {
        sensorKey == thermalDemandKey
    }
}

enum SystemThermalPressure: Int, Codable, Comparable {
    case nominal
    case fair
    case serious
    case critical

    static func < (lhs: SystemThermalPressure, rhs: SystemThermalPressure) -> Bool {
        lhs.rawValue < rhs.rawValue
    }

    init(_ state: ProcessInfo.ThermalState) {
        switch state {
        case .nominal:
            self = .nominal
        case .fair:
            self = .fair
        case .serious:
            self = .serious
        case .critical:
            self = .critical
        @unknown default:
            self = .fair
        }
    }

    var displayName: String {
        switch self {
        case .nominal: "Nominal"
        case .fair: "Fair"
        case .serious: "Serious"
        case .critical: "Critical"
        }
    }
}

struct ThermalDemandReading: Equatable {
    var demandPercent: Double = 0
    var sustainedSiliconTemperature: Double = 0
    var chassisTemperature: Double = 0
    var chassisRisePerMinute: Double = 0
    var pressure: SystemThermalPressure = .nominal
    var usesChassisSensor = false
}

/// Estimates the heat that the enclosure must reject from temperature-only data.
///
/// The model has two thermal nodes:
/// - CPU/GPU temperature is the heat source and receives a 30-second low-pass filter.
/// - Mainboard, airflow, NAND, and battery sensors approximate stored enclosure heat
///   and receive a 90-second low-pass filter.
///
/// The public result is a dimensionless 0...100 demand value. A raw silicon
/// temperature only overrides the filters at the emergency end of the range.
struct ThermalDemandEstimator {
    private var sustainedSiliconTemperature: Double?
    private var filteredChassisTemperature: Double?

    mutating func reset() {
        sustainedSiliconTemperature = nil
        filteredChassisTemperature = nil
    }

    mutating func update(
        siliconTemperature: Double?,
        chassisTemperature: Double?,
        emergencySiliconTemperature: Double? = nil,
        pressure: SystemThermalPressure,
        elapsed: TimeInterval
    ) -> ThermalDemandReading {
        let validSilicon = siliconTemperature.flatMap(Self.validTemperature)
        let validChassis = chassisTemperature.flatMap(Self.validTemperature)
        let validEmergencySilicon = emergencySiliconTemperature.flatMap(Self.validTemperature)
            ?? validSilicon
        let shouldReseed = elapsed <= 0 || elapsed > 30
        let dt = min(max(elapsed, 0.5), 10)

        if shouldReseed {
            sustainedSiliconTemperature = validSilicon
            filteredChassisTemperature = validChassis
        }

        if let validSilicon {
            sustainedSiliconTemperature = Self.lowPass(
                previous: sustainedSiliconTemperature,
                sample: validSilicon,
                elapsed: dt,
                timeConstant: 30
            )
        }

        let previousChassis = filteredChassisTemperature
        if let validChassis {
            filteredChassisTemperature = Self.lowPass(
                previous: filteredChassisTemperature,
                sample: validChassis,
                elapsed: dt,
                timeConstant: 90
            )
        }

        let silicon = sustainedSiliconTemperature ?? 0
        let chassis = filteredChassisTemperature ?? 0
        let chassisRise: Double
        if shouldReseed {
            chassisRise = 0
        } else if let previousChassis, filteredChassisTemperature != nil {
            chassisRise = max(0, (chassis - previousChassis) * 60 / dt)
        } else {
            chassisRise = 0
        }

        let siliconLoad = Self.smoothStep(value: silicon, lower: 50, upper: 95)
        let chassisLoad = Self.smoothStep(value: chassis, lower: 32, upper: 55)
        let risingLoad = Self.smoothStep(value: chassisRise, lower: 0.2, upper: 2.0)

        var demand: Double
        if filteredChassisTemperature != nil {
            demand = 100 * (0.35 * siliconLoad + 0.55 * chassisLoad + 0.10 * risingLoad)
        } else {
            // Models without usable enclosure sensors still get a stable fallback.
            demand = 100 * siliconLoad
        }

        demand = max(demand, Self.pressureFloor(pressure))
        if let validEmergencySilicon, validEmergencySilicon >= 96 {
            let emergencyProgress = min(max((validEmergencySilicon - 96) / 9, 0), 1)
            demand = max(demand, 75 + 25 * emergencyProgress)
        }

        return ThermalDemandReading(
            demandPercent: min(max(demand, 0), 100),
            sustainedSiliconTemperature: silicon,
            chassisTemperature: chassis,
            chassisRisePerMinute: chassisRise,
            pressure: pressure,
            usesChassisSensor: filteredChassisTemperature != nil
        )
    }

    static func representativeTemperature(_ values: [Double]) -> Double? {
        let sorted = values.filter { validTemperature($0) != nil }.sorted()
        guard !sorted.isEmpty else { return nil }
        guard sorted.count > 1 else { return sorted[0] }

        let position = Double(sorted.count - 1) * 0.75
        let lowerIndex = Int(position.rounded(.down))
        let upperIndex = Int(position.rounded(.up))
        guard lowerIndex != upperIndex else { return sorted[lowerIndex] }

        let fraction = position - Double(lowerIndex)
        return sorted[lowerIndex] + fraction * (sorted[upperIndex] - sorted[lowerIndex])
    }

    private static func validTemperature(_ value: Double) -> Double? {
        value.isFinite && value > 0 && value < 120 ? value : nil
    }

    private static func lowPass(
        previous: Double?,
        sample: Double,
        elapsed: TimeInterval,
        timeConstant: TimeInterval
    ) -> Double {
        guard let previous else { return sample }
        let alpha = 1 - exp(-elapsed / timeConstant)
        return previous + alpha * (sample - previous)
    }

    private static func smoothStep(value: Double, lower: Double, upper: Double) -> Double {
        guard upper > lower else { return value >= upper ? 1 : 0 }
        let x = min(max((value - lower) / (upper - lower), 0), 1)
        return x * x * (3 - 2 * x)
    }

    private static func pressureFloor(_ pressure: SystemThermalPressure) -> Double {
        switch pressure {
        case .nominal: 0
        case .fair: 35
        case .serious: 75
        case .critical: 100
        }
    }
}
