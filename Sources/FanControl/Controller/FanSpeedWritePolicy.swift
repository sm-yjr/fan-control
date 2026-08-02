import Foundation

enum FanSpeedWritePolicy {
    private static let curveWriteDwell: TimeInterval = 6
    private static let curveImmediateDeltaRPM = 300

    static func targetRPM(
        requestedRPM: Int,
        previousRPM: Int,
        elapsed: TimeInterval,
        controlMode: FanControlMode,
        maximumRampUpPerSecond: Double,
        maximumRampDownPerSecond: Double,
        bypassRampLimit: Bool,
        preservesStartFromStopped: Bool
    ) -> Int {
        if case .manual = controlMode {
            return requestedRPM
        }
        guard !bypassRampLimit else { return requestedRPM }
        guard !preservesStartFromStopped else { return requestedRPM }
        guard previousRPM != requestedRPM else { return requestedRPM }

        let coalescedRPM = coalescedCurveTarget(
            requestedRPM: requestedRPM,
            previousRPM: previousRPM,
            elapsed: elapsed,
            controlMode: controlMode
        )
        guard previousRPM != coalescedRPM else { return previousRPM }

        let isRampUp = coalescedRPM > previousRPM
        let maximumDelta = Int(
            (isRampUp ? maximumRampUpPerSecond : maximumRampDownPerSecond) * max(elapsed, 1)
        )
        guard abs(coalescedRPM - previousRPM) > maximumDelta else { return coalescedRPM }
        return previousRPM + (isRampUp ? maximumDelta : -maximumDelta)
    }

    private static func coalescedCurveTarget(
        requestedRPM: Int,
        previousRPM: Int,
        elapsed: TimeInterval,
        controlMode: FanControlMode
    ) -> Int {
        guard case .curve = controlMode,
              requestedRPM > 0,
              previousRPM > 0,
              elapsed > 0,
              elapsed < curveWriteDwell,
              abs(requestedRPM - previousRPM) < curveImmediateDeltaRPM else {
            return requestedRPM
        }
        return previousRPM
    }
}
