use fan_core::*;

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn spike_rejection_and_sustained_heat_are_distinct() {
    let mut estimator = ThermalEstimator::default();
    let mut reading = ThermalReading::default();
    for _ in 0..30 {
        reading = estimator.update(Some(45.0), Some(32.0), None, ThermalPressure::Nominal, 2.0);
    }
    let idle = reading.demand_percent;
    reading = estimator.update(Some(90.0), Some(32.0), None, ThermalPressure::Nominal, 2.0);
    assert!(reading.demand_percent - idle < 5.0);
    assert!(reading.sustained_silicon_temperature < 50.0);
    for step in 1..=60 {
        reading = estimator.update(
            Some(90.0),
            Some(32.0 + 13.0 * step as f64 / 60.0),
            None,
            ThermalPressure::Nominal,
            2.0,
        );
    }
    assert!(reading.demand_percent > 30.0);
    assert!(reading.sustained_silicon_temperature > 85.0);
    assert!(reading.chassis_temperature > 36.0);
}

#[test]
fn an_initial_hot_sample_does_not_claim_a_full_window_of_sustained_heat() {
    let mut estimator = ThermalEstimator::default();
    estimator.update(Some(90.0), Some(30.0), None, ThermalPressure::Nominal, 0.0);
    let next = estimator.update(Some(45.0), Some(30.0), None, ThermalPressure::Nominal, 2.0);
    assert!(next.sustained_silicon_temperature < 60.0);
    for _ in 0..30 {
        let reading = estimator.update(Some(45.0), Some(30.0), None, ThermalPressure::Nominal, 2.0);
        assert!(reading.demand_percent < ADAPTIVE_START_DEMAND_PERCENT);
    }
    let steady = estimator.update(Some(90.0), Some(30.0), None, ThermalPressure::Nominal, 60.0);
    assert!(steady.sustained_silicon_temperature > 45.0);
    assert!(steady.sustained_silicon_temperature < 90.0);
}

#[test]
fn enclosure_mass_cools_slowly_and_wake_reseeds() {
    let mut estimator = ThermalEstimator::default();
    let mut hot = ThermalReading::default();
    for _ in 0..90 {
        hot = estimator.update(Some(90.0), Some(50.0), None, ThermalPressure::Nominal, 2.0);
    }
    let cool = estimator.update(Some(45.0), Some(32.0), None, ThermalPressure::Nominal, 2.0);
    assert!(cool.demand_percent > hot.demand_percent * 0.8);
    assert!(cool.chassis_temperature > 45.0);
    estimator.reset();
    let wake = estimator.update(Some(45.0), Some(32.0), None, ThermalPressure::Nominal, 0.0);
    close(wake.sustained_silicon_temperature, 45.0);
    close(wake.chassis_temperature, 32.0);
    close(wake.chassis_rise_per_minute, 0.0);
}

#[test]
fn pressure_and_raw_emergency_bypass_filters() {
    let mut estimator = ThermalEstimator::default();
    assert!(
        estimator
            .update(Some(45.0), Some(30.0), None, ThermalPressure::Serious, 2.0)
            .demand_percent
            >= 75.0
    );
    close(
        estimator
            .update(Some(45.0), Some(30.0), None, ThermalPressure::Critical, 2.0)
            .demand_percent,
        100.0,
    );
    assert!(
        estimator
            .update(
                Some(45.0),
                Some(30.0),
                Some(100.0),
                ThermalPressure::Nominal,
                2.0
            )
            .demand_percent
            >= 85.0
    );
    assert_eq!(
        missing_input_safety_percent(ThermalPressure::Serious, Some(100.0)),
        Some(80.0)
    );
}

#[test]
fn fewer_hot_gpu_sensors_are_not_diluted_by_cpu_sensor_count() {
    let mut sensors = vec![Sensor {
        key: "GPU".into(),
        name: "GPU".into(),
        group: SensorGroup::Gpu,
        value: Some(90.0),
    }];
    for i in 0..16 {
        sensors.push(Sensor {
            key: format!("cpu{i}"),
            name: "CPU".into(),
            group: SensorGroup::Cpu,
            value: Some(45.0),
        });
    }
    sensors.push(Sensor {
        key: "TW0P".into(),
        name: "Airport".into(),
        group: SensorGroup::System,
        value: Some(110.0),
    });
    let snapshot = Snapshot {
        sampled_at: 0.0,
        fan_count: Some(0.0),
        fans: vec![],
        sensors,
        thermal_pressure: ThermalPressure::Nominal,
        cpu_utilization_percent: None,
        machine_id: None,
    };
    let reading = ThermalEstimator::default().update_snapshot(&snapshot, 0.0);
    close(reading.sustained_silicon_temperature, 90.0);
    assert!(!reading.uses_chassis_sensor);
    assert!(reading.demand_percent > 95.0);
}

#[test]
fn missing_or_corrupt_temperature_is_never_a_current_input() {
    let mut estimator = ThermalEstimator::default();
    estimator.update(Some(90.0), Some(50.0), None, ThermalPressure::Nominal, 2.0);
    let missing = estimator.update(Some(f64::NAN), None, None, ThermalPressure::Nominal, 2.0);
    assert!(!missing.available);
    assert!(missing.demand_percent.is_finite());
    assert_eq!(
        representative_temperature(&[0.0, f64::NAN, 120.0, -1.0]),
        None
    );
    close(
        representative_temperature(&[30.0, 40.0, 50.0, 60.0]).unwrap(),
        52.5,
    );
}

#[test]
fn balanced_curve_and_source_scale_migration_preserve_identity() {
    let mut curve = Curve::balanced("curve-id", THERMAL_DEMAND_KEY);
    assert!(curve.interpolate(18.0) < 0.0);
    close(curve.interpolate(28.0), 0.0);
    close(curve.interpolate(60.0), 25.0);
    close(curve.interpolate(100.0), 100.0);
    curve.set_sensor_key("Average CPU");
    assert_eq!(curve.id, "curve-id");
    close(curve.points[0].temperature, 35.0);
    close(curve.hysteresis, 4.0);
}

#[test]
fn falling_hysteresis_holds_cooling_and_interpolation_cannot_produce_nan() {
    let mut curve = Curve::balanced("id", "Average CPU");
    assert!(curve.interpolate_with_hysteresis(70.0, 35.0, false) >= curve.interpolate(70.0));
    close(curve.interpolate_with_hysteresis(90.0, 35.0, false), 35.0);
    curve.points = vec![
        CurvePoint {
            temperature: 40.0,
            speed_percent: 20.0,
        },
        CurvePoint {
            temperature: 40.0,
            speed_percent: 80.0,
        },
        CurvePoint {
            temperature: 60.0,
            speed_percent: 100.0,
        },
    ];
    assert!(curve.interpolate(40.0).is_finite());
    assert!(curve.interpolate(50.0).is_finite());
    close(curve.interpolate(f64::NAN), 100.0);
    assert!(curve.validate().is_err());
}

#[test]
fn swift_configuration_migrates_only_the_untouched_legacy_preset() {
    let json = r#"[{"fanId":0,"mode":{"curve":{"configId":"ABC"}},"curveConfig":{"id":"ABC","name":"Default","sensorKey":"Average CPU","points":[{"id":"1","temperature":35,"fanSpeed":-20},{"temperature":42,"fanSpeed":-20},{"temperature":50,"fanSpeed":0},{"temperature":60,"fanSpeed":25},{"temperature":70,"fanSpeed":45},{"temperature":80,"fanSpeed":70},{"temperature":90,"fanSpeed":100}],"hysteresis":3},"lastTemperature":88,"lastSpeedPercent":70,"wasRising":false},{"fanId":1,"mode":{"manual":{"rpm":3000}}}]"#;
    let loaded = Config::from_json(json).unwrap();
    assert!(loaded.migrated);
    assert!(loaded.warnings.is_empty());
    let curve = loaded.config.fans[0].curve.as_ref().unwrap();
    assert_eq!(curve.id, "ABC");
    assert_eq!(curve.sensor_key, THERMAL_DEMAND_KEY);
    assert_eq!(curve.preset_version, Some(PRESET_VERSION));
    assert_eq!(
        loaded.config.fans[1].mode,
        ControlMode::Manual { rpm: 3000 }
    );
    let roundtrip = Config::from_json(&loaded.config.to_json().unwrap()).unwrap();
    assert_eq!(roundtrip.config, loaded.config);
    assert!(!roundtrip.migrated);
    let customized =
        Config::from_json(&json.replace("\"fanSpeed\":25", "\"fanSpeed\":26")).unwrap();
    assert_eq!(
        customized.config.fans[0].curve.as_ref().unwrap().sensor_key,
        "Average CPU"
    );
}

#[test]
fn invalid_configuration_fails_closed_without_losing_valid_other_fans() {
    let loaded = Config::from_json(r#"[{"fanId":0,"mode":{"curve":{"configId":"gone"}}},{"fanId":1,"mode":{"manual":{"rpm":0}}}]"#).unwrap();
    assert_eq!(loaded.warnings.len(), 2);
    assert!(loaded
        .config
        .fans
        .iter()
        .all(|fan| matches!(fan.mode, ControlMode::Automatic)));
    assert!(Config::from_json(r#"{"version":999,"fans":[]}"#).is_err());
    assert!(Config::from_json(r#"[{"fanId":10}]"#).is_err());
    assert!(Config::from_json(r#"[{"fanId":0},{"fanId":0}]"#).is_err());
    let mut config = Config {
        version: CONFIG_VERSION,
        thermal_policy: ThermalPolicy::default(),
        adaptive_tuning: AdaptiveTuning::default(),
        fans: vec![FanConfig::balanced(0)],
    };
    config.fans[0].curve.as_mut().unwrap().hysteresis = f64::NAN;
    assert!(config.to_json().is_err());
}

#[test]
fn live_fan_id_and_rpm_bounds_refuse_corruption() {
    for value in [
        None,
        Some(f64::NAN),
        Some(f64::INFINITY),
        Some(-1.0),
        Some(2.5),
    ] {
        assert_eq!(valid_fan_count(value), None);
    }
    assert_eq!(valid_fan_count(Some(1e19)), Some(10));
    assert!(valid_fan_id(9, Some(10.0)));
    assert!(!valid_fan_id(10, Some(12.0)));
    assert!(!valid_fan_id(1, None));
    assert_eq!(validated_rpm(-1, Some(1500.0), Some(5200.0), true), None);
    assert_eq!(validated_rpm(0, None, None, false), None);
    assert_eq!(validated_rpm(0, None, None, true), None);
    assert_eq!(validated_rpm(0, Some(1500.0), Some(5200.0), true), None);
    assert_eq!(validated_rpm(0, Some(0.0), Some(5200.0), true), Some(0));
    for (minimum, maximum) in [
        (None, Some(5200.0)),
        (Some(1500.0), None),
        (Some(f64::NAN), Some(5200.0)),
        (Some(0.0), Some(0.0)),
        (Some(5200.0), Some(1500.0)),
        (Some(1e18), Some(1e19)),
    ] {
        assert_eq!(validated_rpm(3000, minimum, maximum, true), None);
    }
    assert_eq!(
        validated_rpm(1, Some(1500.0), Some(5200.0), false),
        Some(1500)
    );
    assert_eq!(
        validated_rpm(i64::MAX, Some(1500.0), Some(5200.0), false),
        Some(5200)
    );
    assert_eq!(
        validated_rpm(20000, Some(1500.0), Some(1e9), false),
        Some(ABSOLUTE_MAXIMUM_RPM)
    );
    assert_eq!(validated_rpm(1, Some(0.0), Some(5000.0), false), Some(1));
    assert_eq!(
        validated_rpm(1, Some(1500.9), Some(5200.0), false),
        Some(1501)
    );
    assert_eq!(
        validated_rpm(6000, Some(1500.0), Some(5200.9), false),
        Some(5200)
    );
}

#[test]
fn cadence_ramp_and_write_failure_policy_preserve_controls() {
    assert_eq!(
        polling_interval(
            false,
            ThermalPressure::Nominal,
            Some(55.0),
            PollingActivity::Automatic
        ),
        8.0
    );
    assert_eq!(
        polling_interval(
            false,
            ThermalPressure::Nominal,
            Some(55.0),
            PollingActivity::Manual
        ),
        5.0
    );
    assert_eq!(
        polling_interval(
            false,
            ThermalPressure::Nominal,
            Some(55.0),
            PollingActivity::Curve
        ),
        2.0
    );
    assert_eq!(
        polling_interval(
            true,
            ThermalPressure::Nominal,
            None,
            PollingActivity::Automatic
        ),
        2.0
    );
    assert_eq!(
        polling_interval(
            false,
            ThermalPressure::Serious,
            None,
            PollingActivity::Automatic
        ),
        2.0
    );
    assert_eq!(
        ramp_target(
            4599,
            2300,
            1.0,
            &ControlMode::Manual { rpm: 4599 },
            false,
            false
        ),
        4599
    );
    assert_eq!(
        ramp_target(
            4599,
            2300,
            1.0,
            &ControlMode::Curve {
                curve_id: "id".into()
            },
            false,
            false
        ),
        2650
    );
    assert_eq!(
        ramp_target(
            4599,
            2300,
            1.0,
            &ControlMode::Curve {
                curve_id: "id".into()
            },
            true,
            false
        ),
        4599
    );
    assert!(target_type_encodable("flt "));
    assert!(target_type_encodable("fpe2"));
    assert!(!target_type_encodable("ui8 "));
    assert!(requires_automatic_fallback(true, false));
    assert!(!requires_automatic_fallback(true, true));
}

#[test]
fn partially_lost_heat_nodes_expire_without_erasing_valid_samples() {
    let mut estimator = ThermalEstimator::default();
    estimator.update(Some(90.0), Some(50.0), None, ThermalPressure::Nominal, 2.0);
    let transient = estimator.update(None, Some(40.0), None, ThermalPressure::Nominal, 2.0);
    assert!(transient.sustained_silicon_temperature > 80.0);
    assert!(transient.available);
    let mut reading = transient;
    for _ in 0..14 {
        reading = estimator.update(None, Some(40.0), None, ThermalPressure::Nominal, 2.0);
    }
    close(reading.sustained_silicon_temperature, 0.0);
    assert!(reading.chassis_temperature > 40.0);
    assert!(reading.uses_chassis_sensor);
    for _ in 0..15 {
        reading = estimator.update(Some(90.0), None, None, ThermalPressure::Nominal, 2.0);
    }
    assert!(!reading.uses_chassis_sensor);
    close(reading.chassis_temperature, 0.0);
    close(reading.sustained_silicon_temperature, 90.0);
}

#[test]
fn malformed_swift_mode_is_repaired_with_a_visible_warning() {
    let load = Config::from_json(
        r#"[{"fanId":0,"mode":{"manual":{"rpm":-1}}},{"fanId":1,"mode":{"automatic":{}}}]"#,
    )
    .unwrap();
    assert_eq!(load.warnings.len(), 1);
    assert!(load
        .config
        .fans
        .iter()
        .all(|fan| matches!(fan.mode, ControlMode::Automatic)));
}
