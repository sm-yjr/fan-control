import Foundation

/// Validates fan ids before any SMC key is constructed from them.
///
/// Fan keys embed the id as a single character ("F0Md", "F2Ac"). An
/// unchecked id interpolated into a key ("F10Md", "F-1Ac") violates the
/// four-character requirement and trips `FourCharCode`'s precondition,
/// crashing the root daemon — potentially while fans are still forced.
enum FanIDBounds {
    static let maximumRepresentableFanId = 9

    /// Validates the live `FNum` reading and returns the enumerable fan-id
    /// count, capped at what can form valid four-character SMC keys.
    ///
    /// nil, NaN, infinity, negative, and fractional readings all fail closed
    /// to `nil` — discovery treats that as "no controllable fans" and the
    /// SMCKit write path refuses the request. A corrupt count above the cap
    /// is clamped instead of converted, so `Int()` can never trap and no
    /// "F10md"-style key can ever be interpolated.
    static func validFanCount(_ fanCount: Double?) -> Int? {
        guard let fanCount, fanCount.isFinite, fanCount >= 0 else { return nil }
        guard fanCount == fanCount.rounded(.down) else { return nil }
        let maximumCount = maximumRepresentableFanId + 1
        if fanCount > Double(maximumCount) { return maximumCount }
        return Int(fanCount)
    }

    /// `fanCount` is the live `FNum` reading, shared with discovery through
    /// `validFanCount` so both paths agree on which ids exist.
    static func isValidFanId(_ fanId: Int, fanCount: Double?) -> Bool {
        guard fanId >= 0 else { return false }
        guard let count = validFanCount(fanCount) else { return false }
        return fanId < count
    }
}

/// Validates a requested RPM against live hardware limits before any SMC
/// encoding happens.
///
/// The helper is reachable by the console user, so requests must be treated
/// as untrusted: negative values, absurd magnitudes, and corrupt or missing
/// `F<n>Mx` readings must all be refused instead of reaching the FPE2/FLT
/// encoders (where a negative Int traps and an unknown maximum means the
/// write is unbounded).
enum FanRPMBounds {
    /// The FPE2 target encoding uses a 14-bit fixed-point field; anything
    /// above this cannot be represented and is refused even if a corrupt
    /// maximum register claimed to allow it.
    static let absoluteMaximumRPM = 16_383

    /// Returns the safe, clamped RPM to write, or nil when the write must be
    /// refused.
    ///
    /// `minimumRPM`/`maximumRPM` must be the live `F<n>Mn`/`F<n>Mx`
    /// readings. Both must be finite, safely convertible to Int, and ordered
    /// `0 <= min < max`; any violation fails closed. A positive request is
    /// clamped into `[min, max]` and can never collapse to 0 — only an
    /// explicit fan-off request (`0` with `allowFanOff`) may stop a fan.
    static func validatedRPM(
        requestedRPM: Int,
        minimumRPM: Double?,
        maximumRPM: Double?,
        allowFanOff: Bool
    ) -> Int? {
        guard requestedRPM >= 0 else { return nil }
        if requestedRPM == 0 {
            return allowFanOff ? 0 : nil
        }

        guard let minimumRPM, minimumRPM.isFinite, minimumRPM >= 0,
              let minimum = safeInt(minimumRPM),
              minimum <= absoluteMaximumRPM else {
            return nil
        }
        guard let maximumRPM, maximumRPM.isFinite,
              let maximum = safeInt(maximumRPM),
              maximum > minimum else {
            return nil
        }

        let clamped = min(max(requestedRPM, minimum), maximum, absoluteMaximumRPM)
        // Defense in depth: a positive request must never be clamped to a
        // stop.
        return clamped > 0 ? clamped : nil
    }

    private static func safeInt(_ value: Double) -> Int? {
        guard value.isFinite, value >= 0, value < Double(Int.max) else { return nil }
        return Int(value)
    }
}

/// Failure decisions for the forced-mode target-write path, extracted so
/// they can be exercised statically without touching real SMC hardware.
enum FanTargetWritePolicy {
    /// Target key data types the writer can encode. Anything else must be
    /// refused: writing the freshly-read bytes back would report success for
    /// a target that was never actually set.
    static func isEncodableTargetType(_ dataType: String) -> Bool {
        dataType == "flt " || dataType == "fpe2"
    }

    /// A fan that is (or just became) forced and whose new target did not
    /// land must be returned to system control; leaving it forced at a
    /// stale target is the exact failure this app exists to prevent.
    static func requiresAutomaticFallback(
        hardwareForced: Bool,
        targetConfirmed: Bool
    ) -> Bool {
        hardwareForced && !targetConfirmed
    }
}

/// Validates the speed range discovered from SMC so no consumer can ever
/// build an inverted or non-finite `ClosedRange` (which traps).
///
/// A failed `F<n>Mx` read used to fall back to `1`, producing ranges like
/// `1500...1`; a later revision fell back to `6000`, which fabricated a
/// writable maximum for hardware whose real limit is unknown. Neither is
/// acceptable: when the maximum is unknown or invalid the range stays legal
/// for display but is marked uncontrollable, and callers must disable manual
/// control and RPM writes for that fan instead of widening them.
enum FanSpeedRange {
    static func validated(
        minSpeed: Double?,
        maxSpeed: Double?
    ) -> (minSpeed: Double, maxSpeed: Double, isControllable: Bool) {
        // Controllable requires BOTH bounds: an unknown minimum would let
        // manual control start at 0, and an unknown maximum has no safe
        // target at all.
        if let minSpeed, minSpeed.isFinite, minSpeed >= 0,
           let maxSpeed, maxSpeed.isFinite, maxSpeed > minSpeed {
            return (minSpeed, maxSpeed, true)
        }

        // Unknown or invalid bounds: keep a minimal legal range for
        // presentation only and forbid user-controlled writes.
        let lower: Double
        if let minSpeed, minSpeed.isFinite, minSpeed >= 0 {
            lower = minSpeed
        } else {
            lower = 0
        }
        return (lower, lower + 1, false)
    }

    /// Defensive bounds for UI controls; tolerates already-stored fan data
    /// that predates discovery-time validation.
    static func sliderBounds(
        minSpeed: Double,
        maxSpeed: Double
    ) -> (lowerBound: Double, upperBound: Double) {
        let lower = minSpeed.isFinite && minSpeed >= 0 ? minSpeed : 0
        let upper = maxSpeed.isFinite && maxSpeed > lower ? maxSpeed : lower + 1
        return (lower, upper)
    }

    /// Sensor readings feed `Int()` conversions in the UI and controller; a
    /// corrupt FLT reading (NaN, infinity, negative) must be normalized at
    /// the snapshot boundary instead of trapping later.
    static func sanitizedSpeedReading(_ value: Double?) -> Double {
        guard let value, value.isFinite, value >= 0 else { return 0 }
        return value
    }
}

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
