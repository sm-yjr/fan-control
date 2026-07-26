import SwiftUI

struct FanStatusView: View {
    let fan: FanInfo
    let fanState: FanState?
    let onModeChange: (FanControlMode) -> Void
    let onEditCurve: () -> Void

    @CLTState private var manualRPM: Double = 1500

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Image(systemName: "fan")
                Text(fan.name)
                    .font(.system(.body, weight: .medium))
                Spacer()
                Text("\(Int(fan.currentSpeed)) RPM")
                    .font(.system(.body, design: .monospaced))
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 4) {
                speedBar(current: fan.currentSpeed, min: fan.minSpeed, max: fan.maxSpeed)
                Text(String(format: "%.0f%%", percentSpeed))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(width: 36, alignment: .trailing)
            }

            HStack(spacing: 8) {
                modeButton("Auto", mode: .automatic)
                modeButton("Manual", mode: .manual(rpm: Int(manualRPM)))
                Button {
                    onEditCurve()
                } label: {
                    HStack(spacing: 2) {
                        Image(systemName: "chart.line.uptrend.xyaxis")
                        Text("Curve")
                    }
                    .font(.caption)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 3)
                    .background(isCurveMode ? Color.accentColor : Color.secondary.opacity(0.15))
                    .foregroundStyle(isCurveMode ? .white : .primary)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                }
                .buttonStyle(.plain)
            }

            if isManualMode {
                VStack(spacing: 2) {
                    Slider(
                        value: $manualRPM,
                        in: fan.minSpeed...fan.maxSpeed,
                        step: 50
                    ) {
                        Text("RPM")
                    } onEditingChanged: { editing in
                        if !editing {
                            onModeChange(.manual(rpm: Int(manualRPM)))
                        }
                    }
                    HStack {
                        Text("\(Int(fan.minSpeed))")
                        Spacer()
                        Text("Target: \(Int(manualRPM)) RPM")
                            .fontWeight(.medium)
                        Spacer()
                        Text("\(Int(fan.maxSpeed))")
                    }
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                }
            }

            if isCurveMode, let state = fanState {
                curveStatus(state)
            }
        }
        .padding(8)
        .background(Color.primary.opacity(0.03))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private var percentSpeed: Double {
        guard fan.maxSpeed > fan.minSpeed else { return 0 }
        return max(0, min(100, (fan.currentSpeed - fan.minSpeed) / (fan.maxSpeed - fan.minSpeed) * 100))
    }

    private var isManualMode: Bool {
        if case .manual = fanState?.mode { return true }
        return false
    }

    private var isCurveMode: Bool {
        if case .curve = fanState?.mode { return true }
        return false
    }

    private func modeButton(_ title: String, mode: FanControlMode) -> some View {
        let isActive: Bool = switch (fanState?.mode, mode) {
        case (.automatic, .automatic): true
        case (.manual, .manual): true
        default: false
        }

        return Button {
            if case .manual = mode {
                manualRPM = fan.currentSpeed
            }
            onModeChange(mode)
        } label: {
            Text(title)
                .font(.caption)
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(isActive ? Color.accentColor : Color.secondary.opacity(0.15))
                .foregroundStyle(isActive ? .white : .primary)
                .clipShape(RoundedRectangle(cornerRadius: 4))
        }
        .buttonStyle(.plain)
    }

    private func speedBar(current: Double, min: Double, max: Double) -> some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(Color.secondary.opacity(0.15))
                RoundedRectangle(cornerRadius: 2)
                    .fill(barColor)
                    .frame(width: geo.size.width * percentSpeed / 100)
            }
        }
        .frame(height: 4)
    }

    private var barColor: Color {
        if percentSpeed > 80 { return .red }
        if percentSpeed > 50 { return .orange }
        return .accentColor
    }

    private func curveStatus(_ state: FanState) -> some View {
        HStack {
            if let config = state.curveConfig {
                Image(systemName: "thermometer.medium")
                    .foregroundStyle(.secondary)
                Text(curveInputText(config: config, value: state.lastTemperature))
                    .font(.caption)
                Spacer()
                Text("→ \(String(format: "%.0f%%", state.lastSpeedPercent))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func curveInputText(config: FanCurveConfig, value: Double) -> String {
        if CurveInput.isThermalDemand(config.sensorKey) {
            return "\(config.sensorKey): \(String(format: "%.1f%%", value))"
        }
        return "\(config.sensorKey): \(String(format: "%.1f°C", value))"
    }
}
