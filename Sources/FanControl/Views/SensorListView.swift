import SwiftUI

struct SensorListView: View {
    let sensors: [TemperatureSensor]

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            let groups = Dictionary(grouping: sensors, by: \.group)
            ForEach([TemperatureSensor.SensorGroup.cpu, .gpu, .system, .other], id: \.self) { group in
                if let items = groups[group], !items.isEmpty {
                    Section {
                        ForEach(items) { sensor in
                            HStack {
                                Text(sensor.name)
                                    .font(.caption)
                                Spacer()
                                Text(String(format: "%.1f°C", sensor.value))
                                    .font(.system(.caption, design: .monospaced))
                                    .foregroundStyle(tempColor(sensor.value))
                            }
                        }
                    } header: {
                        Text(group.rawValue)
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                            .textCase(.uppercase)
                    }
                }
            }
        }
    }

    private func tempColor(_ temp: Double) -> Color {
        if temp > 90 { return .red }
        if temp > 75 { return .orange }
        if temp > 60 { return .yellow }
        return .green
    }
}
