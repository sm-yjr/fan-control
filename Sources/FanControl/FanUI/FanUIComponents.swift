import AppKit
import SwiftUI

extension FanUIColorRole {
    var nsColor: NSColor {
        switch self {
        case .background:
            .windowBackgroundColor
        case .surface, .subtleSurface:
            .controlBackgroundColor
        case .border:
            .separatorColor
        case .primaryText:
            .labelColor
        case .secondaryText:
            .secondaryLabelColor
        case .accent:
            .controlAccentColor
        case .success:
            .systemGreen
        case .caution:
            .systemYellow
        case .warning:
            .systemOrange
        case .critical:
            .systemRed
        }
    }

    var color: Color {
        switch self {
        case .subtleSurface:
            Color.primary.opacity(0.045)
        default:
            Color(nsColor: nsColor)
        }
    }
}

extension FanUITone {
    var color: Color {
        switch self {
        case .neutral:
            FanUIColorRole.primaryText.color
        case .accent:
            FanUIColorRole.accent.color
        case .success:
            FanUIColorRole.success.color
        case .caution:
            FanUIColorRole.caution.color
        case .warning:
            FanUIColorRole.warning.color
        case .critical:
            FanUIColorRole.critical.color
        }
    }
}

extension FanUITextStyle {
    var font: Font {
        let scale: Font.TextStyle = switch spec.scale {
        case .body: .body
        case .callout: .callout
        case .caption: .caption
        case .caption2: .caption2
        case .headline: .headline
        }
        let weight: Font.Weight = switch spec.weight {
        case .regular: .regular
        case .medium: .medium
        case .semibold: .semibold
        }
        let design: Font.Design = switch spec.design {
        case .standard: .default
        case .monospaced: .monospaced
        }
        return .system(scale, design: design, weight: weight)
    }
}

private struct FanUISurfaceModifier: ViewModifier {
    let radius: FanUICornerRadius

    func body(content: Content) -> some View {
        content
            .background(FanUIColorRole.subtleSurface.color)
            .clipShape(
                RoundedRectangle(
                    cornerRadius: radius.points,
                    style: .continuous
                )
            )
            .overlay {
                RoundedRectangle(
                    cornerRadius: radius.points,
                    style: .continuous
                )
                .stroke(FanUIColorRole.border.color.opacity(0.65))
            }
    }
}

extension View {
    func fanUISurface(
        radius: FanUICornerRadius = .panel
    ) -> some View {
        modifier(FanUISurfaceModifier(radius: radius))
    }
}

struct FanCard<Content: View>: View {
    private let content: Content
    private let padding: FanUISpacing

    init(
        padding: FanUISpacing = .large,
        @ViewBuilder content: () -> Content
    ) {
        self.padding = padding
        self.content = content()
    }

    var body: some View {
        content
            .padding(padding.points)
            .fanUISurface()
    }
}

struct FanSection<Content: View>: View {
    private let content: Content
    private let verticalPadding: FanUISpacing

    init(
        verticalPadding: FanUISpacing = .medium,
        @ViewBuilder content: () -> Content
    ) {
        self.verticalPadding = verticalPadding
        self.content = content()
    }

    var body: some View {
        content
            .padding(.horizontal, FanUISpacing.large.points)
            .padding(.vertical, verticalPadding.points)
    }
}

struct FanMetricBadge: View {
    let label: String
    let presentation: FanMetricPresentation

    var body: some View {
        HStack(spacing: FanUISpacing.xSmall.points) {
            Text(label)
                .font(FanUITextStyle.label.font)
                .foregroundStyle(FanUIColorRole.secondaryText.color)
            Text(presentation.valueText)
                .font(FanUITextStyle.metric.font)
                .foregroundStyle(presentation.tone.color)
        }
        .padding(.horizontal, FanUISpacing.small.points)
        .padding(.vertical, FanUISpacing.hairline.points)
        .background(FanUIColorRole.subtleSurface.color)
        .clipShape(
            RoundedRectangle(
                cornerRadius: FanUICornerRadius.badge.points,
                style: .continuous
            )
        )
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityValue(presentation.accessibilityValue)
    }
}

struct FanModeSelector<SelectionValue: Hashable, Options: View>: View {
    let title: String
    @Binding private var selection: SelectionValue
    private let width: CGFloat
    private let options: Options

    init(
        _ title: String,
        selection: Binding<SelectionValue>,
        width: CGFloat = 220,
        @ViewBuilder options: () -> Options
    ) {
        self.title = title
        self._selection = selection
        self.width = width
        self.options = options()
    }

    var body: some View {
        Picker(title, selection: $selection) {
            options
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .frame(width: width)
        .accessibilityLabel(title)
    }
}

struct FanUpdateButton: View {
    let updaterAvailable: Bool
    let action: () -> Void

    @ViewBuilder
    var body: some View {
        let presentation = FanUpdateEntryPresentation.resolve(
            updaterAvailable: updaterAvailable
        )

        if presentation.isVisible {
            Button("Check for Updates", action: action)
                .buttonStyle(.borderless)
                .controlSize(.small)
                .disabled(!presentation.isEnabled)
                .help(presentation.help)
        }
    }
}

struct FanStatusRow: View {
    let title: String
    let value: String
    var systemImage: String?
    var tone: FanUITone = .neutral

    var body: some View {
        HStack(spacing: FanUISpacing.small.points) {
            if let systemImage {
                Image(systemName: systemImage)
                    .foregroundStyle(tone.color)
            }
            Text(title)
                .font(FanUITextStyle.label.font)
            Spacer(minLength: FanUISpacing.medium.points)
            Text(value)
                .font(FanUITextStyle.statusValue.font)
                .foregroundStyle(tone.color)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        .accessibilityValue(value)
    }
}

struct FanProgressStatusRow: View {
    let presentation: FanProgressPresentation

    var body: some View {
        HStack(spacing: FanUISpacing.small.points) {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    RoundedRectangle(
                        cornerRadius: FanUICornerRadius.track.points
                    )
                    .fill(FanUIColorRole.secondaryText.color.opacity(0.12))

                    RoundedRectangle(
                        cornerRadius: FanUICornerRadius.track.points
                    )
                    .fill(presentation.tone.color)
                    .frame(
                        width: geometry.size.width
                            * presentation.fraction
                    )
                }
            }
            .frame(height: 4)

            Text(presentation.valueText)
                .font(FanUITextStyle.metadata.font)
                .foregroundStyle(FanUIColorRole.secondaryText.color)
                .frame(width: 32, alignment: .trailing)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(presentation.accessibilityLabel)
        .accessibilityValue(presentation.accessibilityValue)
    }
}
