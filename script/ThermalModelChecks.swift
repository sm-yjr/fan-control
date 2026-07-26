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
