import SwiftUI
import WidgetKit

struct ReadingView: View {
    @Environment(\.widgetFamily) private var systemFamily
    var familyOverride: WidgetFamily? = nil
    var layoutAudit: ((DialGeometry) -> Void)? = nil
    private var compact: Bool { (familyOverride ?? systemFamily) == .systemSmall }
    let date: Date
    let reading: Reading?
    private var status: ReadingStatus { reading?.status(at: date) ?? .unknown }
    private var recent: Bool { status == .recent }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 1 : 6) {
            HStack(spacing: 6) {
                Image(systemName: "fan").font(.system(size: compact ? 13 : 16))
                Text("Fan Control").font(.system(size: compact ? 12 : 13, weight: .semibold))
                Spacer(minLength: 0)
                if !compact { Label("Readings", systemImage: "waveform.path").font(.system(size: 10)).foregroundStyle(.blue) }
            }
            if compact {
                HStack(alignment: .center, spacing: 7) {
                    temperature
                    TemperatureTrend(points: reading?.history ?? [], date: date)
                        .frame(maxWidth: .infinity).frame(height: 29)
                }
                Divider().opacity(0.45)
                fanGauges.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Divider().opacity(0.45)
                GeometryReader { geometry in
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            temperature
                            TemperatureTrend(points: reading?.history ?? [], date: date).frame(maxHeight: .infinity)
                            Text("Past 10 minutes").font(.system(size: 9)).foregroundStyle(.secondary)
                        }.frame(width: geometry.size.width * ((reading?.fans.count ?? 0) > 1 ? 0.30 : 0.42), height: geometry.size.height)
                        Divider().opacity(0.45)
                        fanGauges.frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                }.frame(maxHeight: .infinity)
            }
            footer
        }.lineLimit(1).minimumScaleFactor(0.8)
    }

    private var temperature: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 1) {
                Text(reading?.temperature.map { String(format: "%.0f", $0) } ?? "—")
                    .font(.system(size: compact ? 29 : 35, weight: .semibold, design: .rounded)).monospacedDigit()
                if reading?.temperature != nil { Text("°C").font(.system(size: compact ? 13 : 17, weight: .medium)) }
            }.foregroundStyle(recent ? Color.primary : Color.secondary)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("CPU temperature"))
                .accessibilityValue(reading?.temperature.map { Text(String(format: "%.0f °C", $0)) } ?? Text("Unavailable"))
            Text("CPU temperature").font(.system(size: compact ? 8 : 10)).foregroundStyle(.secondary)
        }.fixedSize(horizontal: true, vertical: false)
    }

    @ViewBuilder private var fanGauges: some View {
        if let reading, reading.fanCount == 0 {
            Label("No fans", systemImage: "fan").font(.caption).foregroundStyle(.secondary)
        } else if let reading, !reading.fans.isEmpty {
            HStack(spacing: compact ? 5 : 7) {
                ForEach(Array(reading.fans.prefix(2))) { fan in
                    FanDial(fan: fan, recent: recent, single: reading.fans.count == 1, compact: compact, audit: layoutAudit)
                    if fan.id == reading.fans.first?.id && reading.fans.count > 1 { Divider().opacity(0.35) }
                }
            }
        } else {
            Label("No readings", systemImage: "fan").font(.caption).foregroundStyle(.secondary)
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 1) {
            switch status {
            case .recent: EmptyView()
            case .stale: Text("Outdated").foregroundStyle(.orange)
            case .stopped: Text("Collection stopped").foregroundStyle(.secondary)
            case .sleeping: Text("Collection paused for sleep").foregroundStyle(.secondary)
            case .unknown: Text("Open app for readings").foregroundStyle(.secondary)
            }
            if let sample = reading?.sampledAt {
                HStack(spacing: 2) {
                    Text("Sampled")
                    Text(Date(timeIntervalSince1970: sample), style: .relative)
                    Text("ago")
                }.foregroundStyle(.secondary)
            }
        }.font(.system(size: compact ? 8 : 9))
    }

}

struct FanDial: View {
    let fan: FanReading
    let recent: Bool
    let single: Bool
    let compact: Bool
    var audit: ((DialGeometry) -> Void)? = nil
    var body: some View {
        GeometryReader { geometry in
            let height = geometry.size.height
            // Extra vertical room in the small dual layout belongs to the arc,
            // while its 14-point readout stays stable.
            let numberSize = min(single ? 27.0 : 23.0, height * 0.27, geometry.size.width / 3.4, compact && !single ? 14 : .infinity)
            FanDialLayout(hasRange: fan.fraction != nil, audit: audit) {
                Canvas { context, size in
                    // The layout proposes the measured arc diameter and stroke extent.
                    let radius = DialGeometry.radius(fittingWidth: size.width)
                    let thickness = DialGeometry.stroke(forRadius: radius)
                    guard radius > 0 else { return }
                    let center = CGPoint(x: size.width / 2, y: radius + thickness / 2)
                    let stroke = StrokeStyle(lineWidth: thickness, lineCap: .round)
                    var track = Path()
                    track.addArc(center: center, radius: radius, startAngle: .degrees(180), endAngle: .degrees(360), clockwise: false)
                    context.stroke(track, with: .color(Color.secondary.opacity(0.15)), style: stroke)
                    if let fraction = fan.fraction {
                        var progress = Path()
                        progress.addArc(center: center, radius: radius, startAngle: .degrees(180), endAngle: .degrees(180 + 180 * fraction), clockwise: false)
                        context.stroke(progress, with: .color(recent ? .blue : .secondary), style: stroke)
                    }
                }
                Image(systemName: "fan").resizable().scaledToFit().foregroundStyle(.secondary)
                    .frame(idealWidth: min(15, height * 0.13), idealHeight: min(15, height * 0.13)).clipped()
                ViewThatFits(in: .horizontal) {
                    Text(fan.rpm.map { $0.formatted(.number.precision(.fractionLength(0))) } ?? "—").fixedSize()
                    Text(fan.rpm.map { $0.formatted(.number.grouping(.never).precision(.fractionLength(0))) } ?? "—").fixedSize()
                }
                .font(.system(size: numberSize, weight: .semibold, design: .rounded)).monospacedDigit()
                .foregroundStyle(recent ? Color.primary : Color.secondary)
                .lineLimit(1).minimumScaleFactor(1)
                Text("RPM").font(.system(size: min(10, height * 0.13))).foregroundStyle(.secondary).fixedSize()
                Group {
                    if fan.rpm == nil { Text("Unavailable").font(.system(size: 8)).foregroundStyle(.secondary) }
                    else if fan.fraction == nil { Text("Range unknown").font(.system(size: 8)).foregroundStyle(.secondary) }
                    else { (Text("Fan") + Text(single ? "" : " \(fan.id + 1)")).font(.system(size: min(11, height * 0.16))).foregroundStyle(.secondary) }
                }.lineLimit(2).minimumScaleFactor(1).multilineTextAlignment(.center)
            }.frame(width: geometry.size.width, height: geometry.size.height)
        }.accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(fan.name))
            .accessibilityValue(fan.rpm.map { Text(String(format: "%.0f RPM", $0)) } ?? Text("Unavailable"))
    }
}

/// Measure real SwiftUI labels before fitting the semicircle around them.
private struct FanDialLayout: Layout {
    let hasRange: Bool
    var audit: ((DialGeometry) -> Void)?
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        proposal.replacingUnspecifiedDimensions()
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let textProposal = ProposedViewSize(width: bounds.width - 4, height: nil)
        let geometry = DialGeometry(size: bounds.size, value: subviews[2].sizeThatFits(textProposal), unit: subviews[3].sizeThatFits(.unspecified), caption: subviews[4].sizeThatFits(textProposal), symbol: subviews[1].sizeThatFits(.unspecified), hasRange: hasRange)
        audit?(geometry)
        let arcSize = CGSize(width: geometry.radius > 0 ? geometry.radius * 2 + geometry.stroke : 0, height: geometry.radius + geometry.stroke)
        subviews[0].place(at: CGPoint(x: bounds.midX, y: bounds.minY), anchor: .top, proposal: ProposedViewSize(arcSize))
        for (index, rect) in [(2, geometry.value), (3, geometry.unit), (4, geometry.caption)] {
            subviews[index].place(at: CGPoint(x: bounds.minX + rect.minX, y: bounds.minY + rect.minY), anchor: .topLeading, proposal: ProposedViewSize(rect.size))
        }
        if let rect = geometry.symbol {
            subviews[1].place(at: CGPoint(x: bounds.minX + rect.minX, y: bounds.minY + rect.minY), anchor: .topLeading, proposal: ProposedViewSize(rect.size))
        } else {
            subviews[1].place(at: CGPoint(x: bounds.midX, y: bounds.minY), anchor: .top, proposal: .zero)
        }
    }
}
