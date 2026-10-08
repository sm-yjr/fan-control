use fan_core::*;

fn fan(id: u8, rpm: f64, mode: HardwareMode) -> Fan {
    Fan {
        id,
        name: format!("Fan {id}"),
        min_rpm: Some(1500.0),
        max_rpm: Some(5200.0),
        current_rpm: Some(rpm),
        mode,
    }
}

fn snapshot(now: f64, temperature: Option<f64>, rpm: f64, mode: HardwareMode) -> Snapshot {
    Snapshot {
        sampled_at: now,
        fan_count: Some(1.0),
        fans: vec![fan(0, rpm, mode)],
        sensors: vec![Sensor {
            key: "TC0P".into(),
            name: "CPU".into(),
            group: SensorGroup::Cpu,
            value: temperature,
        }],
        thermal_pressure: ThermalPressure::Nominal,
        cpu_utilization_percent: None,
        machine_id: None,
    }
}

fn stop_capable_snapshot(
    now: f64,
    temperature: Option<f64>,
    rpm: f64,
    mode: HardwareMode,
) -> Snapshot {
    let mut sample = snapshot(now, temperature, rpm, mode);
    sample.fans[0].min_rpm = Some(0.0);
    sample
}

fn controller(config: FanConfig) -> Controller {
    Controller::new(Config {
        version: CONFIG_VERSION,
        thermal_policy: ThermalPolicy::default(),
        adaptive_tuning: AdaptiveTuning::default(),
        fans: vec![config],
    })
}

fn manual(rpm: u32) -> FanConfig {
    FanConfig {
        fan_id: 0,
        mode: ControlMode::Manual { rpm },
        curve: None,
    }
}

fn curve_config() -> FanConfig {
    let mut config = FanConfig::balanced(0);
    let curve = config.curve.as_mut().unwrap();
    curve.sensor_key = "TC0P".into();
    curve.hysteresis = 0.0;
    curve.points = [
        (30.0, -20.0),
        (40.0, -20.0),
        (50.0, 0.0),
        (60.0, 40.0),
        (100.0, 100.0),
    ]
    .into_iter()
    .map(|(temperature, speed_percent)| CurvePoint {
        temperature,
        speed_percent,
    })
    .collect();
    config
}

fn only(actions: Vec<Action>) -> Action {
    assert_eq!(actions.len(), 1, "unexpected actions: {actions:?}");
    actions.into_iter().next().unwrap()
}

fn rpm(action: &Action) -> u32 {
    match action.command {
        Command::SetRpm { rpm, .. } => rpm,
        _ => panic!("expected RPM: {action:?}"),
    }
}

#[test]
fn manual_changes_are_immediate_and_acknowledgement_commits_the_target() {
    let mut controller = controller(manual(4599));
    let action = only(controller.update(
        &snapshot(0.0, Some(55.0), 2300.0, HardwareMode::Forced),
        0.0,
    ));
    assert_eq!(rpm(&action), 4599);
    let status = controller.fan_status(0).unwrap();
    assert_eq!(status.desired_rpm, Some(4599));
    assert_eq!(status.applied_rpm, None);
    assert!(status.pending.is_some());
    assert!(controller
        .update(
            &snapshot(1.0, Some(55.0), 2300.0, HardwareMode::Forced),
            1.0
        )
        .is_empty());
    assert!(controller.acknowledge(&action, true, 1.0));
    assert_eq!(controller.fan_status(0).unwrap().applied_rpm, Some(4599));
    assert!(!controller.acknowledge(&action, true, 1.0));
}

#[test]
fn all_custom_modes_obey_critical_serious_and_raw_emergency_floors() {
    for config in [manual(1500), curve_config()] {
        for (pressure, temperature, expected) in [
            (ThermalPressure::Critical, 35.0, 5200),
            (ThermalPressure::Serious, 35.0, 4090),
            (ThermalPressure::Nominal, 100.0, 4460),
        ] {
            let mut controller = controller(config.clone());
            let mut sample = snapshot(0.0, Some(temperature), 0.0, HardwareMode::Forced);
            sample.thermal_pressure = pressure;
            let action = only(controller.update(&sample, 0.0));
            assert!(rpm(&action) >= expected, "unsafe floor: {action:?}");
            assert_eq!(action.reason, ActionReason::Emergency);
            assert!(controller.fan_status(0).unwrap().safety_override);
        }
    }
}

#[test]
fn curves_ramp_but_emergency_targets_skip_the_limit() {
    let mut controller = controller(curve_config());
    let action = only(controller.update(
        &snapshot(0.0, Some(60.0), 2300.0, HardwareMode::Forced),
        0.0,
    ));
    assert_eq!(rpm(&action), 2650);
    assert_eq!(controller.fan_status(0).unwrap().desired_rpm, Some(2980));
    controller.acknowledge(&action, true, 0.0);
    let ramp = only(controller.update(
        &snapshot(2.0, Some(60.0), 2650.0, HardwareMode::Forced),
        2.0,
    ));
    assert_eq!(rpm(&ramp), 2980);
    controller.acknowledge(&ramp, true, 2.0);
    let mut hot = snapshot(3.0, None, 2980.0, HardwareMode::Forced);
    hot.thermal_pressure = ThermalPressure::Critical;
    assert_eq!(rpm(&only(controller.update(&hot, 3.0))), 5200);
}

#[test]
fn stop_and_start_residence_use_successful_writes() {
    let mut controller = controller(curve_config());
    let stop = only(controller.update(
        &stop_capable_snapshot(0.0, Some(35.0), 0.0, HardwareMode::Forced),
        0.0,
    ));
    assert_eq!(rpm(&stop), 0);
    controller.acknowledge(&stop, true, 0.0);
    assert!(controller
        .update(
            &stop_capable_snapshot(89.0, Some(55.0), 0.0, HardwareMode::Forced),
            89.0
        )
        .is_empty());
    let start = only(controller.update(
        &stop_capable_snapshot(90.0, Some(55.0), 0.0, HardwareMode::Forced),
        90.0,
    ));
    assert_eq!(rpm(&start), 1040);
    controller.acknowledge(&start, true, 90.0);
    let cooling = only(controller.update(
        &stop_capable_snapshot(100.0, Some(35.0), 1040.0, HardwareMode::Forced),
        100.0,
    ));
    assert_eq!(rpm(&cooling), 1);
    controller.acknowledge(&cooling, true, 100.0);
    assert!(controller
        .update(
            &stop_capable_snapshot(269.0, Some(35.0), 1.0, HardwareMode::Forced),
            269.0
        )
        .is_empty());
    let stop = only(controller.update(
        &stop_capable_snapshot(270.0, Some(35.0), 1.0, HardwareMode::Forced),
        270.0,
    ));
    assert_eq!(rpm(&stop), 0);
}

#[test]
fn emergency_cooling_interrupts_off_residence() {
    let mut controller = controller(curve_config());
    let stop = only(controller.update(
        &stop_capable_snapshot(0.0, Some(35.0), 0.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&stop, true, 0.0);
    let mut hot = stop_capable_snapshot(2.0, Some(35.0), 0.0, HardwareMode::Forced);
    hot.thermal_pressure = ThermalPressure::Serious;
    let start = only(controller.update(&hot, 2.0));
    assert_eq!(rpm(&start), 3640);
    assert!(!matches!(
        start.command,
        Command::SetRpm {
            allow_fan_off: true,
            ..
        }
    ));
}

#[test]
fn failed_targets_are_not_committed_and_hand_back_precedes_retry() {
    let mut controller = controller(manual(3000));
    let write = only(controller.update(
        &snapshot(0.0, Some(55.0), 1500.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&write, false, 0.0);
    assert_eq!(controller.fan_status(0).unwrap().applied_rpm, None);
    assert!(controller.fan_status(0).unwrap().last_failure.is_some());
    let handback = only(controller.update(
        &snapshot(1.0, Some(55.0), 1500.0, HardwareMode::Forced),
        1.0,
    ));
    assert_eq!(handback.command, Command::SetAutomatic);
    assert_eq!(handback.reason, ActionReason::WriteFailure);
    controller.acknowledge(&handback, true, 1.0);
    assert!(controller.fan_status(0).unwrap().last_failure.is_some());
    let retry = only(controller.update(
        &snapshot(2.0, Some(55.0), 1500.0, HardwareMode::Automatic),
        2.0,
    ));
    assert_eq!(rpm(&retry), 3000);
    controller.acknowledge(&retry, true, 2.0);
    assert!(controller.fan_status(0).unwrap().last_failure.is_none());
}

#[test]
fn failed_stop_does_not_arm_ninety_second_off_residence() {
    let mut controller = controller(curve_config());
    let stop = only(controller.update(
        &stop_capable_snapshot(0.0, Some(35.0), 0.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&stop, false, 0.0);
    let auto = only(controller.update(
        &stop_capable_snapshot(1.0, Some(35.0), 0.0, HardwareMode::Forced),
        1.0,
    ));
    controller.acknowledge(&auto, true, 1.0);
    let start = only(controller.update(
        &stop_capable_snapshot(2.0, Some(55.0), 0.0, HardwareMode::Automatic),
        2.0,
    ));
    assert_eq!(rpm(&start), 1040);
}

#[test]
fn stale_acknowledgement_cannot_override_a_new_user_mode() {
    let mut controller = controller(manual(3000));
    let old = only(controller.update(
        &snapshot(0.0, Some(55.0), 1500.0, HardwareMode::Forced),
        0.0,
    ));
    controller.set_fan_config(FanConfig::automatic(0)).unwrap();
    assert!(!controller.acknowledge(&old, true, 0.0));
    assert_eq!(controller.fan_status(0).unwrap().applied_rpm, None);
    let auto = only(controller.update(
        &snapshot(1.0, Some(55.0), 3000.0, HardwareMode::Forced),
        1.0,
    ));
    assert_eq!(auto.command, Command::SetAutomatic);
    assert!(!controller.acknowledge(&old, true, 1.0));
    assert_eq!(
        controller.fan_status(0).unwrap().pending,
        Some(auto.clone())
    );
    assert!(controller.acknowledge(&auto, true, 1.0));
}

#[test]
fn missing_curve_input_holds_briefly_then_returns_to_automatic() {
    let mut controller = controller(curve_config());
    let action = only(controller.update(
        &snapshot(0.0, Some(60.0), 2980.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&action, true, 0.0);
    assert!(controller
        .update(&snapshot(2.0, None, 2980.0, HardwareMode::Forced), 2.0)
        .is_empty());
    assert_eq!(
        controller.fan_status(0).unwrap().status_reason,
        Some(ActionReason::MissingInput)
    );
    assert!(controller
        .update(&snapshot(31.0, None, 2980.0, HardwareMode::Forced), 31.0)
        .is_empty());
    let auto = only(controller.update(&snapshot(32.0, None, 2980.0, HardwareMode::Forced), 32.0));
    assert_eq!(auto.command, Command::SetAutomatic);
    assert_eq!(auto.reason, ActionReason::MissingInput);
    controller.acknowledge(&auto, true, 32.0);
    let valid = only(controller.update(
        &snapshot(34.0, Some(60.0), 2980.0, HardwareMode::Automatic),
        34.0,
    ));
    assert_eq!(rpm(&valid), 2980);
}

#[test]
fn fresh_critical_pressure_protects_a_curve_even_without_a_sensor() {
    let mut controller = controller(curve_config());
    let mut sample = snapshot(0.0, None, 0.0, HardwareMode::Forced);
    sample.thermal_pressure = ThermalPressure::Critical;
    let action = only(controller.update(&sample, 0.0));
    assert_eq!(rpm(&action), 5200);
    assert_eq!(action.reason, ActionReason::Emergency);
}

#[test]
fn manual_mode_does_not_outlive_lost_safety_telemetry() {
    let mut controller = controller(manual(3000));
    let write = only(controller.update(
        &snapshot(0.0, Some(55.0), 3000.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&write, true, 0.0);
    assert!(controller
        .update(&snapshot(2.0, None, 3000.0, HardwareMode::Forced), 2.0)
        .is_empty());
    let auto = only(controller.update(&snapshot(32.0, None, 3000.0, HardwareMode::Forced), 32.0));
    assert_eq!(auto.command, Command::SetAutomatic);
}

#[test]
fn stale_snapshot_or_failed_discovery_cannot_drive_custom_writes() {
    let mut controller = controller(manual(3000));
    let sample = snapshot(0.0, Some(55.0), 3000.0, HardwareMode::Forced);
    let first = only(controller.update(&sample, 0.0));
    controller.acknowledge(&first, true, 0.0);
    let auto = only(controller.update(&sample, 16.0));
    assert_eq!(auto.command, Command::SetAutomatic);
    assert_eq!(auto.reason, ActionReason::StaleSnapshot);
    assert!(!controller.thermal_reading().available);
    controller.acknowledge(&auto, true, 16.0);
    let mut failed = snapshot(32.0, None, 3000.0, HardwareMode::Forced);
    failed.fan_count = None;
    let auto = only(controller.update(&failed, 32.0));
    assert_eq!(auto.command, Command::SetAutomatic);
}

#[test]
fn missing_bounds_or_unknown_mode_refuse_forced_targets() {
    for invalid in 0..3 {
        let mut controller = controller(manual(3000));
        let mut sample = snapshot(0.0, Some(55.0), 1500.0, HardwareMode::Forced);
        match invalid {
            0 => sample.fans[0].min_rpm = None,
            1 => sample.fans[0].max_rpm = None,
            _ => sample.fans[0].mode = HardwareMode::Unknown,
        };
        assert_eq!(
            only(controller.update(&sample, 0.0)).command,
            Command::SetAutomatic
        );
    }
}

#[test]
fn zero_minimum_does_not_turn_a_nonnegative_curve_point_into_fan_off() {
    let mut controller = controller(curve_config());
    let mut sample = snapshot(0.0, Some(50.0), 0.0, HardwareMode::Forced);
    sample.fans[0].min_rpm = Some(0.0);
    let action = only(controller.update(&sample, 0.0));
    assert_eq!(rpm(&action), 1);
    assert_eq!(
        action.command,
        Command::SetRpm {
            rpm: 1,
            allow_fan_off: false
        }
    );
}

#[test]
fn mode_reconcile_uses_real_hardware_mode_and_rpm_mismatch_retries() {
    let mut controller = controller(manual(3000));
    let write = only(controller.update(
        &snapshot(0.0, Some(55.0), 3000.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&write, true, 0.0);
    let mode = only(controller.update(
        &snapshot(1.0, Some(55.0), 3000.0, HardwareMode::Automatic),
        1.0,
    ));
    assert_eq!(mode.reason, ActionReason::ModeReconcile);
    controller.acknowledge(&mode, true, 1.0);
    assert!(controller
        .update(
            &snapshot(2.0, Some(55.0), 1500.0, HardwareMode::Forced),
            2.0
        )
        .is_empty());
    let retry = only(controller.update(
        &snapshot(12.0, Some(55.0), 1500.0, HardwareMode::Forced),
        12.0,
    ));
    assert_eq!(retry.reason, ActionReason::RpmReconcile);
    assert_eq!(rpm(&retry), 3000);
}

#[test]
fn automatic_handback_failures_retry_at_a_limited_rate() {
    let mut controller = controller(FanConfig::automatic(0));
    let auto = only(controller.update(
        &snapshot(0.0, Some(55.0), 3000.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&auto, false, 0.0);
    assert!(controller
        .update(
            &snapshot(2.0, Some(55.0), 3000.0, HardwareMode::Forced),
            2.0
        )
        .is_empty());
    assert_eq!(
        only(controller.update(
            &snapshot(15.0, Some(55.0), 3000.0, HardwareMode::Forced),
            15.0
        ))
        .command,
        Command::SetAutomatic
    );
}

#[test]
fn sparse_fan_ids_are_addressed_by_id_and_invalid_ids_are_ignored() {
    let mut controller = Controller::new(Config {
        version: CONFIG_VERSION,
        thermal_policy: ThermalPolicy::default(),
        adaptive_tuning: AdaptiveTuning::default(),
        fans: vec![FanConfig {
            fan_id: 2,
            mode: ControlMode::Manual { rpm: 3000 },
            curve: None,
        }],
    });
    let mut sample = snapshot(0.0, Some(55.0), 1500.0, HardwareMode::Forced);
    sample.fan_count = Some(3.0);
    sample.fans = vec![
        fan(2, 1500.0, HardwareMode::Forced),
        fan(10, 1500.0, HardwareMode::Forced),
    ];
    let action = only(controller.update(&sample, 0.0));
    assert_eq!(action.fan_id, 2);
    assert_eq!(rpm(&action), 3000);
}

#[test]
fn sleep_invalidates_old_requests_hands_back_and_resume_reapplies() {
    let mut controller = controller(manual(3000));
    let old = only(controller.update(
        &snapshot(0.0, Some(55.0), 1500.0, HardwareMode::Forced),
        0.0,
    ));
    let auto = only(controller.prepare_for_sleep(1.0));
    assert_eq!(auto.command, Command::SetAutomatic);
    assert_eq!(auto.reason, ActionReason::Sleep);
    assert!(!controller.acknowledge(&old, true, 1.0));
    assert!(controller.acknowledge(&auto, true, 1.0));
    assert!(controller
        .update(
            &snapshot(2.0, Some(55.0), 1500.0, HardwareMode::Forced),
            2.0
        )
        .is_empty());
    controller.resume();
    let restored = only(controller.update(
        &snapshot(60.0, Some(55.0), 1500.0, HardwareMode::Automatic),
        60.0,
    ));
    assert_eq!(rpm(&restored), 3000);
    assert!(!controller.acknowledge(&old, true, 60.0));
}

#[test]
fn invalid_user_edits_do_not_mutate_a_running_configuration() {
    let mut controller = controller(manual(3000));
    assert!(controller.set_fan_config(manual(0)).is_err());
    assert_eq!(
        controller.config().fans[0].mode,
        ControlMode::Manual { rpm: 3000 }
    );
    assert!(controller
        .replace_config(Config {
            version: 999,
            thermal_policy: ThermalPolicy::default(),
            adaptive_tuning: AdaptiveTuning::default(),
            fans: vec![]
        })
        .is_err());
    assert_eq!(controller.config().version, CONFIG_VERSION);
}

#[test]
fn unsupported_explicit_stop_hands_back_then_recovers_without_off_residence() {
    let mut controller = controller(curve_config());
    let idle = only(controller.update(&snapshot(0.0, Some(35.0), 0.0, HardwareMode::Forced), 0.0));
    assert_eq!(idle.command, Command::SetAutomatic);
    assert_eq!(idle.reason, ActionReason::LowDemandSystem);
    controller.acknowledge(&idle, true, 0.0);
    assert!(controller
        .update(
            &snapshot(2.0, Some(35.0), 0.0, HardwareMode::Automatic),
            2.0
        )
        .is_empty());
    let start = only(controller.update(
        &snapshot(4.0, Some(55.0), 0.0, HardwareMode::Automatic),
        4.0,
    ));
    assert_eq!(rpm(&start), 2240);
    controller.acknowledge(&start, true, 4.0);
    let cooling = only(controller.update(
        &snapshot(6.0, Some(35.0), 2240.0, HardwareMode::Forced),
        6.0,
    ));
    assert_eq!(cooling.command, Command::SetAutomatic);
    assert_eq!(cooling.reason, ActionReason::LowDemandSystem);
    assert!(matches!(
        controller.config().fans[0].mode,
        ControlMode::Curve { .. }
    ));
}

#[test]
fn a_valid_chassis_curve_cannot_mask_long_lost_silicon_safety_telemetry() {
    let mut config = curve_config();
    config.curve.as_mut().unwrap().sensor_key = "NAND".into();
    let mut controller = controller(config);
    let chassis = Sensor {
        key: "NAND".into(),
        name: "NAND".into(),
        group: SensorGroup::System,
        value: Some(60.0),
    };
    let mut sample = snapshot(0.0, Some(55.0), 2980.0, HardwareMode::Forced);
    sample.sensors.push(chassis.clone());
    let first = only(controller.update(&sample, 0.0));
    controller.acknowledge(&first, true, 0.0);
    sample = snapshot(2.0, None, 2980.0, HardwareMode::Forced);
    sample.sensors.push(chassis);
    assert!(controller.update(&sample, 2.0).is_empty());
    sample.sampled_at = 32.0;
    let automatic = only(controller.update(&sample, 32.0));
    assert_eq!(automatic.command, Command::SetAutomatic);
    assert_eq!(automatic.reason, ActionReason::MissingInput);
}

#[test]
fn custom_control_waits_for_initial_safety_telemetry_and_edits_cannot_extend_missing_hold() {
    let mut no_telemetry = controller(manual(3000));
    let initial =
        only(no_telemetry.update(&snapshot(0.0, None, 1500.0, HardwareMode::Automatic), 0.0));
    assert_eq!(initial.command, Command::SetAutomatic);
    assert_eq!(initial.reason, ActionReason::MissingInput);

    let mut controller = controller(manual(3000));
    let initial = only(controller.update(
        &snapshot(0.0, Some(55.0), 3000.0, HardwareMode::Forced),
        0.0,
    ));
    controller.acknowledge(&initial, true, 0.0);
    assert!(controller
        .update(&snapshot(2.0, None, 3000.0, HardwareMode::Forced), 2.0)
        .is_empty());
    controller.set_fan_config(manual(3100)).unwrap();
    let edit = only(controller.update(&snapshot(20.0, None, 3000.0, HardwareMode::Forced), 20.0));
    controller.acknowledge(&edit, true, 20.0);
    controller.set_fan_config(manual(3200)).unwrap();
    let expired =
        only(controller.update(&snapshot(32.0, None, 3100.0, HardwareMode::Forced), 32.0));
    assert_eq!(expired.command, Command::SetAutomatic);
}

#[test]
fn editing_the_same_missing_curve_source_does_not_reset_its_hold_deadline() {
    let mut config = curve_config();
    config.curve.as_mut().unwrap().sensor_key = "CH0P".into();
    let mut controller = controller(config.clone());
    let source = Sensor {
        key: "CH0P".into(),
        name: "Other".into(),
        group: SensorGroup::Other,
        value: Some(60.0),
    };
    let mut initial = snapshot(0.0, Some(55.0), 2980.0, HardwareMode::Forced);
    initial.sensors.push(source);
    let write = only(controller.update(&initial, 0.0));
    controller.acknowledge(&write, true, 0.0);
    assert!(controller
        .update(
            &snapshot(2.0, Some(55.0), 2980.0, HardwareMode::Forced),
            2.0
        )
        .is_empty());
    config.curve.as_mut().unwrap().points[2].speed_percent = 1.0;
    controller.set_fan_config(config).unwrap();
    let auto = only(controller.update(
        &snapshot(32.0, Some(55.0), 2980.0, HardwareMode::Forced),
        32.0,
    ));
    assert_eq!(auto.command, Command::SetAutomatic);
    assert_eq!(auto.reason, ActionReason::MissingInput);
}

#[test]
fn deferred_actions_are_neither_applied_nor_failures_and_are_reissued() {
    let mut controller = controller(manual(3000));
    let action = only(controller.update(
        &snapshot(0.0, Some(55.0), 0.0, HardwareMode::Automatic),
        0.0,
    ));
    assert!(controller.defer(&action));
    assert!(!controller.defer(&action));
    let status = controller.fan_status(0).unwrap();
    assert_eq!(status.applied_rpm, None);
    assert!(status.pending.is_none());
    assert!(status.last_failure.is_none());
    // Not a failure: no hand-back is queued, the same target is simply asked again.
    let again = only(controller.update(
        &snapshot(1.0, Some(55.0), 0.0, HardwareMode::Automatic),
        1.0,
    ));
    assert_eq!(rpm(&again), 3000);
}
