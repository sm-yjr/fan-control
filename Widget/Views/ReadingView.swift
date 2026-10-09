import SwiftUI
import WidgetKit

struct ReadingView: View {
    @Environment(\.widgetFamily) private var systemFamily
    var familyOverride: WidgetFamily? = nil
    private var family: WidgetFamily { familyOverride ?? systemFamily }
    let date: Date
    let reading: Reading?
    private var status: ReadingStatus { reading?.status(at: date) ?? .unknown }
    private var recent: Bool { status == .recent }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 5) {
                Image(systemName: "fan.fill").foregroundStyle(.blue)
                Text("Fan Control").font(.caption.weight(.semibold))
                Spacer(minLength: 2)
                if family == .systemMedium { Text("Readings").font(.caption2).foregroundStyle(.secondary) }
            }
            if family == .systemMedium {
                HStack(alignment: .center, spacing: 16) {
                    VStack(alignment: .leading, spacing: 5) {
                        temperature
                        TemperatureTrend(points: reading?.history ?? [], date: date)
                            .frame(height: 34)
                        Text("Past hour · CPU").font(.system(size: 9)).foregroundStyle(.secondary)
                    }.frame(maxWidth: .infinity, alignment: .leading)
                    fanGauges.frame(maxWidth: .infinity)
                }
            } else {
                temperature
                compactFans
            }
            Spacer(minLength: 0)
            footer
        }.font(.caption).lineLimit(1).minimumScaleFactor(0.8)
    }
    private var temperature: some View {
        VStack(alignment: .leading, spacing: 1) {
            Text("CPU temperature").font(.system(size: 10)).foregroundStyle(.secondary)
            Text(reading?.temperature.map { String(format: "%.0f°", $0) } ?? "—")
                .font(.system(size: family == .systemSmall ? 34 : 31, weight: .medium, design: .rounded))
                .monospacedDigit().foregroundStyle(recent ? Color.primary : Color.secondary)
                .accessibilityLabel(Text("CPU temperature"))
                .accessibilityValue(reading?.temperature.map { Text(String(format: "%.0f °C", $0)) } ?? Text("Unavailable"))
        }
    }
    @ViewBuilder private var compactFans: some View {
        if let reading, reading.fanCount == 0 { Text("No fans").foregroundStyle(.secondary) }
        else if let reading, !reading.fans.isEmpty {
            HStack(spacing: 8) {
                ForEach(Array(reading.fans.prefix(2))) { fan in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(reading.fans.count == 1 ? String(localized: "Fan") : String(localized: "Fan") + " \(fan.id + 1)").font(.system(size: 9)).foregroundStyle(.secondary)
                        Text(fan.rpm.map { String(format: "%.0f", $0) } ?? "—").monospacedDigit().font(.system(size: 14, weight: .semibold)).foregroundStyle(recent ? Color.primary : Color.secondary)
                    }
                }
                Text("RPM").font(.system(size: 9)).foregroundStyle(.secondary)
            }
        } else { Text("Fan readings unavailable").font(.system(size: 10)).foregroundStyle(.secondary) }
    }
    @ViewBuilder private var fanGauges: some View {
        if let reading, reading.fanCount == 0 { Label("No fans", systemImage: "fan").foregroundStyle(.secondary) }
        else if let reading, !reading.fans.isEmpty {
            HStack(spacing: 8) {
                ForEach(Array(reading.fans.prefix(2))) { fan in FanDial(fan: fan, recent: recent) }
            }
        } else { Label("No readings", systemImage: "fan").foregroundStyle(.secondary) }
    }
    private var footer: some View {
        VStack(alignment: .leading, spacing: 2) {
            switch status {
            case .recent: Text("Recent sample · macOS refresh").foregroundStyle(.secondary)
            case .stale: Text("Outdated · open app").foregroundStyle(.orange)
            case .stopped: Text("Collection stopped").foregroundStyle(.secondary)
            case .sleeping: Text("Collection paused for sleep").foregroundStyle(.secondary)
            case .unknown: Text("Open app for readings").foregroundStyle(.secondary)
            }
            if let sample = reading?.sampledAt {
                HStack(spacing: 3) {
                    Text("Sampled")
                    Text(Date(timeIntervalSince1970: sample), style: .time)
                }.foregroundStyle(.secondary)
            }
        }.font(.system(size: 9))
    }
}

struct FanDial: View {
    let fan: FanReading
    let recent: Bool
    var body: some View {
        VStack(spacing: 4) {
            ZStack {
                Circle().trim(from: 0.12, to: 0.88).stroke(Color.secondary.opacity(0.16), style: StrokeStyle(lineWidth: 5, lineCap: .round)).rotationEffect(.degrees(90))
                if let fraction = fan.fraction {
                    Circle().trim(from: 0.12, to: 0.12 + 0.76 * fraction).stroke(recent ? Color.blue : Color.secondary, style: StrokeStyle(lineWidth: 5, lineCap: .round)).rotationEffect(.degrees(90))
                }
                VStack(spacing: 1) {
                    Text(fan.rpm.map { String(format: "%.0f", $0) } ?? "—").font(.system(size: 15, weight: .semibold, design: .rounded)).monospacedDigit()
                    Text("RPM").font(.system(size: 8)).foregroundStyle(.secondary)
                }
            }.frame(width: 64, height: 64)
            Text(String(localized: "Fan") + " \(fan.id + 1)").font(.system(size: 9)).foregroundStyle(.secondary)
            if fan.fraction == nil { Text("Range unknown").font(.system(size: 8)).foregroundStyle(.secondary) }
        }.accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(fan.name))
            .accessibilityValue(fan.rpm.map { Text(String(format: "%.0f RPM", $0)) } ?? Text("Unavailable"))
    }
}
