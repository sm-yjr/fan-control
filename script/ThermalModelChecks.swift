import Foundation

@main
enum ThermalModelChecks {
    static func main() {
        checkShortSpikeRejection()
        checkSustainedHeatResponse()
        checkSlowCooling()
        checkSafetyFloors()
        checkBalancedCurve()
        checkLegacyMigration()
        checkControlSourceScaleChange()
        checkPollingCadence()
        checkManualSpeedWritePolicy()
        checkFanSpeedRangeValidation()
        checkCurveInputFailSafe()
        checkFanIDBounds()
        checkFanCountValidation()
        checkFanRPMBounds()
        checkFanTargetWritePolicy()
        checkSanitizedSpeedReading()
        checkHandBackWaiterIsNotMainBound()
        checkHandBackWaiterTimesOut()
        checkHelperWriteOrderingIsFIFO()
        checkSleepHandBackPolicy()
        checkDuplicateCurvePoints()
        print("Thermal model checks passed")
    }

    private static func checkShortSpikeRejection() {
        var estimator = ThermalDemandEstimator()
        var reading = ThermalDemandReading()
        for _ in 0..<30 {
            reading = estimator.update(
                siliconTemperature: 45,
                chassisTemperature: 32,
                pressure: .nominal,
                elapsed: 2
            )
        }
        let idleDemand = reading.demandPercent
        reading = estimator.update(
            siliconTemperature: 90,
            chassisTemperature: 32,
            pressure: .nominal,
            elapsed: 2
        )

        require(reading.demandPercent - idleDemand < 5, "short spike changed demand too much")
        require(reading.sustainedSiliconTemperature < 50, "short spike bypassed source filter")
    }

    private static func checkSustainedHeatResponse() {
        var estimator = ThermalDemandEstimator()
        for _ in 0..<30 {
            _ = estimator.update(
                siliconTemperature: 45,
                chassisTemperature: 32,
                pressure: .nominal,
                elapsed: 2
            )
        }

        var reading = ThermalDemandReading()
        for step in 1...60 {
            reading = estimator.update(
                siliconTemperature: 90,
                chassisTemperature: 32 + 13 * Double(step) / 60,
                pressure: .nominal,
                elapsed: 2
            )
        }

        require(reading.demandPercent > 30, "sustained heat did not raise demand")
        require(reading.sustainedSiliconTemperature > 85, "source filter did not converge")
        require(reading.chassisTemperature > 36, "chassis filter did not track heat soak")
    }

    private static func checkSlowCooling() {
        var estimator = ThermalDemandEstimator()
        var hotReading = ThermalDemandReading()
        for _ in 0..<90 {
            hotReading = estimator.update(
                siliconTemperature: 90,
                chassisTemperature: 50,
                pressure: .nominal,
                elapsed: 2
            )
        }

        let firstCoolReading = estimator.update(
            siliconTemperature: 45,
            chassisTemperature: 32,
            pressure: .nominal,
            elapsed: 2
        )

        require(
            firstCoolReading.demandPercent > hotReading.demandPercent * 0.8,
            "cooling demand collapsed after one cool sample"
        )
        require(firstCoolReading.chassisTemperature > 45, "chassis thermal mass cooled unrealistically fast")
    }

    private static func checkSafetyFloors() {
        var estimator = ThermalDemandEstimator()
        let serious = estimator.update(
            siliconTemperature: 45,
            chassisTemperature: 30,
            pressure: .serious,
            elapsed: 2
        )
        let critical = estimator.update(
            siliconTemperature: 45,
            chassisTemperature: 30,
            pressure: .critical,
            elapsed: 2
        )
        let emergency = estimator.update(
            siliconTemperature: 45,
            chassisTemperature: 30,
            emergencySiliconTemperature: 100,
            pressure: .nominal,
            elapsed: 2
        )

        require(serious.demandPercent >= 75, "serious thermal state floor is too low")
        require(critical.demandPercent == 100, "critical thermal state must demand full cooling")
        require(emergency.demandPercent >= 85, "raw silicon emergency guard did not trigger")
    }

    private static func checkBalancedCurve() {
        let curve = FanCurveConfig.defaultCurve(sensorKey: CurveInput.thermalDemandKey)
        require(FanCurveConfig.isFanOffSpeed(curve.interpolate(temperature: 18)), "idle fan should be off")
        require(approximatelyEqual(curve.interpolate(temperature: 28), 0), "fan should start at minimum RPM")
        require(approximatelyEqual(curve.interpolate(temperature: 60), 25), "mid-load curve changed")
        require(approximatelyEqual(curve.interpolate(temperature: 100), 100), "full load must map to full RPM")
        require(approximatelyEqual(curve.hysteresis, 8), "thermal load hysteresis changed")
    }

    private static func checkLegacyMigration() {
        let id = UUID()
        let legacy = FanCurveConfig(
            id: id,
            name: "Default",
            sensorKey: "Average CPU",
            points: [
                CurvePoint(temperature: 35, fanSpeed: FanCurveConfig.fanOffSpeed),
                CurvePoint(temperature: 42, fanSpeed: FanCurveConfig.fanOffSpeed),
                CurvePoint(temperature: 50, fanSpeed: 0),
                CurvePoint(temperature: 60, fanSpeed: 25),
                CurvePoint(temperature: 70, fanSpeed: 45),
                CurvePoint(temperature: 80, fanSpeed: 70),
                CurvePoint(temperature: 90, fanSpeed: 100),
            ],
            hysteresis: 3,
            presetVersion: nil
        )
        let encoded = try! JSONEncoder().encode(legacy)
        let decoded = try! JSONDecoder().decode(FanCurveConfig.self, from: encoded)
        let migrated = decoded.migratedLegacyDefault()

        require(migrated?.id == id, "migration changed curve identifier")
        require(migrated?.sensorKey == CurveInput.thermalDemandKey, "legacy default did not migrate")
        require(migrated?.presetVersion == FanCurveConfig.currentPresetVersion, "migration version is missing")
    }

    private static func checkControlSourceScaleChange() {
        var curve = FanCurveConfig.defaultCurve(sensorKey: CurveInput.thermalDemandKey)
        let id = curve.id
        curve.setSensorKey("Average CPU")

        require(curve.id == id, "changing control source changed curve identifier")
        require(curve.sensorKey == "Average CPU", "temperature source was not selected")
        require(curve.points.first?.temperature == 35, "load-scale points leaked into temperature scale")
        require(approximatelyEqual(curve.hysteresis, 4), "temperature hysteresis was not restored")
    }

    private static func checkPollingCadence() {
        require(
            SensorPollingPolicy.interval(
                isPopoverPresented: false,
                thermalPressure: .nominal,
                hottestSiliconTemperature: 55,
                activity: .automatic
            ) == 8,
            "automatic background polling did not enter the low-wakeup cadence"
        )
        require(
            SensorPollingPolicy.interval(
                isPopoverPresented: false,
                thermalPressure: .nominal,
                hottestSiliconTemperature: 55,
                activity: .manual
            ) == 5,
            "manual mode polling became too slow for reconciliation"
        )
        require(
            SensorPollingPolicy.interval(
                isPopoverPresented: false,
                thermalPressure: .nominal,
                hottestSiliconTemperature: 55,
                activity: .curve
            ) == 2,
            "curve mode lost its responsive polling cadence"
        )
        require(
            SensorPollingPolicy.interval(
                isPopoverPresented: false,
                thermalPressure: .serious,
                hottestSiliconTemperature: 55,
                activity: .automatic
            ) == 2,
            "serious thermal pressure did not restore responsive polling"
        )
        require(
            SensorPollingPolicy.interval(
                isPopoverPresented: true,
                thermalPressure: .nominal,
                hottestSiliconTemperature: 55,
                activity: .automatic
            ) == 2,
            "visible UI did not restore responsive polling"
        )
    }

    private static func checkManualSpeedWritePolicy() {
        let requestedRPM = 4_599
        let previousRPM = 2_300
        let manualTarget = FanSpeedWritePolicy.targetRPM(
            requestedRPM: requestedRPM,
            previousRPM: previousRPM,
            elapsed: 1,
            controlMode: .manual(rpm: requestedRPM),
            maximumRampUpPerSecond: 350,
            maximumRampDownPerSecond: 250,
            bypassRampLimit: false,
            preservesStartFromStopped: false
        )
        require(
            manualTarget == requestedRPM,
            "manual speed change was truncated by the curve ramp limit"
        )

        let curveTarget = FanSpeedWritePolicy.targetRPM(
            requestedRPM: requestedRPM,
            previousRPM: previousRPM,
            elapsed: 1,
            controlMode: .curve(configId: UUID()),
            maximumRampUpPerSecond: 350,
            maximumRampDownPerSecond: 250,
            bypassRampLimit: false,
            preservesStartFromStopped: false
        )
        require(curveTarget == 2_650, "curve speed change lost its ramp limit")
    }

    private static func checkFanSpeedRangeValidation() {
        // Regression: a failed F<n>Mx discovery read used to fall back to 1
        // while F<n>Mn read 1500, producing the trapping range 1500...1.
        let inverted = FanSpeedRange.validated(minSpeed: 1500, maxSpeed: 1)
        require(inverted.minSpeed == 1500, "valid minimum was discarded")
        require(inverted.maxSpeed > inverted.minSpeed, "inverted range survived validation")
        require(!inverted.isControllable, "inverted range must not be controllable")

        // Regression: a missing maximum must not be fabricated into a wide
        // writable range; the fan becomes display-only and uncontrollable.
        let missingMax = FanSpeedRange.validated(minSpeed: 0, maxSpeed: nil)
        require(missingMax.maxSpeed > missingMax.minSpeed, "missing max produced an illegal range")
        require(!missingMax.isControllable, "missing max must disable manual control")
        require(missingMax.maxSpeed <= missingMax.minSpeed + 1, "missing max fabricated a writable upper bound")

        let missingBoth = FanSpeedRange.validated(minSpeed: nil, maxSpeed: nil)
        require(missingBoth.minSpeed >= 0 && missingBoth.maxSpeed > missingBoth.minSpeed, "fully missing range illegal")
        require(!missingBoth.isControllable, "fully missing range must not be controllable")

        let healthy = FanSpeedRange.validated(minSpeed: 1500, maxSpeed: 5200)
        require(healthy.minSpeed == 1500 && healthy.maxSpeed == 5200, "healthy range was modified")
        require(healthy.isControllable, "healthy range lost controllability")

        // An unknown minimum is not controllable either: manual control
        // would start at 0 without a trustworthy lower bound.
        let notANumber = FanSpeedRange.validated(minSpeed: .nan, maxSpeed: 5000)
        require(notANumber.maxSpeed > notANumber.minSpeed, "NaN minimum produced an illegal range")
        require(!notANumber.isControllable, "unknown minimum must disable manual control")

        let missingMin = FanSpeedRange.validated(minSpeed: nil, maxSpeed: 5000)
        require(!missingMin.isControllable, "missing minimum must disable manual control")

        let infinite = FanSpeedRange.validated(minSpeed: 1500, maxSpeed: .infinity)
        require(infinite.maxSpeed.isFinite, "infinite maximum survived validation")
        require(!infinite.isControllable, "infinite maximum must disable manual control")

        let equal = FanSpeedRange.validated(minSpeed: 2000, maxSpeed: 2000)
        require(equal.maxSpeed > equal.minSpeed, "equal bounds survived validation")
        require(!equal.isControllable, "equal bounds must not be controllable")

        // UI defense: no stored data may construct an illegal ClosedRange.
        let crashBounds = FanSpeedRange.sliderBounds(minSpeed: 1500, maxSpeed: 1)
        require(crashBounds.upperBound >= crashBounds.lowerBound, "slider bounds still invertible")
        _ = crashBounds.lowerBound...crashBounds.upperBound

        let nanBounds = FanSpeedRange.sliderBounds(minSpeed: .nan, maxSpeed: .nan)
        require(nanBounds.upperBound >= nanBounds.lowerBound, "NaN slider bounds not sanitized")
        _ = nanBounds.lowerBound...nanBounds.upperBound

        let normalBounds = FanSpeedRange.sliderBounds(minSpeed: 0, maxSpeed: 5000)
        require(normalBounds.lowerBound == 0 && normalBounds.upperBound == 5000, "normal slider bounds modified")
    }

    private static func checkFanIDBounds() {
        // Invalid ids interpolated into SMC keys crash the daemon; every
        // refusal here must fail closed instead.
        require(!FanIDBounds.isValidFanId(-1, fanCount: 2), "negative fan id accepted")
        require(FanIDBounds.isValidFanId(0, fanCount: 2), "valid fan id rejected")
        require(FanIDBounds.isValidFanId(1, fanCount: 2), "valid fan id rejected")
        require(!FanIDBounds.isValidFanId(2, fanCount: 2), "out-of-range fan id accepted")
        require(FanIDBounds.isValidFanId(9, fanCount: 10), "representable boundary id rejected")
        require(!FanIDBounds.isValidFanId(10, fanCount: 12), "id beyond one key character accepted")
        require(!FanIDBounds.isValidFanId(24, fanCount: 25), "corrupt large fan count accepted")
        require(!FanIDBounds.isValidFanId(1, fanCount: nil), "missing fan count accepted")
        require(!FanIDBounds.isValidFanId(1, fanCount: .nan), "NaN fan count accepted")
        require(!FanIDBounds.isValidFanId(1, fanCount: -3), "negative fan count accepted")
    }

    private static func checkFanRPMBounds() {
        // Untrusted requests and corrupt bounds must be refused before any
        // encoding. Both live bounds are mandatory for positive requests.
        require(FanRPMBounds.validatedRPM(requestedRPM: -1, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: true) == nil, "negative rpm accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: -5000, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: true) == nil, "large negative rpm accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: 0, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: true) == 0, "fan-off rpm rejected")
        require(FanRPMBounds.validatedRPM(requestedRPM: 0, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: false) == nil, "fan-off rpm accepted outside the fan-off path")

        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: nil, maximumRPM: 5200, allowFanOff: true) == nil, "write without a minimum accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: .nan, maximumRPM: 5200, allowFanOff: true) == nil, "NaN minimum accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: 1500, maximumRPM: nil, allowFanOff: true) == nil, "write without a maximum accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: 1500, maximumRPM: .nan, allowFanOff: true) == nil, "NaN maximum accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: 1500, maximumRPM: .infinity, allowFanOff: true) == nil, "infinite maximum accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: 1500, maximumRPM: -12, allowFanOff: true) == nil, "negative maximum accepted")
        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: -4, maximumRPM: 5200, allowFanOff: true) == nil, "negative minimum accepted")

        // A zero maximum must refuse positive requests, never clamp them to
        // a stop.
        require(FanRPMBounds.validatedRPM(requestedRPM: 2500, minimumRPM: 0, maximumRPM: 0, allowFanOff: true) == nil, "zero maximum turned a request into fan-off")
        require(FanRPMBounds.validatedRPM(requestedRPM: 2500, minimumRPM: 5200, maximumRPM: 1500, allowFanOff: true) == nil, "inverted bounds accepted")

        // Positive requests clamp into [min, max] and can never reach 0.
        require(FanRPMBounds.validatedRPM(requestedRPM: 1, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: true) == 1500, "below-minimum rpm not clamped up")
        require(FanRPMBounds.validatedRPM(requestedRPM: 3000, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: true) == 3000, "normal rpm modified")
        require(FanRPMBounds.validatedRPM(requestedRPM: 99_999, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: true) == 5200, "rpm not clamped to live maximum")
        require(FanRPMBounds.validatedRPM(requestedRPM: .max, minimumRPM: 1500, maximumRPM: 5200, allowFanOff: true) == 5200, "Int.max request not clamped")
        require(FanRPMBounds.validatedRPM(requestedRPM: 20_000, minimumRPM: 1e18, maximumRPM: 1e19, allowFanOff: true) == nil, "corrupt huge minimum accepted")
        require(
            FanRPMBounds.validatedRPM(requestedRPM: 20_000, minimumRPM: 1500, maximumRPM: 1e9, allowFanOff: true) == FanRPMBounds.absoluteMaximumRPM,
            "corrupt huge maximum produced an unencodable rpm"
        )
    }

    private static func checkFanCountValidation() {
        require(FanIDBounds.validFanCount(nil) == nil, "missing fan count accepted")
        require(FanIDBounds.validFanCount(.nan) == nil, "NaN fan count accepted")
        require(FanIDBounds.validFanCount(.infinity) == nil, "infinite fan count accepted")
        require(FanIDBounds.validFanCount(-.infinity) == nil, "negative infinite fan count accepted")
        require(FanIDBounds.validFanCount(-3) == nil, "negative fan count accepted")
        require(FanIDBounds.validFanCount(2.5) == nil, "fractional fan count accepted")
        require(FanIDBounds.validFanCount(0) == 0, "zero fan count rejected")
        require(FanIDBounds.validFanCount(2) == 2, "normal fan count modified")
        require(FanIDBounds.validFanCount(10) == 10, "maximum representable count rejected")
        require(FanIDBounds.validFanCount(11) == 10, "count above the key space not capped")
        require(FanIDBounds.validFanCount(1e19) == 10, "huge corrupt count not capped safely")
        require(!FanIDBounds.isValidFanId(0, fanCount: 2.5), "fractional fan count passed id validation")
    }

    private static func checkFanTargetWritePolicy() {
        require(FanTargetWritePolicy.isEncodableTargetType("flt "), "FLT target type rejected")
        require(FanTargetWritePolicy.isEncodableTargetType("fpe2"), "FPE2 target type rejected")
        require(!FanTargetWritePolicy.isEncodableTargetType("ui8 "), "unknown target type accepted")
        require(!FanTargetWritePolicy.isEncodableTargetType(""), "empty target type accepted")

        require(
            FanTargetWritePolicy.requiresAutomaticFallback(hardwareForced: true, targetConfirmed: false),
            "forced fan with unconfirmed target lost its fallback"
        )
        require(
            !FanTargetWritePolicy.requiresAutomaticFallback(hardwareForced: true, targetConfirmed: true),
            "confirmed target triggered a needless fallback"
        )
        require(
            !FanTargetWritePolicy.requiresAutomaticFallback(hardwareForced: false, targetConfirmed: false),
            "automatic hardware triggered a fallback"
        )
    }

    private static func checkSanitizedSpeedReading() {
        require(FanSpeedRange.sanitizedSpeedReading(nil) == 0, "nil reading not normalized")
        require(FanSpeedRange.sanitizedSpeedReading(.nan) == 0, "NaN reading not normalized")
        require(FanSpeedRange.sanitizedSpeedReading(.infinity) == 0, "infinite reading not normalized")
        require(FanSpeedRange.sanitizedSpeedReading(-250) == 0, "negative reading not normalized")
        require(FanSpeedRange.sanitizedSpeedReading(2300) == 2300, "valid reading modified")
        // The normalized value must survive Int() conversion downstream.
        _ = Int(FanSpeedRange.sanitizedSpeedReading(.nan))
    }

    private static func checkHandBackWaiterIsNotMainBound() {
        // Simulates termination: the main thread blocks while the hand-back
        // completes on its own queue. With completions bound to the main
        // queue this wait would burn its full timeout; it must resolve as
        // soon as the completions land.
        let waiter = FanHandBackWaiter()
        waiter.enter()
        waiter.enter()

        let finished = DispatchSemaphore(value: 0)
        var completed = false
        DispatchQueue.global(qos: .userInitiated).async {
            waiter.completionQueue.async { waiter.leave() }
            waiter.completionQueue.async { waiter.leave() }
            completed = waiter.waitForCompletion(timeout: 5)
            finished.signal()
        }
        require(finished.wait(timeout: .now() + 8) == .success, "hand-back wait deadlocked while main was blocked")
        require(completed, "hand-back wait did not observe the delivered completions")
    }

    private static func checkHandBackWaiterTimesOut() {
        let waiter = FanHandBackWaiter()
        waiter.enter()
        let start = Date()
        let completed = waiter.waitForCompletion(timeout: 0.3)
        let elapsed = Date().timeIntervalSince(start)
        require(!completed, "hand-back wait reported success without completions")
        require(elapsed >= 0.25 && elapsed < 2, "hand-back wait ignored its timeout")
        waiter.leave()
    }

    private static func checkHelperWriteOrderingIsFIFO() {
        // Mirrors FanControlWriter.requestQueue: write requests issued in
        // order must reach the helper in order, so a hand-back's
        // automatic/reset enqueued after an RPM write can never be
        // reordered before it and re-forced by a stale request.
        let requestQueue = DispatchQueue(label: "checks.helper-writes")
        var executionOrder: [String] = []
        let orderLock = NSLock()
        let group = DispatchGroup()

        for label in ["rpm", "automatic", "reset"] {
            group.enter()
            requestQueue.async {
                if label == "rpm" {
                    Thread.sleep(forTimeInterval: 0.05)
                }
                orderLock.lock()
                executionOrder.append(label)
                orderLock.unlock()
                group.leave()
            }
        }
        require(group.wait(timeout: .now() + 5) == .success, "ordering check timed out")
        require(executionOrder == ["rpm", "automatic", "reset"], "helper write order was not preserved")
    }

    private static func checkSleepHandBackPolicy() {
        require(SleepHandBackPolicy.timeout > 0, "sleep hand-back timeout must be bounded and positive")
        require(
            !SleepHandBackPolicy.shouldHandBack(canWriteFans: false, fanCount: 2),
            "sleep hand-back attempted without write access"
        )
        require(
            !SleepHandBackPolicy.shouldHandBack(canWriteFans: true, fanCount: 0),
            "sleep hand-back attempted with no fans"
        )
        require(
            SleepHandBackPolicy.shouldHandBack(canWriteFans: true, fanCount: 2),
            "sleep hand-back skipped despite writable fans"
        )

        // Per-sleep-cycle dedup: two will-sleep sources fire inside one
        // cycle, and only the first may run the hand-back. The window must
        // also self-clear for later cycles (e.g. an aborted sleep with no
        // wake notification).
        require(
            SleepHandBackPolicy.shouldPerformHandBack(now: 100, lastHandBackAt: nil),
            "first sleep hand-back in a session skipped"
        )
        require(
            !SleepHandBackPolicy.shouldPerformHandBack(now: 100, lastHandBackAt: 99.5),
            "second will-sleep source ran a duplicate hand-back"
        )
        require(
            !SleepHandBackPolicy.shouldPerformHandBack(now: 100, lastHandBackAt: 96),
            "hand-back repeated inside the dedup window"
        )
        require(
            SleepHandBackPolicy.shouldPerformHandBack(now: 100, lastHandBackAt: 94),
            "hand-back still suppressed after the dedup window"
        )
    }

    private static func checkDuplicateCurvePoints() {
        // Duplicate temperatures create zero-width segments; interpolation
        // must stay finite instead of dividing by zero.
        let config = FanCurveConfig(
            name: "Duplicate",
            sensorKey: "Average CPU",
            points: [
                CurvePoint(temperature: 40, fanSpeed: 20),
                CurvePoint(temperature: 40, fanSpeed: 80),
                CurvePoint(temperature: 60, fanSpeed: 100),
            ]
        )
        let atDuplicate = config.interpolate(temperature: 40)
        require(atDuplicate.isFinite, "duplicate point produced NaN interpolation")
        require(config.interpolate(temperature: 50).isFinite, "mid segment produced NaN interpolation")

        let allDuplicates = FanCurveConfig(
            name: "AllDuplicate",
            sensorKey: "Average CPU",
            points: [
                CurvePoint(temperature: 40, fanSpeed: 20),
                CurvePoint(temperature: 40, fanSpeed: 60),
            ]
        )
        require(allDuplicates.interpolate(temperature: 40).isFinite, "fully duplicated curve produced NaN")

        let nanInput = FanCurveConfig.defaultCurve(sensorKey: "Average CPU")
        require(nanInput.interpolate(temperature: .nan).isFinite, "NaN input not handled")
    }

    private static func checkCurveInputFailSafe() {
        // Ordinary transient gaps hold the last target instead of inventing
        // data or abandoning safety checks.
        require(
            CurveInputFailSafe.speedPercentWhenInputMissing(
                pressure: .nominal,
                hottestSiliconTemperature: 55
            ) == nil,
            "nominal pressure must hold the last target"
        )
        require(
            CurveInputFailSafe.speedPercentWhenInputMissing(
                pressure: .fair,
                hottestSiliconTemperature: 55
            ) == nil,
            "fair pressure must hold the last target"
        )

        // Thermal pressure floors must never be bypassed while the input is nil.
        require(
            CurveInputFailSafe.speedPercentWhenInputMissing(
                pressure: .serious,
                hottestSiliconTemperature: 55
            ) == 70,
            "serious pressure lost its safety floor"
        )
        require(
            CurveInputFailSafe.speedPercentWhenInputMissing(
                pressure: .critical,
                hottestSiliconTemperature: 55
            ) == 100,
            "critical pressure lost its safety floor"
        )

        // Known extreme silicon temperature forces cooling even without pressure.
        require(
            CurveInputFailSafe.speedPercentWhenInputMissing(
                pressure: .nominal,
                hottestSiliconTemperature: 97
            ) == 80,
            "extreme silicon temperature lost its safety floor"
        )
        require(
            CurveInputFailSafe.speedPercentWhenInputMissing(
                pressure: .nominal,
                hottestSiliconTemperature: .nan
            ) == nil,
            "non-finite silicon temperature must not fabricate a target"
        )
    }

    private static func approximatelyEqual(_ lhs: Double, _ rhs: Double) -> Bool {
        abs(lhs - rhs) < 0.001
    }

    private static func require(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fputs("Thermal model check failed: \(message)\n", stderr)
            exit(1)
        }
    }
}
