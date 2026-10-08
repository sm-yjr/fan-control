use fan_core::*;

fn sensor(key: &str, group: SensorGroup, value: f64) -> Sensor {
    Sensor {
        key: key.into(),
        name: key.into(),
        group,
        value: Some(value),
    }
}

fn sample() -> Snapshot {
    Snapshot {
        sampled_at: 0.0,
        fan_count: Some(1.0),
        fans: vec![Fan {
            id: 0,
            name: "模拟风扇".into(),
            min_rpm: Some(1000.0),
            max_rpm: Some(4900.0),
            current_rpm: Some(1000.0),
            mode: HardwareMode::Automatic,
        }],
        sensors: vec![
            sensor("cpu", SensorGroup::Cpu, 85.0),
            sensor("body", SensorGroup::System, 45.0),
        ],
        thermal_pressure: ThermalPressure::Nominal,
        cpu_utilization_percent: Some(95.0),
        machine_id: None,
    }
}

fn controller(fan: FanConfig) -> Controller {
    Controller::new(Config {
        fans: vec![fan],
        ..Config::default()
    })
}

fn tick(controller: &mut Controller, sample: &mut Snapshot, now: f64) -> Vec<Action> {
    sample.sampled_at = now;
    let actions = controller.update(sample, now);
    for action in &actions {
        assert!(controller.acknowledge(action, true, now));
        match action.command {
            Command::SetAutomatic => sample.fans[0].mode = HardwareMode::Automatic,
            Command::SetRpm { rpm, .. } => {
                assert!((1000..=4900).contains(&rpm));
                sample.fans[0].mode = HardwareMode::Forced;
                sample.fans[0].current_rpm = Some(rpm as f64);
            }
        }
    }
    actions
}

fn applied(controller: &Controller) -> u32 {
    controller.fan_status(0).unwrap().applied_rpm.unwrap()
}

#[test]
fn fresh_sampling_gaps_keep_established_cooling_but_do_not_confirm_unobserved_load() {
    for gap in [16.0, 31.0, 120.0] {
        let mut controller = controller(FanConfig::adaptive(0));
        let mut sample = sample();
        for now in (0..=600).step_by(2) {
            tick(&mut controller, &mut sample, f64::from(now));
        }
        let previous = applied(&controller);
        let hot = controller.thermal_reading().clone();
        let actions = tick(&mut controller, &mut sample, 600.0 + gap);
        assert!(!actions
            .iter()
            .any(|action| action.command == Command::SetAutomatic));
        assert!(previous.saturating_sub(applied(&controller)) <= 175);
        let next = controller.thermal_reading();
        assert!(next.adaptive.thermal_load_percent >= hot.adaptive.thermal_load_percent * 0.95);
        assert!(next.adaptive.heat_soak_percent >= hot.adaptive.heat_soak_percent * 0.95);
        assert!(!next.adaptive.load_sustained);
        assert_eq!(next.adaptive.silicon_rise_celsius_per_second, 0.0);
        assert!((next.sustained_silicon_temperature - 85.0).abs() < 0.01);
        // 明确恢复会话仍丢弃历史，不能把醒前负载当作醒后确认。
        controller.resume();
        tick(&mut controller, &mut sample, 602.0 + gap);
        assert_eq!(
            controller.thermal_reading().adaptive.thermal_load_percent,
            0.0
        );
        assert!(!controller.thermal_reading().adaptive.load_sustained);
    }
}

#[test]
fn a_fresh_cool_sample_after_a_long_gap_preserves_heat_memory_and_decelerates() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut sample = sample();
    for now in (0..=600).step_by(2) {
        tick(&mut controller, &mut sample, f64::from(now));
    }
    let previous = applied(&controller);
    let previous_load = controller.thermal_reading().adaptive.thermal_load_percent;
    sample.sensors[0].value = Some(45.0);
    sample.sensors[1].value = Some(30.0);
    sample.cpu_utilization_percent = Some(0.0);
    tick(&mut controller, &mut sample, 720.0);
    assert!(controller.thermal_reading().adaptive.thermal_load_percent > previous_load * 0.9);
    assert!(previous.saturating_sub(applied(&controller)) <= 175);
}

#[test]
fn cpu_aggregate_and_hotspot_are_distinct_and_invalid_aggregate_uses_core_fallback() {
    let mut sample = sample();
    sample.sensors = vec![
        sensor("TCMb", SensorGroup::Cpu, 62.0),
        sensor("TCMz", SensorGroup::Cpu, 90.0),
        sensor("core-a", SensorGroup::Cpu, 40.0),
        sensor("core-b", SensorGroup::Cpu, 60.0),
    ];
    assert_eq!(sample.average_temperature(SensorGroup::Cpu), Some(62.0));
    assert_eq!(sample.hottest_silicon(), Some(90.0));
    assert_eq!(
        sample.input_value("Average CPU", &ThermalReading::default()),
        Some(62.0)
    );
    sample.sensors[0].value = Some(f64::NAN);
    assert_eq!(sample.average_temperature(SensorGroup::Cpu), Some(50.0));
    sample.sensors[0].group = SensorGroup::Other;
    sample.sensors[0].value = Some(110.0);
    assert_eq!(sample.average_temperature(SensorGroup::Cpu), Some(50.0));
    assert_eq!(sample.hottest_silicon(), Some(90.0));
}

#[test]
fn a_lone_valid_cpu_hotspot_supports_normal_cooling_without_becoming_an_average() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut sample = sample();
    sample.sensors = vec![sensor("TCMz", SensorGroup::Cpu, 81.8)];
    sample.cpu_utilization_percent = None;
    tick(&mut controller, &mut sample, 0.0);
    assert!(controller.thermal_reading().available);
    assert!(controller.thermal_reading().adaptive.available);
    assert_eq!(sample.average_temperature(SensorGroup::Cpu), None);
    assert_eq!(sample.hottest_silicon(), Some(81.8));
    for now in (2..=300).step_by(2) {
        tick(&mut controller, &mut sample, f64::from(now));
    }
    assert!(controller.thermal_reading().adaptive.demand_percent > 47.0);
    assert!(applied(&controller) >= 2833);
    assert_eq!(
        controller.thermal_reading().sustained_silicon_temperature,
        0.0
    );
    sample.sensors[0].value = Some(97.0);
    let actions = tick(&mut controller, &mut sample, 302.0);
    assert_eq!(actions[0].reason, ActionReason::Emergency);
    assert!(applied(&controller) >= 4120);
}

#[test]
fn losing_the_only_valid_hotspot_holds_briefly_then_returns_and_chassis_alone_cannot_resume() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut sample = sample();
    sample.sensors = vec![
        sensor("TCMz", SensorGroup::Cpu, 81.8),
        sensor("body", SensorGroup::System, 50.0),
    ];
    for now in (0..=300).step_by(2) {
        tick(&mut controller, &mut sample, f64::from(now));
    }
    let previous = applied(&controller);
    sample.sensors[0].value = None;
    for now in (302..=330).step_by(2) {
        assert!(tick(&mut controller, &mut sample, f64::from(now)).is_empty());
        assert!(!controller.thermal_reading().adaptive.available);
        assert_eq!(applied(&controller), previous);
    }
    let actions = tick(&mut controller, &mut sample, 332.0);
    assert_eq!(actions[0].reason, ActionReason::MissingInput);
    assert_eq!(actions[0].command, Command::SetAutomatic);
    for now in (334..=600).step_by(2) {
        assert!(!tick(&mut controller, &mut sample, f64::from(now))
            .iter()
            .any(|action| matches!(action.command, Command::SetRpm { .. })));
    }
}

#[test]
fn sustained_hotspot_cannot_be_diluted_by_a_cool_average_or_cold_chassis() {
    let mut controller = controller(FanConfig::adaptive(0));
    let mut sample = sample();
    sample.cpu_utilization_percent = None;
    sample.sensors = vec![
        sensor("TCMb", SensorGroup::Cpu, 55.0),
        sensor("TCMz", SensorGroup::Cpu, 90.0),
        sensor("core-a", SensorGroup::Cpu, 40.0),
        sensor("body", SensorGroup::System, 30.0),
    ];
    for now in (0..=300).step_by(2) {
        tick(&mut controller, &mut sample, f64::from(now));
    }
    let reading = controller.thermal_reading();
    assert!((reading.sustained_silicon_temperature - 55.0).abs() < 0.01);
    assert!((reading.sustained_hotspot_temperature - 90.0).abs() < 0.01);
    assert!(reading.demand_percent >= 74.0);
    assert!(reading.adaptive.demand_percent >= 74.0);
    assert!(applied(&controller) >= 3886);
    let before = applied(&controller);
    sample.sensors[1].value = Some(95.0);
    for now in (302..=480).step_by(2) {
        tick(&mut controller, &mut sample, f64::from(now));
    }
    assert!(applied(&controller) >= before);
    assert!(!controller.fan_status(0).unwrap().safety_override);
}

#[test]
fn emergency_escalates_immediately_and_all_custom_modes_release_at_35_rpm_per_second() {
    let mut curve = FanConfig::balanced(0);
    curve.curve.as_mut().unwrap().set_sensor_key("cpu");
    for config in [
        FanConfig::adaptive(0),
        FanConfig {
            fan_id: 0,
            mode: ControlMode::Manual { rpm: 1500 },
            curve: None,
        },
        curve,
    ] {
        let mut controller = controller(config);
        let mut sample = sample();
        sample.sensors[0].value = Some(45.0);
        sample.sensors[1].value = Some(30.0);
        sample.cpu_utilization_percent = None;
        sample.thermal_pressure = ThermalPressure::Critical;
        let actions = tick(&mut controller, &mut sample, 0.0);
        assert_eq!(actions[0].reason, ActionReason::Emergency);
        assert_eq!(applied(&controller), 4900);
        sample.thermal_pressure = ThermalPressure::Serious;
        tick(&mut controller, &mut sample, 2.0);
        assert_eq!(applied(&controller), 4830);
        assert!(applied(&controller) >= 3730);
        sample.thermal_pressure = ThermalPressure::Nominal;
        tick(&mut controller, &mut sample, 4.0);
        assert_eq!(applied(&controller), 4760);
        sample.thermal_pressure = ThermalPressure::Critical;
        tick(&mut controller, &mut sample, 6.0);
        assert_eq!(applied(&controller), 4900);
    }
}

#[test]
fn a_curve_stop_after_emergency_waits_until_cooling_has_ramped_to_the_hardware_minimum() {
    let mut config = FanConfig::balanced(0);
    config.curve.as_mut().unwrap().set_sensor_key("cpu");
    let mut controller = controller(config);
    let mut sample = sample();
    sample.sensors[0].value = Some(30.0);
    sample.thermal_pressure = ThermalPressure::Critical;
    tick(&mut controller, &mut sample, 0.0);
    assert_eq!(applied(&controller), 4900);
    sample.thermal_pressure = ThermalPressure::Nominal;
    for now in (2..=110).step_by(2) {
        let before = applied(&controller);
        let actions = tick(&mut controller, &mut sample, f64::from(now));
        assert!(!actions
            .iter()
            .any(|action| action.command == Command::SetAutomatic));
        assert!(before.saturating_sub(applied(&controller)) <= 70);
    }
    tick(&mut controller, &mut sample, 112.0);
    assert_eq!(applied(&controller), 1000);
    let returned = tick(&mut controller, &mut sample, 114.0);
    assert!(returned
        .iter()
        .any(|action| action.command == Command::SetAutomatic));
}

#[test]
fn normal_manual_reductions_remain_immediate_without_a_safety_override() {
    let manual = |rpm| FanConfig {
        fan_id: 0,
        mode: ControlMode::Manual { rpm },
        curve: None,
    };
    let mut controller = controller(manual(4000));
    let mut sample = sample();
    tick(&mut controller, &mut sample, 0.0);
    controller.set_fan_config(manual(1500)).unwrap();
    tick(&mut controller, &mut sample, 2.0);
    assert_eq!(applied(&controller), 1500);
}
