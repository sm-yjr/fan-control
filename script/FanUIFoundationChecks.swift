import Foundation

@main
enum FanUIFoundationChecks {
    static func main() {
        checkSpacingScale()
        checkCornerRadiusScale()
        checkTypographyRoles()
        checkSemanticColorCoverage()
        checkTemperaturePresentations()
        checkThermalLoadPresentations()
        checkBatteryPresentations()
        checkFanSpeedPresentation()
        checkUpdateEntryPresentation()
        print("FanUI foundation checks passed")
    }

    private static func checkSpacingScale() {
        let scale = FanUISpacing.allCases.map(\.points)
        require(
            scale == [2, 4, 6, 8, 12, 16, 20],
            "spacing tokens changed or are not ordered from compact to spacious"
        )
    }

    private static func checkCornerRadiusScale() {
        require(FanUICornerRadius.track.points == 2, "track radius changed")
        require(FanUICornerRadius.badge.points == 4, "badge radius changed")
        require(FanUICornerRadius.control.points == 6, "control radius changed")
        require(FanUICornerRadius.panel.points == 8, "panel radius changed")
    }

    private static func checkTypographyRoles() {
        require(
            FanUITextStyle.metric.spec.design == .monospaced,
            "metric text must keep tabular, stable-width glyphs"
        )
        require(
            FanUITextStyle.metric.spec.weight == .medium,
            "metric text must remain visually distinct from its label"
        )
        require(
            FanUITextStyle.sectionHeading.spec.weight == .semibold,
            "section headings must preserve hierarchy"
        )
        require(
            Set(FanUITextStyle.allCases).count == FanUITextStyle.allCases.count,
            "typography roles contain duplicates"
        )
    }

    private static func checkSemanticColorCoverage() {
        require(
            Set(FanUIColorRole.allCases) == [
                .background,
                .surface,
                .subtleSurface,
                .border,
                .primaryText,
                .secondaryText,
                .accent,
                .success,
                .caution,
                .warning,
                .critical,
            ],
            "semantic color roles are incomplete"
        )
    }

    private static func checkTemperaturePresentations() {
        let normal = FanMetricPresentation.temperatureSummary(68)
        require(normal.valueText == "68°C", "temperature formatting changed")
        require(normal.tone == .neutral, "normal temperature must use neutral emphasis")
        require(
            normal.accessibilityValue == "68 degrees Celsius",
            "temperature accessibility value changed"
        )

        require(
            FanMetricPresentation.temperatureSummary(76).tone == .warning,
            "warm temperature must use warning emphasis"
        )
        require(
            FanMetricPresentation.temperatureSummary(91).tone == .critical,
            "hot temperature must use critical emphasis"
        )

        require(
            FanMetricPresentation.temperatureSensor(61).tone == .caution,
            "elevated sensor temperature must remain visible"
        )
        require(
            FanMetricPresentation.temperatureSensor(55).tone == .success,
            "cool sensor temperature must use success emphasis"
        )
    }

    private static func checkThermalLoadPresentations() {
        let normal = FanMetricPresentation.thermalLoad(61)
        require(normal.valueText == "61%", "thermal-load formatting changed")
        require(normal.tone == .neutral, "normal thermal load must use neutral emphasis")
        require(
            normal.accessibilityValue == "61 percent",
            "thermal-load accessibility value changed"
        )
        require(
            FanMetricPresentation.thermalLoad(70).tone == .warning,
            "high thermal load must use warning emphasis"
        )
        require(
            FanMetricPresentation.thermalLoad(90).tone == .critical,
            "critical thermal load must use critical emphasis"
        )
    }

    private static func checkBatteryPresentations() {
        let normal = FanMetricPresentation.batteryLevel(
            percent: 85,
            isCharging: false
        )
        require(normal.valueText == "85%", "battery level formatting changed")
        require(normal.tone == .neutral, "healthy battery level must stay neutral")
        require(
            normal.accessibilityValue == "85 percent",
            "battery level accessibility value changed"
        )

        require(
            FanMetricPresentation.batteryLevel(percent: 18, isCharging: false).tone == .caution,
            "low battery must use caution emphasis"
        )
        require(
            FanMetricPresentation.batteryLevel(percent: 8, isCharging: false).tone == .critical,
            "nearly empty battery must use critical emphasis"
        )
        let charging = FanMetricPresentation.batteryLevel(
            percent: 50,
            isCharging: true
        )
        require(charging.tone == .success, "charging battery must use success emphasis")
        require(
            charging.accessibilityValue == "50 percent, charging",
            "charging battery accessibility value changed"
        )
        require(
            FanMetricPresentation.batteryLevel(percent: .nan, isCharging: false).valueText == "—",
            "invalid battery level must fail closed"
        )

        let chargingPower = FanMetricPresentation.batteryPower(watts: 32.46)
        require(chargingPower.valueText == "+32.5 W", "charging power formatting changed")
        require(chargingPower.tone == .success, "charging power must use success emphasis")
        require(
            chargingPower.accessibilityValue == "32.5 watts charging",
            "charging power accessibility value changed"
        )

        let dischargingPower = FanMetricPresentation.batteryPower(watts: -18.2)
        require(dischargingPower.valueText == "-18.2 W", "discharging power formatting changed")
        require(dischargingPower.tone == .neutral, "discharging power must stay neutral")
        require(
            dischargingPower.accessibilityValue == "18.2 watts discharging",
            "discharging power accessibility value changed"
        )

        require(
            FanMetricPresentation.batteryPower(watts: 0.01).valueText == "0.0 W",
            "near-zero battery power must display as zero without a sign"
        )
        require(
            FanMetricPresentation.batteryPower(watts: .nan).valueText == "—",
            "invalid battery power must fail closed"
        )

        let adapter = FanMetricPresentation.adapterPower(watts: 96)
        require(adapter.valueText == "96 W", "adapter power formatting changed")
        require(adapter.tone == .neutral, "adapter power must stay neutral")
        require(
            adapter.accessibilityValue == "96 watts",
            "adapter power accessibility value changed"
        )
        require(
            FanMetricPresentation.adapterPower(watts: 0).valueText == "—",
            "disconnected adapter power must fail closed"
        )
        require(
            FanMetricPresentation.adapterPower(watts: .nan).valueText == "—",
            "invalid adapter power must fail closed"
        )
    }

    private static func checkFanSpeedPresentation() {
        let stopped = FanProgressPresentation.fanSpeed(
            currentRPM: 0,
            minimumRPM: 2_000,
            maximumRPM: 6_000,
            fanName: "Left Fan"
        )
        require(stopped.fraction == 0, "stopped fan progress must be zero")
        require(stopped.valueText == "0%", "stopped fan percentage changed")

        let running = FanProgressPresentation.fanSpeed(
            currentRPM: 4_400,
            minimumRPM: 2_000,
            maximumRPM: 6_000,
            fanName: "Left Fan"
        )
        require(approximatelyEqual(running.fraction, 0.6), "fan progress calculation changed")
        require(running.valueText == "60%", "fan progress formatting changed")
        require(running.tone == .warning, "mid-high fan speed must use warning emphasis")
        require(
            running.accessibilityValue == "4,400 RPM, 60 percent",
            "fan progress accessibility value changed"
        )

        let invalid = FanProgressPresentation.fanSpeed(
            currentRPM: .nan,
            minimumRPM: 2_000,
            maximumRPM: 2_000,
            fanName: "Left Fan"
        )
        require(invalid.fraction == 0, "invalid fan bounds must fail closed")
    }

    private static func checkUpdateEntryPresentation() {
        let available = FanUpdateEntryPresentation.resolve(
            updaterAvailable: true
        )
        require(available.isVisible, "available updater entry was hidden")
        require(available.isEnabled, "available updater entry was disabled")
        require(
            available.help == "Check for a newer version",
            "available updater help changed"
        )

        let unavailable = FanUpdateEntryPresentation.resolve(
            updaterAvailable: false
        )
        require(
            unavailable.isVisible,
            "unavailable updater entry must remain visible"
        )
        require(
            !unavailable.isEnabled,
            "unavailable updater entry must not invoke an absent runtime"
        )
        require(
            unavailable.help.contains("unavailable"),
            "unavailable updater entry does not explain its state"
        )
    }

    private static func approximatelyEqual(_ lhs: Double, _ rhs: Double) -> Bool {
        abs(lhs - rhs) < 0.000_001
    }

    private static func require(
        _ condition: @autoclosure () -> Bool,
        _ message: String
    ) {
        guard condition() else {
            fputs("FanUI foundation check failed: \(message)\n", stderr)
            exit(1)
        }
    }
}
