import SwiftUI

struct CurveEditorSheet: View {
    @Bindable var fanController: FanController
    @Bindable var sensorManager: SensorManager
    let fanId: Int
    @Environment(\.dismiss) private var dismiss

    @CLTState private var config: FanCurveConfig
    @CLTState private var selectedSensorKey: String

    init(fanController: FanController, sensorManager: SensorManager, fanId: Int) {
        self.fanController = fanController
        self.sensorManager = sensorManager
        self.fanId = fanId

        let existing = fanController.fanStates.first(where: { $0.fanId == fanId })?.curveConfig
        let defaultSensor = sensorManager.averageCPU > 0 ? "Average CPU" : (sensorManager.temperatures.first?.key ?? "")
        let cfg = existing ?? FanCurveConfig.defaultCurve(sensorKey: defaultSensor)
        self._config = CLTState(initialValue: cfg)
        self._selectedSensorKey = CLTState(initialValue: cfg.sensorKey)
    }

    var body: some View {
        VStack(spacing: 16) {
            Text("Fan Curve Editor")
                .font(.headline)

            sensorPicker

            CurveEditorView(
                points: $config.points,
                currentTemperature: currentTemp,
                currentSpeed: currentSpeedPercent
            )
            .frame(height: 250)

            hysteresisSlider

            HStack {
                Button("Reset to Default") {
                    config = FanCurveConfig.defaultCurve(sensorKey: selectedSensorKey)
                }
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Apply") {
                    config.sensorKey = selectedSensorKey
                    fanController.setCurveConfig(config, forFan: fanId)
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
        .frame(width: 480)
        .onChange(of: selectedSensorKey) { _, newValue in
            config.sensorKey = newValue
        }
    }

    private var sensorPicker: some View {
        HStack {
            Text("Temperature Source:")
                .font(.subheadline)
            Picker("", selection: $selectedSensorKey) {
                if sensorManager.averageCPU > 0 {
                    Text("Average CPU").tag("Average CPU")
                    Text("Hottest CPU").tag("Hottest CPU")
                }
                if sensorManager.averageGPU > 0 {
                    Text("Average GPU").tag("Average GPU")
                    Text("Hottest GPU").tag("Hottest GPU")
                }
                Divider()
                ForEach(sensorManager.temperatures) { sensor in
                    Text("\(sensor.name) (\(String(format: "%.0f°C", sensor.value)))")
                        .tag(sensor.key)
                }
            }
            .frame(width: 200)
        }
    }

    private var hysteresisSlider: some View {
        HStack {
            Text("Hysteresis:")
                .font(.subheadline)
            Slider(value: $config.hysteresis, in: 0...10, step: 0.5)
            Text(String(format: "%.1f°C", config.hysteresis))
                .font(.system(.subheadline, design: .monospaced))
            .frame(width: 50)
        }
    }

    private var currentTemp: Double {
        switch selectedSensorKey {
        case "Average CPU": sensorManager.averageCPU
        case "Average GPU": sensorManager.averageGPU
        case "Hottest CPU": sensorManager.hottestCPU
        case "Hottest GPU": sensorManager.hottestGPU
        default: sensorManager.temperatures.first(where: { $0.key == selectedSensorKey })?.value ?? 0
        }
    }

    private var currentSpeedPercent: Double {
        return config.interpolate(temperature: currentTemp)
    }
}

struct CurveEditorView: View {
    @Binding var points: [CurvePoint]
    var currentTemperature: Double
    var currentSpeed: Double

    private let tempRange: ClosedRange<Double> = 20...100
    private let speedRange: ClosedRange<Double> = FanCurveConfig.fanOffSpeed...100
    private let padding: CGFloat = 40

    @CLTState private var draggingPointId: UUID?

    var body: some View {
        GeometryReader { geo in
            let plotArea = CGRect(
                x: padding,
                y: 10,
                width: geo.size.width - padding - 10,
                height: geo.size.height - padding - 10
            )

            ZStack(alignment: .topLeading) {
                gridAndLabels(plotArea)
                curvePath(plotArea)
                currentIndicator(plotArea)
                controlPoints(plotArea)
            }
            .contentShape(Rectangle())
            .onTapGesture(count: 2) { location in
                addPoint(at: location, in: plotArea)
            }
        }
        .background(Color.primary.opacity(0.03))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    // MARK: - Grid

    private func gridAndLabels(_ rect: CGRect) -> some View {
        Canvas { context, _ in
            let gridColor = Color.secondary.opacity(0.15)
            let zeroY = speedToY(0, in: rect)
            let offRect = CGRect(
                x: rect.minX,
                y: zeroY,
                width: rect.width,
                height: rect.maxY - zeroY
            )
            context.fill(Path(offRect), with: .color(Color.orange.opacity(0.08)))
            context.stroke(
                Path { p in p.move(to: CGPoint(x: rect.minX, y: zeroY)); p.addLine(to: CGPoint(x: rect.maxX, y: zeroY)) },
                with: .color(Color.secondary.opacity(0.35)), lineWidth: 1
            )
            context.draw(
                Text("0 RPM").font(.system(size: 9)).foregroundStyle(.orange),
                at: CGPoint(x: rect.minX - 22, y: rect.maxY - 6)
            )

            for temp in stride(from: 20.0, through: 100.0, by: 10.0) {
                let x = tempToX(temp, in: rect)
                context.stroke(
                    Path { p in p.move(to: CGPoint(x: x, y: rect.minY)); p.addLine(to: CGPoint(x: x, y: rect.maxY)) },
                    with: .color(gridColor), lineWidth: 0.5
                )
                context.draw(
                    Text("\(Int(temp))°").font(.system(size: 9)).foregroundStyle(.secondary),
                    at: CGPoint(x: x, y: rect.maxY + 12)
                )
            }

            for speed in stride(from: 0.0, through: 100.0, by: 20.0) {
                let y = speedToY(speed, in: rect)
                context.stroke(
                    Path { p in p.move(to: CGPoint(x: rect.minX, y: y)); p.addLine(to: CGPoint(x: rect.maxX, y: y)) },
                    with: .color(gridColor), lineWidth: 0.5
                )
                context.draw(
                    Text("\(Int(speed))%").font(.system(size: 9)).foregroundStyle(.secondary),
                    at: CGPoint(x: rect.minX - 20, y: y)
                )
            }
        }
    }

    // MARK: - Curve

    private func curvePath(_ rect: CGRect) -> some View {
        let sorted = points.sorted { $0.temperature < $1.temperature }
        return Path { path in
            guard let first = sorted.first else { return }
            path.move(to: CGPoint(x: rect.minX, y: speedToY(first.fanSpeed, in: rect)))
            path.addLine(to: CGPoint(x: tempToX(first.temperature, in: rect), y: speedToY(first.fanSpeed, in: rect)))

            for point in sorted {
                path.addLine(to: CGPoint(x: tempToX(point.temperature, in: rect), y: speedToY(point.fanSpeed, in: rect)))
            }

            if let last = sorted.last {
                path.addLine(to: CGPoint(x: rect.maxX, y: speedToY(last.fanSpeed, in: rect)))
            }
        }
        .stroke(Color.accentColor, lineWidth: 2)
    }

    // MARK: - Current indicator

    private func currentIndicator(_ rect: CGRect) -> some View {
        let x = tempToX(currentTemperature, in: rect)
        let y = speedToY(currentSpeed, in: rect)

        return ZStack {
            Path { p in
                p.move(to: CGPoint(x: x, y: rect.minY))
                p.addLine(to: CGPoint(x: x, y: rect.maxY))
            }
            .stroke(Color.orange.opacity(0.5), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))

            Circle()
                .fill(Color.orange)
                .frame(width: 8, height: 8)
                .position(x: x, y: y)
        }
    }

    // MARK: - Control points

    private func controlPoints(_ rect: CGRect) -> some View {
        ForEach(points) { point in
            let x = tempToX(point.temperature, in: rect)
            let y = speedToY(point.fanSpeed, in: rect)

            Circle()
                .fill(draggingPointId == point.id ? Color.white : Color.accentColor)
                .stroke(Color.accentColor, lineWidth: 2)
                .frame(width: 14, height: 14)
                .position(x: x, y: y)
                .gesture(
                    DragGesture(minimumDistance: 1)
                        .onChanged { value in
                            draggingPointId = point.id
                            guard let idx = points.firstIndex(where: { $0.id == point.id }) else { return }
                            let temp = xToTemp(value.location.x, in: rect)
                            let speed = yToSpeed(value.location.y, in: rect)
                            points[idx].temperature = min(max(temp, tempRange.lowerBound), tempRange.upperBound)
                            points[idx].fanSpeed = min(max(speed, speedRange.lowerBound), speedRange.upperBound)
                        }
                        .onEnded { _ in draggingPointId = nil }
                )
                .contextMenu {
                    if points.count > 2 {
                        Button("Delete Point") {
                            points.removeAll { $0.id == point.id }
                        }
                    }
                }
        }
    }

    // MARK: - Coordinate conversion

    private func tempToX(_ temp: Double, in rect: CGRect) -> CGFloat {
        let ratio = (temp - tempRange.lowerBound) / (tempRange.upperBound - tempRange.lowerBound)
        return rect.minX + CGFloat(ratio) * rect.width
    }

    private func speedToY(_ speed: Double, in rect: CGRect) -> CGFloat {
        let ratio = (speed - speedRange.lowerBound) / (speedRange.upperBound - speedRange.lowerBound)
        return rect.maxY - CGFloat(ratio) * rect.height
    }

    private func xToTemp(_ x: CGFloat, in rect: CGRect) -> Double {
        let ratio = Double((x - rect.minX) / rect.width)
        return tempRange.lowerBound + ratio * (tempRange.upperBound - tempRange.lowerBound)
    }

    private func yToSpeed(_ y: CGFloat, in rect: CGRect) -> Double {
        let ratio = Double((rect.maxY - y) / rect.height)
        return speedRange.lowerBound + ratio * (speedRange.upperBound - speedRange.lowerBound)
    }

    private func addPoint(at location: CGPoint, in rect: CGRect) {
        guard rect.contains(location) else { return }
        let temp = xToTemp(location.x, in: rect)
        let speed = yToSpeed(location.y, in: rect)
        points.append(CurvePoint(
            temperature: min(max(temp, tempRange.lowerBound), tempRange.upperBound),
            fanSpeed: min(max(speed, speedRange.lowerBound), speedRange.upperBound)
        ))
    }
}
