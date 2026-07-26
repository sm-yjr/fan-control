import Foundation

struct CurvePoint: Codable, Identifiable, Equatable {
    var id = UUID()
    var temperature: Double
    var fanSpeed: Double
}

enum FanControlMode: Codable, Equatable {
    case automatic
    case manual(rpm: Int)
    case curve(configId: UUID)
}

struct FanCurveConfig: Codable, Identifiable, Equatable {
    var id = UUID()
    var name: String
    var sensorKey: String
    var points: [CurvePoint]
    var hysteresis: Double = 3.0
    var presetVersion: Int?

    static let fanOffSpeed: Double = -20
    static let currentPresetVersion = 2

    static func defaultCurve(sensorKey: String) -> FanCurveConfig {
        if CurveInput.isThermalDemand(sensorKey) {
            return FanCurveConfig(
                name: "Balanced Thermal",
                sensorKey: sensorKey,
                points: [
                    CurvePoint(temperature: 0, fanSpeed: FanCurveConfig.fanOffSpeed),
                    CurvePoint(temperature: 18, fanSpeed: FanCurveConfig.fanOffSpeed),
                    CurvePoint(temperature: 28, fanSpeed: 0),
                    CurvePoint(temperature: 45, fanSpeed: 12),
                    CurvePoint(temperature: 60, fanSpeed: 25),
                    CurvePoint(temperature: 75, fanSpeed: 45),
                    CurvePoint(temperature: 88, fanSpeed: 70),
                    CurvePoint(temperature: 100, fanSpeed: 100),
                ],
                hysteresis: 8,
                presetVersion: currentPresetVersion
            )
        }

        return FanCurveConfig(
            name: "Balanced Temperature",
            sensorKey: sensorKey,
            points: [
                CurvePoint(temperature: 35, fanSpeed: FanCurveConfig.fanOffSpeed),
                CurvePoint(temperature: 45, fanSpeed: FanCurveConfig.fanOffSpeed),
                CurvePoint(temperature: 55, fanSpeed: 0),
                CurvePoint(temperature: 65, fanSpeed: 15),
                CurvePoint(temperature: 75, fanSpeed: 35),
                CurvePoint(temperature: 85, fanSpeed: 65),
                CurvePoint(temperature: 95, fanSpeed: 100),
            ],
            hysteresis: 4,
            presetVersion: currentPresetVersion
        )
    }

    mutating func setSensorKey(_ newSensorKey: String) {
        guard newSensorKey != sensorKey else { return }
        if CurveInput.isThermalDemand(newSensorKey) != CurveInput.isThermalDemand(sensorKey) {
            let existingID = id
            self = Self.defaultCurve(sensorKey: newSensorKey)
            id = existingID
        } else {
            sensorKey = newSensorKey
        }
    }

    func migratedLegacyDefault() -> FanCurveConfig? {
        guard sensorKey == "Average CPU",
              name == "Default",
              abs(hysteresis - 3) < 0.001,
              pointsMatch(Self.legacyDefaultPoints) else {
            return nil
        }

        var migrated = Self.defaultCurve(sensorKey: CurveInput.thermalDemandKey)
        migrated.id = id
        return migrated
    }

    func interpolate(temperature: Double) -> Double {
        let sorted = points.sorted { $0.temperature < $1.temperature }
        guard sorted.count >= 2 else { return 100 }

        if temperature <= sorted.first!.temperature {
            return sorted.first!.fanSpeed
        }
        if temperature >= sorted.last!.temperature {
            return sorted.last!.fanSpeed
        }

        for i in 0..<(sorted.count - 1) {
            let lo = sorted[i]
            let hi = sorted[i + 1]
            if temperature >= lo.temperature && temperature <= hi.temperature {
                let ratio = (temperature - lo.temperature) / (hi.temperature - lo.temperature)
                return lo.fanSpeed + ratio * (hi.fanSpeed - lo.fanSpeed)
            }
        }
        return 100
    }

    func interpolateWithHysteresis(temperature: Double, lastSpeed: Double, isRising: Bool) -> Double {
        if isRising {
            return interpolate(temperature: temperature)
        } else {
            let shifted = interpolate(temperature: temperature + hysteresis)
            return min(shifted, lastSpeed)
        }
    }

    static func isFanOffSpeed(_ speed: Double) -> Bool {
        speed < 0
    }

    private func pointsMatch(_ expected: [(Double, Double)]) -> Bool {
        let sorted = points.sorted { $0.temperature < $1.temperature }
        guard sorted.count == expected.count else { return false }
        return zip(sorted, expected).allSatisfy { point, expectedPoint in
            abs(point.temperature - expectedPoint.0) < 0.001
                && abs(point.fanSpeed - expectedPoint.1) < 0.001
        }
    }

    private static let legacyDefaultPoints: [(Double, Double)] = [
        (35, fanOffSpeed),
        (42, fanOffSpeed),
        (50, 0),
        (60, 25),
        (70, 45),
        (80, 70),
        (90, 100),
    ]
}
