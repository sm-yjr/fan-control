import Foundation

enum FanPollingActivity {
    case automatic
    case manual
    case curve
}

enum SensorPollingPolicy {
    static func interval(
        isPopoverPresented: Bool,
        thermalPressure: SystemThermalPressure,
        hottestSiliconTemperature: Double,
        activity: FanPollingActivity
    ) -> TimeInterval {
        if isPopoverPresented
            || thermalPressure >= .serious
            || hottestSiliconTemperature >= 90 {
            return 2
        }

        switch activity {
        case .automatic:
            return 8
        case .manual:
            return 5
        case .curve:
            return 2
        }
    }
}
