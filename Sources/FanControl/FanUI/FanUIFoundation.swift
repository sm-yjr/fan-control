import Foundation

/// Semantic design tokens and presentation values shared by FanUI components.
///
/// The types in this file deliberately avoid importing SwiftUI so their
/// behavior can be checked with the standalone Command Line Tools.
enum FanUISpacing: Double, CaseIterable {
    case hairline = 2
    case xSmall = 4
    case small = 6
    case medium = 8
    case large = 12
    case xLarge = 16
    case xxLarge = 20

    var points: CGFloat { CGFloat(rawValue) }
}

enum FanUICornerRadius: Double, CaseIterable {
    case track = 2
    case badge = 4
    case control = 6
    case panel = 8

    var points: CGFloat { CGFloat(rawValue) }
}

enum FanUIFontScale: Equatable {
    case body
    case callout
    case caption
    case caption2
    case headline
}

enum FanUIFontWeight: Equatable {
    case regular
    case medium
    case semibold
}

enum FanUIFontDesign: Equatable {
    case standard
    case monospaced
}

struct FanUIFontSpec: Equatable {
    let scale: FanUIFontScale
    let weight: FanUIFontWeight
    let design: FanUIFontDesign
}

enum FanUITextStyle: CaseIterable {
    case body
    case label
    case metric
    case metadata
    case statusValue
    case sectionHeading

    var spec: FanUIFontSpec {
        switch self {
        case .body:
            FanUIFontSpec(scale: .body, weight: .regular, design: .standard)
        case .label:
            FanUIFontSpec(scale: .caption, weight: .regular, design: .standard)
        case .metric:
            FanUIFontSpec(scale: .caption, weight: .medium, design: .monospaced)
        case .metadata:
            FanUIFontSpec(scale: .caption2, weight: .regular, design: .standard)
        case .statusValue:
            FanUIFontSpec(scale: .caption, weight: .regular, design: .monospaced)
        case .sectionHeading:
            FanUIFontSpec(scale: .caption2, weight: .semibold, design: .standard)
        }
    }
}

enum FanUIColorRole: CaseIterable {
    case background
    case surface
    case subtleSurface
    case border
    case primaryText
    case secondaryText
    case accent
    case success
    case caution
    case warning
    case critical
}

enum FanUITone: Equatable {
    case neutral
    case accent
    case success
    case caution
    case warning
    case critical
}

struct FanMetricPresentation: Equatable {
    let valueText: String
    let accessibilityValue: String
    let tone: FanUITone

    static func temperatureSummary(_ value: Double) -> FanMetricPresentation {
        temperature(
            value,
            normalTone: .neutral,
            elevatedTone: .neutral
        )
    }

    static func temperatureSensor(_ value: Double) -> FanMetricPresentation {
        temperature(
            value,
            normalTone: .success,
            elevatedTone: .caution
        )
    }

    static func thermalLoad(_ value: Double) -> FanMetricPresentation {
        guard value.isFinite else { return unavailable }

        let tone: FanUITone
        if value >= 90 {
            tone = .critical
        } else if value >= 70 {
            tone = .warning
        } else {
            tone = .neutral
        }

        return FanMetricPresentation(
            valueText: String(format: "%.0f%%", value),
            accessibilityValue: String(format: "%.0f percent", value),
            tone: tone
        )
    }

    static func batteryLevel(
        percent: Double,
        isCharging: Bool
    ) -> FanMetricPresentation {
        guard percent.isFinite else { return unavailable }

        let clamped = min(max(percent, 0), 100)
        let tone: FanUITone
        if isCharging {
            tone = .success
        } else if clamped <= 10 {
            tone = .critical
        } else if clamped <= 20 {
            tone = .caution
        } else {
            tone = .neutral
        }

        return FanMetricPresentation(
            valueText: String(format: "%.0f%%", clamped),
            accessibilityValue: String(
                format: isCharging
                    ? "%.0f percent, charging"
                    : "%.0f percent",
                clamped
            ),
            tone: tone
        )
    }

    static func batteryPower(watts: Double) -> FanMetricPresentation {
        guard watts.isFinite else { return unavailable }

        if abs(watts) < 0.05 {
            return FanMetricPresentation(
                valueText: "0.0 W",
                accessibilityValue: "0 watts",
                tone: .neutral
            )
        }

        let charging = watts > 0
        return FanMetricPresentation(
            valueText: String(format: "%+.1f W", watts),
            accessibilityValue: String(
                format: charging
                    ? "%.1f watts charging"
                    : "%.1f watts discharging",
                abs(watts)
            ),
            tone: charging ? .success : .neutral
        )
    }

    static func adapterPower(watts: Double) -> FanMetricPresentation {
        guard watts.isFinite, watts > 0 else { return unavailable }

        return FanMetricPresentation(
            valueText: String(format: "%.0f W", watts),
            accessibilityValue: String(format: "%.0f watts", watts),
            tone: .neutral
        )
    }

    private static func temperature(
        _ value: Double,
        normalTone: FanUITone,
        elevatedTone: FanUITone
    ) -> FanMetricPresentation {
        guard value.isFinite else { return unavailable }

        let tone: FanUITone
        if value > 90 {
            tone = .critical
        } else if value > 75 {
            tone = .warning
        } else if value > 60 {
            tone = elevatedTone
        } else {
            tone = normalTone
        }

        return FanMetricPresentation(
            valueText: String(format: "%.0f°C", value),
            accessibilityValue: String(
                format: "%.0f degrees Celsius",
                value
            ),
            tone: tone
        )
    }

    private static let unavailable = FanMetricPresentation(
        valueText: "—",
        accessibilityValue: "Unavailable",
        tone: .neutral
    )
}

struct FanProgressPresentation: Equatable {
    let fraction: Double
    let valueText: String
    let accessibilityLabel: String
    let accessibilityValue: String
    let tone: FanUITone

    static func fanSpeed(
        currentRPM: Double,
        minimumRPM: Double,
        maximumRPM: Double,
        fanName: String
    ) -> FanProgressPresentation {
        let validBounds = minimumRPM.isFinite
            && maximumRPM.isFinite
            && maximumRPM > minimumRPM
        let validCurrent = currentRPM.isFinite ? max(0, currentRPM) : 0
        let fraction = validBounds
            ? min(max(
                (validCurrent - minimumRPM)
                    / (maximumRPM - minimumRPM),
                0
            ), 1)
            : 0
        let percent = fraction * 100

        let tone: FanUITone
        if percent > 80 {
            tone = .critical
        } else if percent > 50 {
            tone = .warning
        } else {
            tone = .accent
        }

        return FanProgressPresentation(
            fraction: fraction,
            valueText: String(format: "%.0f%%", percent),
            accessibilityLabel: "\(fanName) speed",
            accessibilityValue: "\(formattedInteger(Int(validCurrent))) RPM, "
                + "\(Int(percent.rounded())) percent",
            tone: tone
        )
    }

    private static func formattedInteger(_ value: Int) -> String {
        let digits = String(abs(value))
        var reversed = ""
        for (offset, character) in digits.reversed().enumerated() {
            if offset > 0 && offset.isMultiple(of: 3) {
                reversed.append(",")
            }
            reversed.append(character)
        }
        let grouped = String(reversed.reversed())
        return value < 0 ? "-\(grouped)" : grouped
    }
}

struct FanUpdateEntryPresentation: Equatable {
    let isVisible: Bool
    let isEnabled: Bool
    let help: String

    static func resolve(
        updaterAvailable: Bool
    ) -> FanUpdateEntryPresentation {
        FanUpdateEntryPresentation(
            isVisible: true,
            isEnabled: updaterAvailable,
            help: updaterAvailable
                ? "Check for a newer version"
                : "Update checking is unavailable in this build"
        )
    }
}
