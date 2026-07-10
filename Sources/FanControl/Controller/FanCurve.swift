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

    static let fanOffSpeed: Double = -20

    static func defaultCurve(sensorKey: String) -> FanCurveConfig {
        FanCurveConfig(
            name: "Default",
            sensorKey: sensorKey,
            points: [
                CurvePoint(temperature: 35, fanSpeed: FanCurveConfig.fanOffSpeed),
                CurvePoint(temperature: 42, fanSpeed: FanCurveConfig.fanOffSpeed),
                CurvePoint(temperature: 50, fanSpeed: 0),
                CurvePoint(temperature: 60, fanSpeed: 25),
                CurvePoint(temperature: 70, fanSpeed: 45),
                CurvePoint(temperature: 80, fanSpeed: 70),
                CurvePoint(temperature: 90, fanSpeed: 100),
            ]
        )
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
}
