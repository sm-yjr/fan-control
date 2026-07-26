import SwiftUI

private enum ControlModeSelection: Hashable {
    case automatic
    case manual
    case curve
}

struct ContentView: View {
    @Bindable var appState: AppState

    var sensorManager: SensorManager { appState.sensorManager }
    var fanController: FanController { appState.fanController }

    @CLTState private var selectedFanIndex: Int = 0
    @CLTState private var showAllSensors = false

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
        .frame(width: 420)
        .background(FanUIColorRole.background.color)
    }

    private var permissionBanner: some View {
        FanSection {
            FanCard(padding: .medium) {
                HStack(
                    alignment: .top,
                    spacing: FanUISpacing.medium.points
                ) {
                    Image(systemName: "lock.trianglebadge.exclamationmark")
                        .foregroundStyle(FanUIColorRole.warning.color)
                    VStack(
                        alignment: .leading,
                        spacing: FanUISpacing.hairline.points
                    ) {
                        Text("Fan control needs privileged helper")
                            .font(FanUITextStyle.label.font)
                            .fontWeight(.medium)
                        Text(appState.helperMessage ?? "Install once with administrator approval, then double-click works normally.")
                            .font(FanUITextStyle.metadata.font)
                            .foregroundStyle(FanUIColorRole.secondaryText.color)
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
                    .font(FanUITextStyle.label.font)
                    .disabled(appState.isInstallingHelper)
                }
            }
        }
    }

    // MARK: - Temperature bar

    private var temperatureBar: some View {
        VStack(spacing: 0) {
            HStack(spacing: FanUISpacing.large.points) {
                if sensorManager.thermalDemandAvailable {
                    FanMetricBadge(
                        label: "Heat",
                        presentation: .thermalLoad(
                            sensorManager.thermalDemand
                        )
                    )
                }
                if sensorManager.averageCPU > 0 {
                    FanMetricBadge(
                        label: "CPU",
                        presentation: .temperatureSummary(
                            sensorManager.averageCPU
                        )
                    )
                }
                if sensorManager.averageGPU > 0 {
                    FanMetricBadge(
                        label: "GPU",
                        presentation: .temperatureSummary(
                            sensorManager.averageGPU
                        )
                    )
                }
                Spacer()
                Button {
                    showAllSensors.toggle()
                } label: {
                    Image(systemName: showAllSensors ? "chevron.up" : "chevron.down")
                        .frame(width: 20, height: 20)
                }
                .buttonStyle(.borderless)
                .controlSize(.small)
                .foregroundStyle(.secondary)
                .accessibilityLabel(showAllSensors ? "Hide sensor details" : "Show sensor details")
                .help(showAllSensors ? "Hide sensor details" : "Show sensor details")
            }
            .padding(.horizontal, FanUISpacing.large.points)
            .padding(.vertical, FanUISpacing.medium.points)

            if showAllSensors {
                ScrollView {
                    VStack(
                        alignment: .leading,
                        spacing: FanUISpacing.small.points
                    ) {
                        if sensorManager.thermalDemandAvailable {
                            thermalModelSummary
                            Divider()
                        }
                        SensorListView(sensors: sensorManager.temperatures)
                    }
                }
                .frame(maxHeight: 150)
                .padding(.horizontal, FanUISpacing.large.points)
                .padding(.bottom, FanUISpacing.small.points)
            }
        }
    }

    private var thermalModelSummary: some View {
        VStack(
            alignment: .leading,
            spacing: FanUISpacing.hairline.points
        ) {
            Text("THERMAL MODEL")
                .font(FanUITextStyle.sectionHeading.font)
                .foregroundStyle(FanUIColorRole.secondaryText.color)
            FanStatusRow(
                title: "Sustained silicon",
                value: String(
                    format: "%.1f°C",
                    sensorManager.sustainedSiliconTemperature
                )
            )
            if sensorManager.thermalDemandUsesChassisSensor {
                FanStatusRow(
                    title: "Chassis thermal mass",
                    value: String(
                        format: "%.1f°C · +%.1f°C/min",
                        sensorManager.chassisTemperature,
                        sensorManager.chassisRisePerMinute
                    )
                )
            } else {
                Text("No chassis sensor; using sustained silicon fallback")
                    .font(FanUITextStyle.metadata.font)
                    .foregroundStyle(FanUIColorRole.secondaryText.color)
            }
            FanStatusRow(
                title: "System thermal pressure",
                value: sensorManager.systemThermalPressure.displayName
            )
        }
    }

    // MARK: - Fan tabs

    private var fanTabs: some View {
        let selection = Binding<Int>(
            get: { min(selectedFanIndex, max(sensorManager.fans.count - 1, 0)) },
            set: { selectedFanIndex = $0 }
        )

        return Picker("Fan", selection: selection) {
            ForEach(Array(sensorManager.fans.enumerated()), id: \.element.id) { index, fan in
                Text("\(fan.name) · \(Int(fan.currentSpeed)) RPM")
                    .tag(index)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .padding(.horizontal, FanUISpacing.large.points)
        .padding(.vertical, FanUISpacing.medium.points)
        .accessibilityLabel("Fan")
        .help("Choose the fan to configure")
    }

    // MARK: - Curve section (inline primary UI)

    @ViewBuilder
    private var curveSection: some View {
        let safeFanIndex = min(selectedFanIndex, max(sensorManager.fans.count - 1, 0))

        if safeFanIndex < sensorManager.fans.count {
            let fan = sensorManager.fans[safeFanIndex]
            let fanId = fan.id

            VStack(spacing: FanUISpacing.medium.points) {
                modeRow(fan: fan)

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
            .padding(.vertical, FanUISpacing.medium.points)
        }
    }

    // MARK: - Mode row

    private func modeRow(fan: FanInfo) -> some View {
        return HStack(spacing: FanUISpacing.medium.points) {
            FanModeSelector(
                "Control mode for \(fan.name)",
                selection: controlModeBinding(for: fan)
            ) {
                Text("Auto").tag(ControlModeSelection.automatic)
                Text("Manual").tag(ControlModeSelection.manual)
                Text("Curve").tag(ControlModeSelection.curve)
            }

            Spacer()

            if isCurveMode(fanId: fan.id),
               let stateIndex = fanController.fanStates.firstIndex(where: { $0.fanId == fan.id }) {
                sensorPicker(stateIndex: stateIndex, fanId: fan.id)
            }
        }
        .padding(.horizontal, FanUISpacing.large.points)
    }

    private func controlModeBinding(for fan: FanInfo) -> Binding<ControlModeSelection> {
        Binding(
            get: {
                guard let mode = fanController.fanStates.first(where: { $0.fanId == fan.id })?.mode else {
                    return .automatic
                }
                switch mode {
                case .automatic:
                    return .automatic
                case .manual:
                    return .manual
                case .curve:
                    return .curve
                }
            },
            set: { selection in
                switch selection {
                case .automatic:
                    fanController.setMode(.automatic, forFan: fan.id)
                case .manual:
                    let currentRPM = max(fan.minSpeed, min(fan.currentSpeed, fan.maxSpeed))
                    fanController.setMode(.manual(rpm: Int(currentRPM)), forFan: fan.id)
                case .curve:
                    let config = fanController.fanStates
                        .first(where: { $0.fanId == fan.id })?
                        .curveConfig
                        ?? FanCurveConfig.defaultCurve(sensorKey: defaultSensorKey)
                    fanController.setCurveConfig(config, forFan: fan.id)
                }
            }
        )
    }

    private func isCurveMode(fanId: Int) -> Bool {
        guard let mode = fanController.fanStates.first(where: { $0.fanId == fanId })?.mode else {
            return false
        }
        if case .curve = mode { return true }
        return false
    }

    private func sensorPicker(stateIndex: Int, fanId: Int) -> some View {
        let binding = Binding<String>(
            get: {
                fanController.fanStates[stateIndex].curveConfig?.sensorKey ?? defaultSensorKey
            },
            set: { newKey in
                var config = fanController.fanStates[stateIndex].curveConfig
                    ?? FanCurveConfig.defaultCurve(sensorKey: newKey)
                config.setSensorKey(newKey)
                fanController.setCurveConfig(config, forFan: fanId)
            }
        )
        return Picker("", selection: binding) {
            Text("Thermal Load").tag(CurveInput.thermalDemandKey)
            Divider()
            Text("CPU Avg").tag("Average CPU")
            Text("CPU Max").tag("Hottest CPU")
            if sensorManager.averageGPU > 0 {
                Text("GPU Avg").tag("Average GPU")
                Text("GPU Max").tag("Hottest GPU")
            }
        }
        .pickerStyle(.menu)
        .labelsHidden()
        .frame(width: 132)
        .accessibilityLabel("Curve control source")
        .help("Choose the temperature or thermal-load signal used by this curve")
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

        return VStack(spacing: FanUISpacing.small.points) {
            CurveEditorView(
                points: Binding(
                    get: { configBinding.wrappedValue.points },
                    set: { pts in
                        var c = configBinding.wrappedValue
                        c.points = pts
                        configBinding.wrappedValue = c
                    }
                ),
                currentInput: currentInput(for: sensorKey),
                currentSpeed: currentCurveSpeed(configBinding.wrappedValue, sensorKey: sensorKey),
                isThermalDemand: CurveInput.isThermalDemand(sensorKey)
            )
            .frame(height: 200)
            .padding(.horizontal, FanUISpacing.xSmall.points)

            HStack {
                Text("Hysteresis")
                    .font(FanUITextStyle.label.font)
                    .foregroundStyle(FanUIColorRole.secondaryText.color)
                Slider(
                    value: Binding(
                        get: { configBinding.wrappedValue.hysteresis },
                        set: { val in
                            var c = configBinding.wrappedValue
                            c.hysteresis = val
                            configBinding.wrappedValue = c
                        }
                    ),
                    in: CurveInput.isThermalDemand(sensorKey) ? 0...20 : 0...10,
                    step: 0.5
                ) {
                    Text("Hysteresis")
                }
                .labelsHidden()
                .accessibilityValue(hysteresisText(
                    configBinding.wrappedValue.hysteresis,
                    sensorKey: sensorKey
                ))
                Text(hysteresisText(configBinding.wrappedValue.hysteresis, sensorKey: sensorKey))
                    .font(FanUITextStyle.statusValue.font)
                    .frame(width: 54)

                Button("Reset Curve") {
                    fanController.resetCurve(forFan: fanId)
                }
                .buttonStyle(.borderless)
                .controlSize(.small)
                .help("Restore the default curve for this fan")
            }
            .padding(.horizontal, FanUISpacing.large.points)
        }
    }

    // MARK: - Manual slider

    private func manualSlider(fanId: Int, fan: FanInfo, rpm: Int) -> some View {
        VStack(spacing: FanUISpacing.xSmall.points) {
            Slider(
                value: Binding(
                    get: { Double(rpm) },
                    set: { val in
                        fanController.setMode(.manual(rpm: Int(val)), forFan: fanId)
                    }
                ),
                in: fan.minSpeed...fan.maxSpeed,
                step: 50
            ) {
                Text("Target fan speed")
            }
            .labelsHidden()
            .accessibilityValue("\(rpm) RPM")
            HStack {
                Text("\(Int(fan.minSpeed))")
                Spacer()
                Text("Target: \(rpm) RPM").fontWeight(.medium)
                Spacer()
                Text("\(Int(fan.maxSpeed))")
            }
            .font(FanUITextStyle.metadata.font)
            .foregroundStyle(FanUIColorRole.secondaryText.color)
        }
        .padding(.horizontal, FanUISpacing.large.points)
        .padding(.vertical, FanUISpacing.medium.points)
    }

    // MARK: - Auto indicator

    private var autoIndicator: some View {
        FanStatusRow(
            title: "Fan control",
            value: "System automatic",
            systemImage: "checkmark.circle",
            tone: .success
        )
        .padding(.horizontal, FanUISpacing.large.points)
        .padding(.vertical, FanUISpacing.xxLarge.points)
    }

    // MARK: - Speed indicator

    private func speedIndicator(fan: FanInfo) -> some View {
        FanProgressStatusRow(
            presentation: .fanSpeed(
                currentRPM: fan.currentSpeed,
                minimumRPM: fan.minSpeed,
                maximumRPM: fan.maxSpeed,
                fanName: fan.name
            )
        )
        .padding(.horizontal, FanUISpacing.large.points)
    }

    // MARK: - Helpers

    private var defaultSensorKey: String {
        sensorManager.preferredCurveSensorKey
    }

    private func currentInput(for sensorKey: String) -> Double {
        sensorManager.curveInputValue(for: sensorKey) ?? 0
    }

    private func currentCurveSpeed(_ config: FanCurveConfig, sensorKey: String) -> Double {
        config.interpolate(temperature: currentInput(for: sensorKey))
    }

    private func hysteresisText(_ hysteresis: Double, sensorKey: String) -> String {
        if CurveInput.isThermalDemand(sensorKey) {
            return String(format: "%.1f%%", hysteresis)
        }
        return String(format: "%.1f°C", hysteresis)
    }

    // MARK: - Footer

    private var footerSection: some View {
        HStack {
            Button("Reset All") {
                for fan in sensorManager.fans {
                    fanController.setMode(.automatic, forFan: fan.id)
                }
            }
            .buttonStyle(.borderless)
            .controlSize(.small)
            .help("Return every fan to system automatic control")

            FanUpdateButton(
                updaterAvailable: appState.updateController.isAvailable
            ) {
                appState.updateController.checkForUpdates()
            }

            Spacer()

            Button("Quit") {
                fanController.stop()
                NSApplication.shared.terminate(nil)
            }
            .buttonStyle(.borderless)
            .controlSize(.small)
            .keyboardShortcut("q")
        }
        .padding(.horizontal, FanUISpacing.large.points)
        .padding(.vertical, FanUISpacing.medium.points)
    }
}
