import SwiftUI

struct SensorListView: View {
    let sensors: [TemperatureSensor]

    var body: some View {
        VStack(
            alignment: .leading,
            spacing: FanUISpacing.xSmall.points
        ) {
            let groups = Dictionary(grouping: sensors, by: \.group)
            ForEach([TemperatureSensor.SensorGroup.cpu, .gpu, .system, .other], id: \.self) { group in
                if let items = groups[group], !items.isEmpty {
                    Section {
                        ForEach(items) { sensor in
                            FanStatusRow(
                                title: sensor.name,
                                value: String(
                                    format: "%.1f°C",
                                    sensor.value
                                ),
                                tone: FanMetricPresentation
                                    .temperatureSensor(sensor.value)
                                    .tone
                            )
                        }
                    } header: {
                        Text(group.rawValue)
                            .font(FanUITextStyle.sectionHeading.font)
                            .foregroundStyle(
                                FanUIColorRole.secondaryText.color
                            )
                            .textCase(.uppercase)
                    }
                }
            }
        }
    }

}
