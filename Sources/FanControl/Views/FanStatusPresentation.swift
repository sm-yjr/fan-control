import Foundation

enum StatusItemIconLayout {
    private static let fallbackContainerSide: CGFloat = 24
    static let rotationAnchorX: CGFloat = 0.5
    static let rotationAnchorY: CGFloat = 0.5

    static func frame(
        containerSize: CGSize,
        iconSide: CGFloat
    ) -> StatusItemIconFrame {
        let width = containerSize.width > 0
            ? containerSize.width
            : fallbackContainerSide
        let height = containerSize.height > 0
            ? containerSize.height
            : fallbackContainerSide
        let originX = (width - iconSide) / 2
        let originY = (height - iconSide) / 2

        return StatusItemIconFrame(
            x: originX,
            y: originY,
            width: iconSide,
            height: iconSide
        )
    }
}

struct StatusItemIconFrame: Equatable {
    let x: CGFloat
    let y: CGFloat
    let width: CGFloat
    let height: CGFloat

    var midX: CGFloat { x + width / 2 }
    var midY: CGFloat { y + height / 2 }
}

struct FanRotationSample: Equatable {
    let currentRPM: Double
    let minimumRPM: Double
    let maximumRPM: Double
}

enum FanRotationLevel: Int, Comparable {
    case stopped
    case low
    case high

    static func < (lhs: FanRotationLevel, rhs: FanRotationLevel) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

struct FanStatusPresentation: Equatable {
    let level: FanRotationLevel
    let maximumRPM: Int

    var rotationPeriod: TimeInterval? {
        switch level {
        case .stopped:
            nil
        case .low:
            2.4
        case .high:
            0.8
        }
    }

    var accessibilityValue: String {
        switch level {
        case .stopped:
            "Stopped"
        case .low:
            "Low speed, \(Self.formattedRPM(maximumRPM)) RPM"
        case .high:
            "High speed, \(Self.formattedRPM(maximumRPM)) RPM"
        }
    }

    static func resolve(samples: [FanRotationSample]) -> FanStatusPresentation {
        var highestLevel = FanRotationLevel.stopped
        var maximumRPM = 0

        for sample in samples {
            let currentRPM = sample.currentRPM.isFinite ? max(0, sample.currentRPM) : 0
            maximumRPM = max(maximumRPM, Int(currentRPM.rounded()))
            highestLevel = max(highestLevel, level(for: sample, currentRPM: currentRPM))
        }

        return FanStatusPresentation(level: highestLevel, maximumRPM: maximumRPM)
    }

    func shouldAnimate(reduceMotion: Bool) -> Bool {
        !reduceMotion && rotationPeriod != nil
    }

    func resolvedRotationPeriod(
        reduceMotion: Bool,
        isStatusItemVisible: Bool
    ) -> TimeInterval? {
        guard isStatusItemVisible, !reduceMotion else { return nil }
        return rotationPeriod
    }

    func angle(at time: TimeInterval) -> Double {
        guard let rotationPeriod, rotationPeriod > 0 else { return 0 }
        let phase = time.truncatingRemainder(dividingBy: rotationPeriod)
        let normalizedPhase = phase >= 0 ? phase : phase + rotationPeriod
        let angle = normalizedPhase / rotationPeriod * 360
        return abs(angle - 360) < 0.000_001 ? 0 : angle
    }

    private static func level(
        for sample: FanRotationSample,
        currentRPM: Double
    ) -> FanRotationLevel {
        guard currentRPM >= 100 else { return .stopped }
        guard sample.minimumRPM.isFinite,
              sample.maximumRPM.isFinite,
              sample.maximumRPM > sample.minimumRPM else {
            return .low
        }

        let operatingFraction = (currentRPM - sample.minimumRPM)
            / (sample.maximumRPM - sample.minimumRPM)
        return operatingFraction >= 0.55 ? .high : .low
    }

    private static func formattedRPM(_ value: Int) -> String {
        groupedInteger(value)
    }

    private static func groupedInteger(_ value: Int) -> String {
        let digits = String(abs(value))
        var reversed = ""
        for (offset, character) in digits.reversed().enumerated() {
            if offset > 0 && offset.isMultiple(of: 3) {
                reversed.append(",")
            }
            reversed.append(character)
        }
        let grouped = String(reversed.reversed())
        return value < 0 ? "-\(grouped)" : grouped
    }
}
