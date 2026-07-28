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
        checkFanSpeedPresentation()
        checkReduceMotionPolicy()
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

    private static func checkReduceMotionPolicy() {
        require(
            FanUIMotion.resolvedDuration(0.8, reduceMotion: false) == 0.8,
            "standard animation duration changed"
        )
        require(
            FanUIMotion.resolvedDuration(0.8, reduceMotion: true) == 0,
            "Reduce Motion must remove nonessential animation"
        )
        require(
            FanUIMotion.resolvedDuration(nil, reduceMotion: false) == nil,
            "missing animations must stay disabled"
        )
        require(
            FanUIMotion.resolvedDuration(
                0.8,
                reduceMotion: false,
                allowsAnimation: false
            ) == nil,
            "hidden status UI must not keep a continuous animation alive"
        )
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
