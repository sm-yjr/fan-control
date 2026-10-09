import SwiftUI

struct TemperatureTrend: View {
    let points: [TemperaturePoint]
    let date: Date
    private var visible: [TemperaturePoint] {
        let end = date.timeIntervalSince1970
        return points.filter { $0.sampledAt <= end && $0.sampledAt >= end - 600 }
    }
    var body: some View {
        Canvas { context, size in
            let end = date.timeIntervalSince1970
            let values = visible.compactMap(\.temperature)
            guard let low = values.min(), let high = values.max(), values.count >= 2 else { return }
            let bottom = low - 3; let span = max(10, high - bottom + 3)
            var line = Path(); var area = Path(); var previous: Double?; var lastX: Double?; var count = 0
            func finishArea() {
                guard let lastX, count >= 2 else { return }
                area.addLine(to: CGPoint(x: lastX, y: size.height)); area.closeSubpath()
                context.fill(area, with: .linearGradient(Gradient(colors: [Color.blue.opacity(0.18), Color.blue.opacity(0.03)]), startPoint: .zero, endPoint: CGPoint(x: 0, y: size.height)))
            }
            for point in visible {
                guard let temperature = point.temperature else { finishArea(); previous = nil; count = 0; area = Path(); continue }
                let location = CGPoint(x: (point.sampledAt - (end - 600)) / 600 * size.width, y: size.height - (temperature - bottom) / span * size.height)
                if let last = previous, point.sampledAt - last <= 90 {
                    line.addLine(to: location); area.addLine(to: location); count += 1
                } else {
                    finishArea(); line.move(to: location); area = Path()
                    area.move(to: CGPoint(x: location.x, y: size.height)); area.addLine(to: location); count = 1
                }
                previous = point.sampledAt; lastX = location.x
            }
            finishArea()
            context.stroke(line, with: .color(.blue), style: StrokeStyle(lineWidth: 1.6, lineCap: .round, lineJoin: .round))
        }.accessibilityLabel(Text("CPU temperature history, past 10 minutes"))
            .overlay(alignment: .center) {
                if visible.compactMap(\.temperature).count < 2 { Text("Collecting history").font(.system(size: 8)).foregroundStyle(.secondary) }
            }
    }
}
