import AppKit
import SwiftUI

private enum PreviewMode: Hashable {
    case automatic
    case manual
    case curve
}

private struct FanUIContractView: View {
    var body: some View {
        VStack(spacing: FanUISpacing.medium.points) {
            FanMetricBadge(
                label: "Heat",
                presentation: .thermalLoad(61)
            )

            FanCard {
                FanStatusRow(
                    title: "System thermal pressure",
                    value: "Nominal",
                    systemImage: "thermometer.medium",
                    tone: .success
                )
            }

            FanModeSelector(
                "Control mode",
                selection: .constant(PreviewMode.automatic)
            ) {
                Text("Auto").tag(PreviewMode.automatic)
                Text("Manual").tag(PreviewMode.manual)
                Text("Curve").tag(PreviewMode.curve)
            }

            FanUpdateButton(updaterAvailable: true) {}
            FanUpdateButton(updaterAvailable: false) {}

            FanProgressStatusRow(
                presentation: .fanSpeed(
                    currentRPM: 2_800,
                    minimumRPM: 2_000,
                    maximumRPM: 6_000,
                    fanName: "Left Fan"
                )
            )
        }
        .padding(FanUISpacing.large.points)
        .background(FanUIColorRole.background.color)
    }
}

@main
enum FanUIComponentChecks {
    static func main() {
        checkAdaptiveSemanticColors()

        let hostingView = NSHostingView(rootView: FanUIContractView())
        hostingView.frame = NSRect(x: 0, y: 0, width: 420, height: 240)
        hostingView.layoutSubtreeIfNeeded()

        require(
            hostingView.fittingSize.width > 0
                && hostingView.fittingSize.height > 0,
            "FanUI component tree did not produce a measurable native view"
        )
        print("FanUI component checks passed")
    }

    private static func checkAdaptiveSemanticColors() {
        let light = resolvedRGBA(
            FanUIColorRole.background.nsColor,
            appearanceName: .aqua
        )
        let dark = resolvedRGBA(
            FanUIColorRole.background.nsColor,
            appearanceName: .darkAqua
        )

        require(
            light != dark,
            "semantic background did not adapt between Aqua and Dark Aqua"
        )
    }

    private static func resolvedRGBA(
        _ color: NSColor,
        appearanceName: NSAppearance.Name
    ) -> [CGFloat] {
        guard let appearance = NSAppearance(named: appearanceName) else {
            return []
        }

        var components: [CGFloat] = []
        appearance.performAsCurrentDrawingAppearance {
            guard let resolved = color.usingColorSpace(.deviceRGB) else {
                return
            }
            var red: CGFloat = 0
            var green: CGFloat = 0
            var blue: CGFloat = 0
            var alpha: CGFloat = 0
            resolved.getRed(
                &red,
                green: &green,
                blue: &blue,
                alpha: &alpha
            )
            components = [red, green, blue, alpha]
        }
        return components
    }

    private static func require(
        _ condition: @autoclosure () -> Bool,
        _ message: String
    ) {
        guard condition() else {
            fputs("FanUI component check failed: \(message)\n", stderr)
            exit(1)
        }
    }
}
