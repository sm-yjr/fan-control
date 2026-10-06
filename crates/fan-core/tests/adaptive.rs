use fan_core::*;

fn sample(now: f64, cpu: Option<f64>, gpu: Option<f64>, load: Option<f64>) -> Snapshot {
    Snapshot {
        sampled_at: now,
        fan_count: Some(1.0),
        thermal_pressure: ThermalPressure::Nominal,
        cpu_utilization_percent: load,
        machine_id: Some("device-a".into()),
        fans: vec![Fan {
            id: 0,
            name: "fan".into(),
            min_rpm: Some(1500.0),
            max_rpm: Some(6000.0),
            current_rpm: Some(1800.0),
            mode: HardwareMode::Automatic,
        }],
        sensors: vec![
            Sensor {
                key: "cpu".into(),
                name: "CPU".into(),
                group: SensorGroup::Cpu,
                value: cpu,
            },
            Sensor {
                key: "gpu".into(),
                name: "GPU".into(),
                group: SensorGroup::Gpu,
                value: gpu,
            },
            Sensor {
                key: "body".into(),
                name: "Board proxy".into(),
                group: SensorGroup::System,
                value: Some(30.0),
            },
        ],
    }
}
fn controller(fan: FanConfig) -> Controller {
    Controller::new(Config {
        fans: vec![fan],
        ..Config::default()
    })
}
fn tick(controller: &mut Controller, sample: &mut Snapshot) -> Vec<Action> {
    let actions = controller.update(sample, sample.sampled_at);
    for action in &actions {
        assert!(controller.acknowledge(action, true, sample.sampled_at));
        match action.command {
            Command::SetAutomatic => sample.fans[0].mode = HardwareMode::Automatic,
            Command::SetRpm { rpm, .. } => {
                sample.fans[0].mode = HardwareMode::Forced;
                sample.fans[0].current_rpm = Some(rpm as f64);
            }
        }
    }
    actions
}
fn rpm(actions: &[Action]) -> Option<u32> {
    actions.iter().find_map(|action| match action.command {
        Command::SetRpm { rpm, .. } => Some(rpm),
        _ => None,
    })
}
fn calibration() -> SurfaceCalibration {
    SurfaceCalibration::from_measurements("device-a", "body", 40.0, 30.0, 50.0, 40.0).unwrap()
}

#[test]
fn sustained_cpu_load_intervenes_before_a_temperature_only_curve_and_before_pressure() {
    let mut adaptive = controller(FanConfig::adaptive(0));
    let mut curve = controller(FanConfig::balanced(0));
    let mut adaptive_sample = sample(0.0, Some(45.0), None, Some(90.0));
    let mut curve_sample = adaptive_sample.clone();
    for time in [0.0, 2.0, 4.0] {
        adaptive_sample.sampled_at = time;
        curve_sample.sampled_at = time;
        let early = tick(&mut adaptive, &mut adaptive_sample);
        let old = tick(&mut curve, &mut curve_sample);
        assert!(rpm(&old).is_none());
        if time == 4.0 {
            assert!(rpm(&early).unwrap() >= 3000);
            assert_eq!(early[0].reason, ActionReason::AdaptivePerformance);
            assert_eq!(adaptive_sample.thermal_pressure, ThermalPressure::Nominal);
        } else {
            assert!(rpm(&early).is_none());
        }
    }
}
#[test]
fn one_load_spike_and_missing_load_do_not_become_sustained_work() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, Some(45.0), None, Some(0.0));
    for (time, load) in [
        (0.0, Some(0.0)),
        (2.0, Some(100.0)),
        (4.0, None),
        (6.0, Some(0.0)),
    ] {
        state.sampled_at = time;
        state.cpu_utilization_percent = load;
        assert!(rpm(&tick(&mut controller, &mut state)).is_none());
        assert!(!controller.thermal_reading().adaptive.load_sustained);
    }
}
#[test]
fn gpu_prediction_is_not_masked_by_a_hotter_stable_cpu() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, Some(70.0), Some(50.0), None);
    for (time, gpu) in [(0.0, 50.0), (2.0, 55.0), (4.0, 60.0)] {
        state.sampled_at = time;
        state.sensors[1].value = Some(gpu);
        tick(&mut controller, &mut state);
    }
    let reading = &controller.thermal_reading().adaptive;
    assert!(reading.predicted_silicon_celsius.unwrap() > 80.0);
    assert_eq!(
        reading.intervention,
        AdaptiveIntervention::RisingTemperature
    );
}
#[test]
fn adaptive_taking_ownership_never_reduces_the_live_system_cooling_level() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, Some(65.0), None, None);
    let first = tick(&mut controller, &mut state);
    assert!(rpm(&first).unwrap() < 5000);
    state.sampled_at = 2.0;
    state.fans[0].mode = HardwareMode::Automatic;
    state.fans[0].current_rpm = Some(5000.0);
    let after_reset = tick(&mut controller, &mut state);
    assert!(rpm(&after_reset).unwrap() >= 5000);
}
#[test]
fn adaptive_emergency_floors_skip_prediction_and_ramp_confirmation() {
    for (pressure, temperature, floor) in [
        (ThermalPressure::Serious, 45.0, 4650),
        (ThermalPressure::Critical, 45.0, 6000),
        (ThermalPressure::Nominal, 97.0, 5100),
    ] {
        let mut controller = controller(FanConfig::adaptive(0));
        let mut state = sample(0.0, Some(temperature), None, None);
        state.thermal_pressure = pressure;
        let actions = tick(&mut controller, &mut state);
        assert!(rpm(&actions).unwrap() >= floor);
        assert_eq!(actions[0].reason, ActionReason::Emergency);
        assert!(controller.fan_status(0).unwrap().safety_override);
    }
}
#[test]
fn cpu_load_and_a_surface_proxy_cannot_authorize_control_without_silicon_safety_inputs() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, None, None, Some(100.0));
    for time in [0.0, 2.0, 4.0, 6.0] {
        state.sampled_at = time;
        assert!(rpm(&tick(&mut controller, &mut state)).is_none());
        assert!(!controller.thermal_reading().adaptive.available);
    }
    state.sampled_at = 8.0;
    state.sensors[0].value = Some(80.0);
    assert!(rpm(&tick(&mut controller, &mut state)).is_some());
    state.sampled_at = 10.0;
    state.sensors[0].value = None;
    assert!(tick(&mut controller, &mut state).is_empty());
    state.sampled_at = 40.0;
    let actions = tick(&mut controller, &mut state);
    assert_eq!(actions[0].command, Command::SetAutomatic);
    assert_eq!(actions[0].reason, ActionReason::MissingInput);
}
#[test]
fn stale_adaptive_samples_always_hand_back_even_with_high_cpu_load() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, Some(80.0), None, Some(100.0));
    tick(&mut controller, &mut state);
    let actions = controller.update(&state, 16.0);
    assert_eq!(actions[0].command, Command::SetAutomatic);
    assert_eq!(actions[0].reason, ActionReason::StaleSnapshot);
    assert!(!controller.thermal_reading().adaptive.available);
}
#[test]
fn cooling_residence_uses_a_successful_target_and_eventually_returns_to_system() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, Some(45.0), None, Some(100.0));
    for time in [0.0, 2.0, 4.0] {
        state.sampled_at = time;
        tick(&mut controller, &mut state);
    }
    for time in (6..184).step_by(2) {
        state.sampled_at = time as f64;
        state.cpu_utilization_percent = Some(0.0);
        assert!(!tick(&mut controller, &mut state)
            .iter()
            .any(|action| action.command == Command::SetAutomatic));
    }
    state.sampled_at = 184.0;
    assert_eq!(
        tick(&mut controller, &mut state)[0].command,
        Command::SetAutomatic
    );
}
#[test]
fn failed_adaptive_rpm_does_not_arm_a_cooling_residence() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, Some(45.0), None, Some(100.0));
    for time in [0.0, 2.0] {
        state.sampled_at = time;
        tick(&mut controller, &mut state);
    }
    state.sampled_at = 4.0;
    let pending = controller.update(&state, 4.0);
    assert!(rpm(&pending).is_some());
    assert!(controller.acknowledge(&pending[0], false, 4.0));
    state.sampled_at = 6.0;
    state.cpu_utilization_percent = Some(0.0);
    assert_eq!(
        tick(&mut controller, &mut state)[0].command,
        Command::SetAutomatic
    );
    state.sampled_at = 8.0;
    assert!(rpm(&tick(&mut controller, &mut state)).is_none());
}
#[test]
fn heat_soak_keeps_cooling_after_an_instantaneous_temperature_drop() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut state = sample(0.0, Some(90.0), None, None);
    state.sensors[2].value = Some(55.0);
    tick(&mut controller, &mut state);
    state.sampled_at = 2.0;
    state.sensors[0].value = Some(40.0);
    state.sensors[2].value = Some(30.0);
    tick(&mut controller, &mut state);
    assert!(controller.thermal_reading().adaptive.heat_soak_percent > 60.0);
    assert_eq!(
        controller.thermal_reading().adaptive.intervention,
        AdaptiveIntervention::HeatSoak
    );
}
#[test]
fn calibrated_comfort_estimate_requires_the_same_machine_current_sensor_and_both_ranges() {
    let mut policy = ThermalPolicy {
        comfort_target_celsius: Some(38.0),
        calibration: Some(calibration()),
    };
    let mut estimator = AdaptiveEstimator::default();
    let mut state = sample(0.0, Some(45.0), None, None);
    state.sensors[2].value = Some(48.0);
    let thermal = ThermalEstimator::default().update_snapshot(&state, 0.0);
    let active = estimator.update(&state, &thermal, &policy, 0.0);
    assert_eq!(active.estimated_surface_celsius, Some(38.0));
    assert_eq!(active.comfort_status, ComfortStatus::Active);
    state.machine_id = None;
    assert_eq!(
        estimator
            .update(&state, &thermal, &policy, 2.0)
            .comfort_status,
        ComfortStatus::MachineMismatch
    );
    state.machine_id = Some("device-a".into());
    state.sensors[2].value = None;
    assert_eq!(
        estimator
            .update(&state, &thermal, &policy, 2.0)
            .comfort_status,
        ComfortStatus::MissingSensor
    );
    state.sensors[2].value = Some(51.0);
    assert_eq!(
        estimator
            .update(&state, &thermal, &policy, 2.0)
            .comfort_status,
        ComfortStatus::AboveCalibrationRange
    );
    policy.calibration = None;
    let uncalibrated = estimator.update(&state, &thermal, &policy, 2.0);
    assert_eq!(uncalibrated.comfort_status, ComfortStatus::Uncalibrated);
    assert_eq!(uncalibrated.estimated_surface_celsius, None);
}
#[test]
fn comfort_cooling_is_independent_of_low_performance_load_and_remains_bounded() {
    let mut config = Config {
        fans: vec![FanConfig::adaptive(0)],
        ..Config::default()
    };
    config.thermal_policy = ThermalPolicy {
        comfort_target_celsius: Some(35.0),
        calibration: Some(calibration()),
    };
    let mut controller = Controller::new(config);
    let mut state = sample(0.0, Some(45.0), None, Some(0.0));
    state.sensors[2].value = Some(50.0);
    let actions = tick(&mut controller, &mut state);
    assert_eq!(actions[0].reason, ActionReason::SurfaceComfort);
    assert!(rpm(&actions).unwrap() > 3000);
    assert!(rpm(&actions).unwrap() <= 6000);
}
#[test]
fn calibration_rejects_unmeasured_ranges_bad_correlation_and_unstable_offsets() {
    assert!(
        SurfaceCalibration::from_measurements("device-a", "body", 40.0, 30.0, 42.0, 32.0).is_err()
    );
    assert!(
        SurfaceCalibration::from_measurements("device-a", "body", 40.0, 35.0, 50.0, 30.0).is_err()
    );
    assert!(
        SurfaceCalibration::from_measurements("device-a", "body", 40.0, 30.0, 50.0, 45.0).is_err()
    );
    assert!(
        SurfaceCalibration::from_measurements("device-a", "body", 70.0, 20.0, 80.0, 30.0).is_err()
    );
    let mut invalid = calibration();
    invalid.offset_celsius = 0.0;
    assert!(invalid.validate().is_err());
    assert!(ThermalPolicy {
        comfort_target_celsius: Some(46.0),
        calibration: None
    }
    .validate()
    .is_err());
}
#[test]
fn old_rust_configuration_migrates_without_changing_custom_modes_or_claiming_calibration() {
    let old = r#"{"version":1,"fans":[{"fan_id":0,"mode":{"mode":"manual","rpm":1800}}]}"#;
    let load = Config::from_json(old).unwrap();
    assert!(load.migrated);
    assert_eq!(load.config.version, 2);
    assert!(matches!(
        load.config.fans[0].mode,
        ControlMode::Manual { rpm: 1800 }
    ));
    assert_eq!(
        load.config.thermal_policy.comfort_target_celsius,
        Some(38.0)
    );
    assert!(load.config.thermal_policy.calibration.is_none());
    assert_eq!(
        Config::from_json(&load.config.to_json().unwrap())
            .unwrap()
            .config,
        load.config
    );
}
#[test]
fn device_scope_is_persisted_only_in_configuration_and_never_in_snapshot_diagnostics() {
    let mut config = Config::default();
    config.thermal_policy.calibration = Some(calibration());
    assert!(config.to_json().unwrap().contains("device-a"));
    assert!(!serde_json::to_string(&sample(0.0, Some(45.0), None, None))
        .unwrap()
        .contains("device-a"));
}
#[test]
fn changed_comfort_policy_cannot_reuse_a_previously_calibrated_result_from_the_same_snapshot() {
    let mut config = Config {
        fans: vec![FanConfig::adaptive(0)],
        ..Config::default()
    };
    config.thermal_policy = ThermalPolicy {
        comfort_target_celsius: Some(35.0),
        calibration: Some(calibration()),
    };
    let mut controller = Controller::new(config);
    let mut state = sample(0.0, Some(45.0), None, None);
    state.sensors[2].value = Some(50.0);
    assert_eq!(
        tick(&mut controller, &mut state)[0].reason,
        ActionReason::SurfaceComfort
    );
    let mut disabled = controller.config().clone();
    disabled.thermal_policy.comfort_target_celsius = None;
    controller.replace_config(disabled).unwrap();
    let actions = controller.update(&state, 0.0);
    assert!(actions
        .iter()
        .all(|action| action.reason != ActionReason::SurfaceComfort));
    assert_eq!(
        controller.thermal_reading().adaptive.comfort_status,
        ComfortStatus::NotRequested
    );
    assert_eq!(
        controller
            .thermal_reading()
            .adaptive
            .estimated_surface_celsius,
        None
    );
}

fn upper_endpoint_configuration() -> Config {
    Config {
        fans: vec![FanConfig::adaptive(0)],
        thermal_policy: ThermalPolicy {
            comfort_target_celsius: Some(38.0),
            calibration: Some(
                SurfaceCalibration::from_measurements("device-a", "body", 36.0, 32.0, 46.0, 42.0)
                    .unwrap(),
            ),
        },
        ..Config::default()
    }
}

#[test]
fn crossing_the_hot_calibration_endpoint_never_reduces_its_cooling_and_recovers_when_cooler() {
    let mut controller = Controller::new(upper_endpoint_configuration());
    let mut state = sample(0.0, Some(45.0), None, Some(0.0));
    state.sensors[2].value = Some(46.0);
    assert_eq!(rpm(&tick(&mut controller, &mut state)), Some(4209));
    let endpoint_demand = controller.thermal_reading().adaptive.comfort_demand_percent;
    for (time, proxy) in [(2.0, 46.1), (4.0, 47.0), (6.0, 49.0), (8.0, 50.0)] {
        state.sampled_at = time;
        state.sensors[2].value = Some(proxy);
        let actions = tick(&mut controller, &mut state);
        assert!(actions
            .iter()
            .all(|action| matches!(action.command, Command::SetRpm { rpm, .. } if rpm >= 4209)));
        assert!(controller.fan_status(0).unwrap().desired_rpm.unwrap() >= 4209);
        assert!(controller.fan_status(0).unwrap().applied_rpm.unwrap() >= 4209);
        let reading = &controller.thermal_reading().adaptive;
        assert_eq!(reading.estimated_surface_celsius, None);
        assert_eq!(reading.comfort_status, ComfortStatus::AboveCalibrationRange);
        assert!(reading.comfort_demand_percent >= endpoint_demand);
    }
    state.sampled_at = 10.0;
    state.sensors[2].value = Some(44.0);
    let cooler = tick(&mut controller, &mut state);
    assert!(rpm(&cooler).unwrap() < 4209);
    assert_eq!(
        controller.thermal_reading().adaptive.comfort_status,
        ComfortStatus::Active
    );
    assert_eq!(
        controller
            .thermal_reading()
            .adaptive
            .estimated_surface_celsius,
        Some(40.0)
    );
}

#[test]
fn starting_above_calibration_does_not_need_a_prior_ack_and_the_ramp_cannot_undercut_the_floor() {
    let mut controller = Controller::new(upper_endpoint_configuration());
    let mut state = sample(0.0, Some(45.0), None, Some(0.0));
    state.fans[0].mode = HardwareMode::Forced;
    state.fans[0].current_rpm = Some(1500.0);
    state.sensors[2].value = Some(46.1);
    let actions = controller.update(&state, 0.0);
    assert_eq!(rpm(&actions), Some(4209));
    assert_eq!(controller.fan_status(0).unwrap().applied_rpm, None);
    assert_eq!(
        controller
            .thermal_reading()
            .adaptive
            .estimated_surface_celsius,
        None
    );
    assert_eq!(
        controller.thermal_reading().adaptive.comfort_status,
        ComfortStatus::AboveCalibrationRange
    );
}

#[test]
fn both_proxy_and_surface_upper_limits_preserve_the_highest_supported_boundary() {
    for (cold_surface, hot_surface, edge_proxy) in [(33.0, 41.0, 45.0), (31.0, 43.0, 46.0)] {
        let mut config = upper_endpoint_configuration();
        config.thermal_policy.calibration = Some(
            SurfaceCalibration::from_measurements(
                "device-a",
                "body",
                36.0,
                cold_surface,
                46.0,
                hot_surface,
            )
            .unwrap(),
        );
        let mut boundary = Controller::new(config.clone());
        let mut state = sample(0.0, Some(45.0), None, Some(0.0));
        state.sensors[2].value = Some(edge_proxy);
        let edge = rpm(&tick(&mut boundary, &mut state)).unwrap();
        let edge_demand = boundary.thermal_reading().adaptive.comfort_demand_percent;
        let mut cold_start = Controller::new(config);
        state.fans[0].mode = HardwareMode::Forced;
        state.fans[0].current_rpm = Some(1500.0);
        state.sensors[2].value = Some(edge_proxy + 0.1);
        assert!(rpm(&cold_start.update(&state, 0.0)).unwrap() >= edge);
        let above = &cold_start.thermal_reading().adaptive;
        assert_eq!(above.estimated_surface_celsius, None);
        assert_eq!(above.comfort_status, ComfortStatus::AboveCalibrationRange);
        assert!(above.comfort_demand_percent >= edge_demand);
    }
}

#[test]
fn the_cold_side_of_calibration_does_not_claim_a_temperature_or_keep_a_hot_comfort_floor() {
    let mut controller = Controller::new(upper_endpoint_configuration());
    let mut state = sample(0.0, Some(45.0), None, Some(0.0));
    state.sensors[2].value = Some(35.9);
    assert!(rpm(&tick(&mut controller, &mut state)).is_none());
    let reading = &controller.thermal_reading().adaptive;
    assert_eq!(reading.estimated_surface_celsius, None);
    assert_eq!(reading.comfort_status, ComfortStatus::BelowCalibrationRange);
    assert_eq!(reading.comfort_demand_percent, 0.0);
}

#[test]
fn hot_endpoint_fallback_still_requires_same_machine_current_valid_sensor_and_valid_policy() {
    let config = upper_endpoint_configuration();
    let policy = &config.thermal_policy;
    let mut state = sample(0.0, Some(45.0), None, Some(0.0));
    state.sensors[2].value = Some(60.0);
    let thermal = ThermalReading::default();
    let mut estimator = AdaptiveEstimator::default();
    for identity in [None, Some("other-device".into())] {
        state.machine_id = identity;
        let reading = estimator.update(&state, &thermal, policy, 0.0);
        assert_eq!(reading.comfort_status, ComfortStatus::MachineMismatch);
        assert_eq!(reading.comfort_demand_percent, 0.0);
        assert_eq!(reading.estimated_surface_celsius, None);
    }
    state.machine_id = Some("device-a".into());
    for value in [None, Some(f64::NAN), Some(f64::INFINITY), Some(120.0)] {
        state.sensors[2].value = value;
        let reading = estimator.update(&state, &thermal, policy, 0.0);
        assert_eq!(reading.comfort_status, ComfortStatus::MissingSensor);
        assert_eq!(reading.comfort_demand_percent, 0.0);
    }
    state.sensors[2].value = Some(60.0);
    let mut invalid_policy = policy.clone();
    invalid_policy.calibration.as_mut().unwrap().offset_celsius = 0.0;
    let invalid = estimator.update(&state, &thermal, &invalid_policy, 0.0);
    assert_eq!(invalid.comfort_status, ComfortStatus::OutOfRange);
    assert_eq!(invalid.comfort_demand_percent, 0.0);
    let mut disabled = policy.clone();
    disabled.comfort_target_celsius = None;
    assert_eq!(
        estimator
            .update(&state, &thermal, &disabled, 0.0)
            .comfort_demand_percent,
        0.0
    );
}

#[test]
fn hot_calibration_fallback_cannot_override_missing_silicon_staleness_or_missing_live_bounds() {
    let mut state = sample(0.0, None, None, Some(0.0));
    state.sensors[2].value = Some(46.1);
    state.fans[0].mode = HardwareMode::Forced;
    let mut no_silicon = Controller::new(upper_endpoint_configuration());
    let actions = no_silicon.update(&state, 0.0);
    assert_eq!(actions.len(), 1);
    assert!(actions
        .iter()
        .all(|action| action.command == Command::SetAutomatic));
    assert!(!no_silicon.thermal_reading().adaptive.available);
    state.sensors[0].value = Some(45.0);
    let mut stale = Controller::new(upper_endpoint_configuration());
    tick(&mut stale, &mut state);
    let stale_actions = stale.update(&state, 16.0);
    assert_eq!(stale_actions.len(), 1);
    assert!(stale_actions
        .iter()
        .all(|action| action.command == Command::SetAutomatic));
    assert_eq!(
        stale.thermal_reading().adaptive.estimated_surface_celsius,
        None
    );
    assert_eq!(stale.thermal_reading().adaptive.comfort_demand_percent, 0.0);
    assert_eq!(
        stale.thermal_reading().adaptive.comfort_status,
        ComfortStatus::MissingSensor
    );
    let mut unbounded = Controller::new(upper_endpoint_configuration());
    state.fans[0].max_rpm = None;
    let unbounded_actions = unbounded.update(&state, 0.0);
    assert_eq!(unbounded_actions.len(), 1);
    assert!(unbounded_actions
        .iter()
        .all(|action| action.command == Command::SetAutomatic));
}
