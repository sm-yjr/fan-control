import Foundation

enum SystemThermalPressure: Int, Comparable {
    case nominal
    case fair
    case serious
    case critical

    static func < (lhs: Self, rhs: Self) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

enum FanControlMode: Equatable {
    case automatic
    case manual(rpm: Int)
    case curve(configId: UUID)
}

@main
enum PerformancePolicyChecks {
    static func main() {
        checkPolling()
        checkCurveWrites()
        print("Performance policy checks passed")
    }

    private static func checkPolling() {
        require(
            SensorPollingPolicy.interval(
                isPopoverPresented: false,
                thermalPressure: .nominal,
                hottestSiliconTemperature: 55,
                activity: .curve
            ) == 3,
            "curve polling should use background cadence"
        )
        require(
            SensorPollingPolicy.interval(
                isPopoverPresented: true,
                thermalPressure: .nominal,
                hottestSiliconTemperature: 55,
                activity: .curve
            ) == 2,
            "visible UI should restore responsiveness"
        )
    }

    private static func checkCurveWrites() {
        let mode = FanControlMode.curve(configId: UUID())
        let merged = FanSpeedWritePolicy.targetRPM(
            requestedRPM: 2480,
            previousRPM: 2300,
            elapsed: 3,
            controlMode: mode,
            maximumRampUpPerSecond: 350,
            maximumRampDownPerSecond: 250,
            bypassRampLimit: false,
            preservesStartFromStopped: false
        )
        require(merged == 2300, "small curve changes should be coalesced")

        let largeChange = FanSpeedWritePolicy.targetRPM(
            requestedRPM: 2600,
            previousRPM: 2300,
            elapsed: 3,
            controlMode: mode,
            maximumRampUpPerSecond: 350,
            maximumRampDownPerSecond: 250,
            bypassRampLimit: false,
            preservesStartFromStopped: false
        )
        require(largeChange == 2600, "large curve changes should remain responsive")
    }

    private static func require(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else { fatalError(message) }
    }
}
