import SwiftUI

struct CurveEditorView: View {
    @Binding var points: [CurvePoint]
    var currentInput: Double
    var currentSpeed: Double
    var isThermalDemand: Bool

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
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Fan curve editor")
            .accessibilityHint("Curve points provide actions for adjusting input and fan speed")
        }
        .fanUISurface()
        .help("Double-click to add a curve point; drag a point to adjust it")
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

            let step = isThermalDemand ? 20.0 : 10.0
            for input in stride(from: inputRange.lowerBound, through: inputRange.upperBound, by: step) {
                let x = inputToX(input, in: rect)
                context.stroke(
                    Path { p in p.move(to: CGPoint(x: x, y: rect.minY)); p.addLine(to: CGPoint(x: x, y: rect.maxY)) },
                    with: .color(gridColor), lineWidth: 0.5
                )
                context.draw(
                    Text(inputLabel(input)).font(.system(size: 9)).foregroundStyle(.secondary),
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
            path.addLine(to: CGPoint(x: inputToX(first.temperature, in: rect), y: speedToY(first.fanSpeed, in: rect)))

            for point in sorted {
                path.addLine(to: CGPoint(x: inputToX(point.temperature, in: rect), y: speedToY(point.fanSpeed, in: rect)))
            }

            if let last = sorted.last {
                path.addLine(to: CGPoint(x: rect.maxX, y: speedToY(last.fanSpeed, in: rect)))
            }
        }
        .stroke(Color.accentColor, lineWidth: 2)
    }

    // MARK: - Current indicator

    private func currentIndicator(_ rect: CGRect) -> some View {
        let x = inputToX(currentInput, in: rect)
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
            let x = inputToX(point.temperature, in: rect)
            let y = speedToY(point.fanSpeed, in: rect)

            ZStack {
                Circle()
                    .fill(draggingPointId == point.id ? Color.white : Color.accentColor)
                    .stroke(Color.accentColor, lineWidth: 2)
                    .frame(width: 14, height: 14)
            }
                .frame(width: 28, height: 28)
                .contentShape(Circle())
                .position(x: x, y: y)
                .gesture(
                    DragGesture(minimumDistance: 1)
                        .onChanged { value in
                            draggingPointId = point.id
                            guard let idx = points.firstIndex(where: { $0.id == point.id }) else { return }
                            let input = xToInput(value.location.x, in: rect)
                            let speed = yToSpeed(value.location.y, in: rect)
                            points[idx].temperature = min(max(input, inputRange.lowerBound), inputRange.upperBound)
                            points[idx].fanSpeed = min(max(speed, speedRange.lowerBound), speedRange.upperBound)
                        }
                        .onEnded { _ in draggingPointId = nil }
                )
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("Curve point")
                .accessibilityValue(accessibilityValue(for: point))
                .accessibilityHint("Use actions to adjust this point")
                .accessibilityAction(named: Text("Increase \(inputAccessibilityName)")) {
                    adjustPoint(point.id, inputDelta: inputAdjustmentStep)
                }
                .accessibilityAction(named: Text("Decrease \(inputAccessibilityName)")) {
                    adjustPoint(point.id, inputDelta: -inputAdjustmentStep)
                }
                .accessibilityAction(named: Text("Increase fan speed")) {
                    adjustPoint(point.id, speedDelta: 5)
                }
                .accessibilityAction(named: Text("Decrease fan speed")) {
                    adjustPoint(point.id, speedDelta: -5)
                }
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

    private var inputRange: ClosedRange<Double> {
        isThermalDemand ? 0...100 : 20...100
    }

    private func inputToX(_ input: Double, in rect: CGRect) -> CGFloat {
        let rawRatio = (input - inputRange.lowerBound) / (inputRange.upperBound - inputRange.lowerBound)
        let ratio = min(max(rawRatio, 0), 1)
        return rect.minX + CGFloat(ratio) * rect.width
    }

    private func speedToY(_ speed: Double, in rect: CGRect) -> CGFloat {
        let ratio = (speed - speedRange.lowerBound) / (speedRange.upperBound - speedRange.lowerBound)
        return rect.maxY - CGFloat(ratio) * rect.height
    }

    private func xToInput(_ x: CGFloat, in rect: CGRect) -> Double {
        let ratio = Double((x - rect.minX) / rect.width)
        return inputRange.lowerBound + ratio * (inputRange.upperBound - inputRange.lowerBound)
    }

    private func yToSpeed(_ y: CGFloat, in rect: CGRect) -> Double {
        let ratio = Double((rect.maxY - y) / rect.height)
        return speedRange.lowerBound + ratio * (speedRange.upperBound - speedRange.lowerBound)
    }

    private func addPoint(at location: CGPoint, in rect: CGRect) {
        guard rect.contains(location) else { return }
        let input = xToInput(location.x, in: rect)
        let speed = yToSpeed(location.y, in: rect)
        points.append(CurvePoint(
            temperature: min(max(input, inputRange.lowerBound), inputRange.upperBound),
            fanSpeed: min(max(speed, speedRange.lowerBound), speedRange.upperBound)
        ))
    }

    private var inputAccessibilityName: String {
        isThermalDemand ? "thermal load" : "temperature"
    }

    private var inputAdjustmentStep: Double {
        isThermalDemand ? 2 : 1
    }

    private func accessibilityValue(for point: CurvePoint) -> String {
        let inputValue = isThermalDemand
            ? "\(Int(point.temperature.rounded())) percent thermal load"
            : "\(Int(point.temperature.rounded())) degrees Celsius"
        let speedValue = FanCurveConfig.isFanOffSpeed(point.fanSpeed)
            ? "fan stopped"
            : "\(Int(point.fanSpeed.rounded())) percent fan speed"
        return "\(inputValue), \(speedValue)"
    }

    private func adjustPoint(
        _ pointId: UUID,
        inputDelta: Double = 0,
        speedDelta: Double = 0
    ) {
        guard let index = points.firstIndex(where: { $0.id == pointId }) else { return }
        points[index].temperature = min(
            max(points[index].temperature + inputDelta, inputRange.lowerBound),
            inputRange.upperBound
        )
        points[index].fanSpeed = min(
            max(points[index].fanSpeed + speedDelta, speedRange.lowerBound),
            speedRange.upperBound
        )
    }

    private func inputLabel(_ input: Double) -> String {
        isThermalDemand ? "\(Int(input))%" : "\(Int(input))°"
    }
}
