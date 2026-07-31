import Foundation

enum FanSpeedWritePolicy {
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

        let isRampUp = requestedRPM > previousRPM
        let maximumDelta = Int(
            (isRampUp ? maximumRampUpPerSecond : maximumRampDownPerSecond) * max(elapsed, 1)
        )
        guard abs(requestedRPM - previousRPM) > maximumDelta else { return requestedRPM }
        return previousRPM + (isRampUp ? maximumDelta : -maximumDelta)
    }
}
