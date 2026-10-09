import SwiftUI

struct TemperatureTrend: View {
    let points: [TemperaturePoint]
    let date: Date
    var body: some View {
        Canvas { context, size in
            let end = date.timeIntervalSince1970
            let visible = points.filter { $0.sampledAt <= end && $0.sampledAt >= end - 3600 }
            let values = visible.compactMap(\.temperature)
            guard let low = values.min(), let high = values.max(), values.count >= 2 else { return }
            let bottom = low - 3; let span = max(10, high - bottom + 3)
            var path = Path(); var previous: Double?
            for point in visible {
                guard let temperature = point.temperature else { previous = nil; continue }
                let location = CGPoint(x: (point.sampledAt - (end - 3600)) / 3600 * size.width, y: size.height - (temperature - bottom) / span * size.height)
                if let last = previous, point.sampledAt - last <= 90 { path.addLine(to: location) }
                else { path.move(to: location) }
                previous = point.sampledAt
            }
            context.stroke(path, with: .color(.blue), style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
        }.accessibilityLabel(Text("CPU temperature history, past hour"))
            .overlay(alignment: .center) {
                if points.compactMap(\.temperature).count < 2 { Text("Collecting history").font(.system(size: 9)).foregroundStyle(.secondary) }
            }
    }
}
