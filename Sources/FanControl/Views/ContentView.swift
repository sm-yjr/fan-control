import SwiftUI

struct ContentView: View {
    @Bindable var appState: AppState

    var sensorManager: SensorManager { appState.sensorManager }
    var fanController: FanController { appState.fanController }

    @State private var selectedFanIndex: Int = 0
    @State private var showAllSensors = false

    var body: some View {
        VStack(spacing: 0) {
            temperatureBar
            Divider()

            if !appState.canWriteFans {
                permissionBanner
                Divider()
            }

            if !sensorManager.fans.isEmpty {
                fanTabs
                Divider()
                curveSection
                    .disabled(!appState.canWriteFans)
                    .opacity(appState.canWriteFans ? 1 : 0.45)
            } else {
                Text("No fans detected")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 80)
            }

            Divider()
            footerSection
        }
        .frame(width: 400)
    }

    private var permissionBanner: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "lock.trianglebadge.exclamationmark")
                .foregroundStyle(.orange)
            VStack(alignment: .leading, spacing: 2) {
                Text("Fan control needs privileged helper")
                    .font(.caption)
                    .fontWeight(.medium)
                Text(appState.helperMessage ?? "Install once with administrator approval, then double-click works normally.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button {
                appState.installHelper()
            } label: {
                if appState.isInstallingHelper {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Text("Enable")
                }
            }
            .font(.caption)
            .disabled(appState.isInstallingHelper)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    // MARK: - Temperature bar

    private var temperatureBar: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                if sensorManager.averageCPU > 0 {
                    tempChip("CPU", sensorManager.averageCPU)
                }
                if sensorManager.averageGPU > 0 {
                    tempChip("GPU", sensorManager.averageGPU)
                }
                Spacer()
                Button {
                    showAllSensors.toggle()
                } label: {
                    Image(systemName: showAllSensors ? "chevron.up" : "chevron.down")
                        .font(.caption2)
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            if showAllSensors {
                ScrollView {
                    SensorListView(sensors: sensorManager.temperatures)
                }
                .frame(maxHeight: 150)
                .padding(.horizontal, 12)
                .padding(.bottom, 6)
            }
        }
    }

    private func tempChip(_ label: String, _ value: Double) -> some View {
        HStack(spacing: 4) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(String(format: "%.0f°C", value))
                .font(.system(.caption, design: .monospaced, weight: .medium))
                .foregroundStyle(tempColor(value))
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .background(Color.primary.opacity(0.05))
        .clipShape(RoundedRectangle(cornerRadius: 4))
    }

    private func tempColor(_ temp: Double) -> Color {
        if temp > 90 { return .red }
        if temp > 75 { return .orange }
        if temp > 60 { return .yellow }
        return .green
    }

    // MARK: - Fan tabs

    private var fanTabs: some View {
        HStack(spacing: 0) {
            ForEach(Array(sensorManager.fans.enumerated()), id: \.element.id) { index, fan in
                Button {
                    selectedFanIndex = index
                } label: {
                    VStack(spacing: 2) {
                        Text(fan.name)
                            .font(.caption)
                            .fontWeight(selectedFanIndex == index ? .semibold : .regular)
                        Text("\(Int(fan.currentSpeed)) RPM")
                            .font(.system(.caption2, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(selectedFanIndex == index ? Color.accentColor.opacity(0.12) : Color.clear)
                }
                .buttonStyle(.plain)
            }
            Spacer()
        }
        .padding(.horizontal, 4)
    }

    // MARK: - Curve section (inline primary UI)

    @ViewBuilder
    private var curveSection: some View {
        let safeFanIndex = min(selectedFanIndex, max(sensorManager.fans.count - 1, 0))

        if safeFanIndex < sensorManager.fans.count {
            let fan = sensorManager.fans[safeFanIndex]
            let fanId = fan.id

            VStack(spacing: 8) {
                modeRow(fanId: fanId)

                if let stateIdx = fanController.fanStates.firstIndex(where: { $0.fanId == fanId }) {
                    let state = fanController.fanStates[stateIdx]

                    switch state.mode {
                    case .curve:
                        inlineCurveEditor(fanId: fanId, stateIndex: stateIdx)
                    case .manual(let rpm):
                        manualSlider(fanId: fanId, fan: fan, rpm: rpm)
                    case .automatic:
                        autoIndicator
                    }
                }

                speedIndicator(fan: fan)
            }
            .padding(.vertical, 8)
        }
    }

    // MARK: - Mode row

    private func modeRow(fanId: Int) -> some View {
        let stateIdx = fanController.fanStates.firstIndex(where: { $0.fanId == fanId })
        let currentMode = stateIdx.map { fanController.fanStates[$0].mode } ?? .automatic

        return HStack(spacing: 8) {
            modeButton("Auto", isActive: { if case .automatic = currentMode { return true }; return false }()) {
                fanController.setMode(.automatic, forFan: fanId)
            }
            modeButton("Manual", isActive: { if case .manual = currentMode { return true }; return false }()) {
                let speed = sensorManager.fans.first(where: { $0.id == fanId })?.currentSpeed ?? 1500
                fanController.setMode(.manual(rpm: Int(speed)), forFan: fanId)
            }
            modeButton("Curve", isActive: { if case .curve = currentMode { return true }; return false }()) {
                let config = stateIdx.flatMap { fanController.fanStates[$0].curveConfig }
                    ?? FanCurveConfig.defaultCurve(sensorKey: defaultSensorKey)
                fanController.setCurveConfig(config, forFan: fanId)
            }

            Spacer()

            if case .curve = currentMode, let idx = stateIdx {
                sensorPicker(stateIndex: idx, fanId: fanId)
            }
        }
        .padding(.horizontal, 12)
    }

    private func modeButton(_ title: String, isActive: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(.caption)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(isActive ? Color.accentColor : Color.secondary.opacity(0.12))
                .foregroundStyle(isActive ? .white : .primary)
                .clipShape(RoundedRectangle(cornerRadius: 5))
        }
        .buttonStyle(.plain)
    }

    private func sensorPicker(stateIndex: Int, fanId: Int) -> some View {
        let binding = Binding<String>(
            get: {
                fanController.fanStates[stateIndex].curveConfig?.sensorKey ?? defaultSensorKey
            },
            set: { newKey in
                var config = fanController.fanStates[stateIndex].curveConfig
                    ?? FanCurveConfig.defaultCurve(sensorKey: newKey)
                config.sensorKey = newKey
                fanController.setCurveConfig(config, forFan: fanId)
            }
        )
        return Picker("", selection: binding) {
            Text("CPU Avg").tag("Average CPU")
            Text("CPU Max").tag("Hottest CPU")
            if sensorManager.averageGPU > 0 {
                Text("GPU Avg").tag("Average GPU")
                Text("GPU Max").tag("Hottest GPU")
            }
        }
        .pickerStyle(.menu)
        .frame(width: 100)
        .font(.caption)
    }

    // MARK: - Inline curve editor

    private func inlineCurveEditor(fanId: Int, stateIndex: Int) -> some View {
        let configBinding = Binding<FanCurveConfig>(
            get: {
                fanController.fanStates[stateIndex].curveConfig
                    ?? FanCurveConfig.defaultCurve(sensorKey: defaultSensorKey)
            },
            set: { newConfig in
                fanController.setCurveConfig(newConfig, forFan: fanId)
            }
        )

        let sensorKey = configBinding.wrappedValue.sensorKey

        return VStack(spacing: 6) {
            CurveEditorView(
                points: Binding(
                    get: { configBinding.wrappedValue.points },
                    set: { pts in
                        var c = configBinding.wrappedValue
                        c.points = pts
                        configBinding.wrappedValue = c
                    }
                ),
                currentTemperature: currentTemp(for: sensorKey),
                currentSpeed: currentCurveSpeed(configBinding.wrappedValue, sensorKey: sensorKey)
            )
            .frame(height: 200)
            .padding(.horizontal, 4)

            HStack {
                Text("Hysteresis")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Slider(
                    value: Binding(
                        get: { configBinding.wrappedValue.hysteresis },
                        set: { val in
                            var c = configBinding.wrappedValue
                            c.hysteresis = val
                            configBinding.wrappedValue = c
                        }
                    ),
                    in: 0...10, step: 0.5
                )
                Text(String(format: "%.1f°C", configBinding.wrappedValue.hysteresis))
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 40)

                Button("Reset Curve") {
                    fanController.resetCurve(forFan: fanId)
                }
                .font(.caption)
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12)
        }
    }

    // MARK: - Manual slider

    private func manualSlider(fanId: Int, fan: FanInfo, rpm: Int) -> some View {
        VStack(spacing: 4) {
            Slider(
                value: Binding(
                    get: { Double(rpm) },
                    set: { val in
                        fanController.setMode(.manual(rpm: Int(val)), forFan: fanId)
                    }
                ),
                in: fan.minSpeed...fan.maxSpeed,
                step: 50
            )
            HStack {
                Text("\(Int(fan.minSpeed))")
                Spacer()
                Text("Target: \(rpm) RPM").fontWeight(.medium)
                Spacer()
                Text("\(Int(fan.maxSpeed))")
            }
            .font(.caption2)
            .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    // MARK: - Auto indicator

    private var autoIndicator: some View {
        HStack {
            Image(systemName: "checkmark.circle")
                .foregroundStyle(.green)
            Text("System auto control")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 20)
    }

    // MARK: - Speed indicator

    private func speedIndicator(fan: FanInfo) -> some View {
        let percent = fan.maxSpeed > fan.minSpeed
            ? max(0, min(100, (fan.currentSpeed - fan.minSpeed) / (fan.maxSpeed - fan.minSpeed) * 100))
            : 0

        return HStack(spacing: 6) {
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color.secondary.opacity(0.12))
                    RoundedRectangle(cornerRadius: 2)
                        .fill(percent > 80 ? Color.red : percent > 50 ? Color.orange : Color.accentColor)
                        .frame(width: geo.size.width * percent / 100)
                }
            }
            .frame(height: 4)

            Text(String(format: "%.0f%%", percent))
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(width: 32, alignment: .trailing)
        }
        .padding(.horizontal, 12)
    }

    // MARK: - Helpers

    private var defaultSensorKey: String {
        sensorManager.averageCPU > 0 ? "Average CPU" : (sensorManager.temperatures.first?.key ?? "")
    }

    private func currentTemp(for sensorKey: String) -> Double {
        switch sensorKey {
        case "Average CPU": sensorManager.averageCPU
        case "Average GPU": sensorManager.averageGPU
        case "Hottest CPU": sensorManager.hottestCPU
        case "Hottest GPU": sensorManager.hottestGPU
        default: sensorManager.temperatures.first(where: { $0.key == sensorKey })?.value ?? 0
        }
    }

    private func currentCurveSpeed(_ config: FanCurveConfig, sensorKey: String) -> Double {
        let temperature = currentTemp(for: sensorKey)
        return config.interpolate(temperature: temperature)
    }

    // MARK: - Footer

    private var footerSection: some View {
        HStack {
            Button("Reset All") {
                for fan in sensorManager.fans {
                    fanController.setMode(.automatic, forFan: fan.id)
                }
            }
            .font(.caption)
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)

            Spacer()

            Button("Quit") {
                fanController.stop()
                NSApplication.shared.terminate(nil)
            }
            .font(.caption)
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }
}
