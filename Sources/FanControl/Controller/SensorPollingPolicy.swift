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
            // Thermal Demand already filters the source over 30 and 90 seconds.
            // Three-second background samples preserve control fidelity while
            // reducing full SMC snapshots by one third compared with 2 seconds.
            return 3
        }
    }
}
