import Foundation

@main
enum StatusItemPresentationChecks {
    static func main() {
        checkStoppedFansDoNotAnimate()
        checkLowSpeedUsesCalmAnimation()
        checkHighSpeedUsesFasterAnimation()
        checkFastestFanDeterminesStatus()
        checkReduceMotionStopsTimeline()
        checkMenuBarAnimatesWithoutPopover()
        checkRotationPhase()
        checkStatusIconUsesGeometricCenter()
        print("Status item presentation checks passed")
    }

    private static func checkStoppedFansDoNotAnimate() {
        let presentation = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 0, minimumRPM: 2_000, maximumRPM: 6_000),
            FanRotationSample(currentRPM: 40, minimumRPM: 2_000, maximumRPM: 6_000),
        ])

        require(presentation.level == .stopped, "stopped fans were reported as active")
        require(presentation.rotationPeriod == nil, "stopped fans scheduled animation")
        require(presentation.accessibilityValue == "Stopped", "stopped status is not readable")
    }

    private static func checkLowSpeedUsesCalmAnimation() {
        let presentation = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 2_300, minimumRPM: 2_000, maximumRPM: 6_000),
        ])

        require(presentation.level == .low, "minimum operating RPM was not treated as low speed")
        require(presentation.rotationPeriod == 2.4, "low-speed animation period changed")
        require(presentation.accessibilityValue == "Low speed, 2,300 RPM", "low-speed status lacks RPM")
    }

    private static func checkHighSpeedUsesFasterAnimation() {
        let presentation = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 4_400, minimumRPM: 2_000, maximumRPM: 6_000),
        ])

        require(presentation.level == .high, "upper operating range was not treated as high speed")
        require(presentation.rotationPeriod == 0.8, "high-speed animation period changed")
        require(presentation.accessibilityValue == "High speed, 4,400 RPM", "high-speed status lacks RPM")
    }

    private static func checkFastestFanDeterminesStatus() {
        let presentation = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 2_100, minimumRPM: 2_000, maximumRPM: 6_000),
            FanRotationSample(currentRPM: 5_000, minimumRPM: 2_000, maximumRPM: 6_000),
        ])

        require(presentation.level == .high, "combined status ignored the fastest fan")
        require(presentation.maximumRPM == 5_000, "combined status reported the wrong RPM")
    }

    private static func checkReduceMotionStopsTimeline() {
        let presentation = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 4_400, minimumRPM: 2_000, maximumRPM: 6_000),
        ])

        require(presentation.shouldAnimate(reduceMotion: false), "active fan did not animate")
        require(!presentation.shouldAnimate(reduceMotion: true), "Reduce Motion did not pause animation")
    }

    private static func checkMenuBarAnimatesWithoutPopover() {
        let active = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 2_300, minimumRPM: 2_000, maximumRPM: 6_000),
        ])

        require(
            active.resolvedRotationPeriod(
                reduceMotion: false,
                isStatusItemVisible: true
            ) == 2.4,
            "visible status icon must animate without opening the popover"
        )
        require(
            active.resolvedRotationPeriod(
                reduceMotion: true,
                isStatusItemVisible: true
            ) == nil,
            "Reduce Motion must stop the menu bar animation"
        )
        require(
            active.resolvedRotationPeriod(
                reduceMotion: false,
                isStatusItemVisible: false
            ) == nil,
            "hidden status icon must not keep a continuous animation alive"
        )

        let stopped = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 0, minimumRPM: 2_000, maximumRPM: 6_000),
        ])
        require(
            stopped.resolvedRotationPeriod(
                reduceMotion: false,
                isStatusItemVisible: true
            ) == nil,
            "stopped fans must not animate in the menu bar"
        )
    }

    private static func checkRotationPhase() {
        let presentation = FanStatusPresentation.resolve(samples: [
            FanRotationSample(currentRPM: 2_300, minimumRPM: 2_000, maximumRPM: 6_000),
        ])

        require(approximatelyEqual(presentation.angle(at: 0), 0), "animation did not start at zero")
        require(approximatelyEqual(presentation.angle(at: 0.6), 90), "quarter turn phase is incorrect")
        require(approximatelyEqual(presentation.angle(at: 2.4), 0), "full turn did not wrap cleanly")
    }

    private static func checkStatusIconUsesGeometricCenter() {
        let fallbackFrame = StatusItemIconLayout.frame(
            containerSize: CGSize(width: 0, height: 0),
            iconSide: 18
        )
        require(
            fallbackFrame == StatusItemIconFrame(
                x: 3,
                y: 3,
                width: 18,
                height: 18
            ),
            "zero-sized status button did not use the 24-point fallback box"
        )

        let measuredFrame = StatusItemIconLayout.frame(
            containerSize: CGSize(width: 30, height: 26),
            iconSide: 18
        )
        require(
            measuredFrame.midX == 15 && measuredFrame.midY == 13,
            "status icon was not centered in the measured button bounds"
        )
        require(
            StatusItemIconLayout.rotationAnchorX == 0.5
                && StatusItemIconLayout.rotationAnchorY == 0.5,
            "status icon rotation anchor was not its geometric center"
        )
    }

    private static func approximatelyEqual(_ lhs: Double, _ rhs: Double) -> Bool {
        abs(lhs - rhs) < 0.001
    }

    private static func require(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("Status item presentation check failed: \(message)\n", stderr)
            exit(1)
        }
    }
}
