use fan_core::*;

const MINIMUM_RPM: u32 = 1500;
const MAXIMUM_RPM: u32 = 6000;
const SAMPLE_SECONDS: f64 = 2.0;

// 逻辑时钟与立即成功的硬件回读仅用于策略模拟，不访问 SMC 或 helper。
struct Simulation {
    controller: Controller,
    snapshot: Snapshot,
    now: f64,
}

impl Simulation {
    fn new() -> Self {
        Self::with_config(Config {
            fans: vec![FanConfig::adaptive(0)],
            ..Config::default()
        })
    }

    fn with_config(config: Config) -> Self {
        Self {
            controller: Controller::new(config),
            now: 0.0,
            snapshot: Snapshot {
                sampled_at: 0.0,
                fan_count: Some(1.0),
                thermal_pressure: ThermalPressure::Nominal,
                cpu_utilization_percent: Some(0.0),
                machine_id: Some("simulated-device".into()),
                fans: vec![Fan {
                    id: 0,
                    name: "Simulated fan".into(),
                    min_rpm: Some(MINIMUM_RPM as f64),
                    max_rpm: Some(MAXIMUM_RPM as f64),
                    current_rpm: Some(0.0),
                    mode: HardwareMode::Automatic,
                }],
                sensors: vec![
                    Sensor {
                        key: "cpu".into(),
                        name: "Simulated CPU".into(),
                        group: SensorGroup::Cpu,
                        value: Some(45.0),
                    },
                    Sensor {
                        key: "gpu".into(),
                        name: "Simulated GPU".into(),
                        group: SensorGroup::Gpu,
                        value: None,
                    },
                    Sensor {
                        key: "body".into(),
                        name: "Simulated chassis proxy".into(),
                        group: SensorGroup::System,
                        value: Some(30.0),
                    },
                ],
            },
        }
    }

    fn temperature(&mut self, key: &str, value: Option<f64>) {
        self.snapshot
            .sensors
            .iter_mut()
            .find(|sensor| sensor.key == key)
            .unwrap()
            .value = value;
    }

    fn poll(&mut self, now: f64) -> Vec<Action> {
        self.now = now;
        self.snapshot.sampled_at = now;
        self.controller.update(&self.snapshot, now)
    }

    fn accept(&mut self, actions: &[Action]) {
        for action in actions {
            assert!(self.controller.acknowledge(action, true, self.now));
            match action.command {
                Command::SetAutomatic => {
                    self.snapshot.fans[0].mode = HardwareMode::Automatic;
                    // 本虚构系统会在低负载自动模式停转，用于发现反复接管。
                    self.snapshot.fans[0].current_rpm = Some(0.0);
                }
                Command::SetRpm { rpm, .. } => {
                    assert!((MINIMUM_RPM..=MAXIMUM_RPM).contains(&rpm));
                    self.snapshot.fans[0].mode = HardwareMode::Forced;
                    self.snapshot.fans[0].current_rpm = Some(rpm as f64);
                }
            }
        }
    }

    fn tick(&mut self, now: f64) -> Vec<Action> {
        let actions = self.poll(now);
        self.accept(&actions);
        actions
    }

    fn step(&mut self) -> Vec<Action> {
        self.tick(self.now + SAMPLE_SECONDS)
    }

    fn advance(&mut self, seconds: u32) -> Vec<Action> {
        assert_eq!(seconds % 2, 0);
        let mut actions = Vec::new();
        for _ in 0..seconds / 2 {
            actions.extend(self.step());
        }
        actions
    }

    fn applied_rpm(&self) -> Option<u32> {
        self.controller.fan_status(0).unwrap().applied_rpm
    }

    fn reading(&self) -> &AdaptiveReading {
        &self.controller.thermal_reading().adaptive
    }
}

fn requested_rpm(actions: &[Action]) -> Option<u32> {
    actions.iter().find_map(|action| match action.command {
        Command::SetRpm { rpm, .. } => Some(rpm),
        Command::SetAutomatic => None,
    })
}

fn returns_to_system(actions: &[Action]) -> bool {
    actions
        .iter()
        .any(|action| action.command == Command::SetAutomatic)
}

fn assert_normal_slew(previous: u32, current: u32, seconds: f64) {
    let elapsed = seconds.min(5.0);
    if current > previous {
        assert!(
            current - previous <= (100.0 * elapsed) as u32,
            "ordinary increase {previous} -> {current} exceeded {elapsed}s bound"
        );
    } else {
        assert!(
            previous - current <= (35.0 * elapsed) as u32,
            "ordinary decrease {previous} -> {current} exceeded {elapsed}s bound"
        );
    }
}

fn warm_under_load(simulation: &mut Simulation, seconds: u32) {
    simulation.temperature("cpu", Some(85.0));
    simulation.temperature("body", Some(45.0));
    simulation.snapshot.cpu_utilization_percent = Some(95.0);
    simulation.tick(0.0);
    simulation.advance(seconds);
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Forced);
    assert!(simulation.applied_rpm().unwrap() > MINIMUM_RPM + 1500);
}

#[test]
fn a_two_second_chip_spike_from_idle_does_not_take_ownership() {
    let mut simulation = Simulation::new();
    simulation.tick(0.0);
    simulation.advance(30);
    simulation.temperature("cpu", Some(90.0));
    assert!(requested_rpm(&simulation.step()).is_none());
    simulation.temperature("cpu", Some(45.0));
    let cooldown = simulation.advance(180);
    assert!(requested_rpm(&cooldown).is_none());
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Automatic);
    assert!(simulation.reading().demand_percent < 12.0);
}

#[test]
fn first_hot_silicon_reading_does_not_immediately_seed_chassis_heat() {
    let mut simulation = Simulation::new();
    simulation.temperature("cpu", Some(90.0));
    let actions = simulation.tick(0.0);
    assert!(requested_rpm(&actions).is_none());
    assert_eq!(simulation.reading().heat_soak_percent, 0.0);
    assert_eq!(simulation.reading().thermal_load_percent, 0.0);
}

#[test]
fn one_hot_startup_sample_does_not_later_become_sustained_cooling() {
    let mut simulation = Simulation::new();
    simulation.temperature("cpu", Some(90.0));
    assert!(requested_rpm(&simulation.tick(0.0)).is_none());
    simulation.temperature("cpu", Some(45.0));
    for _ in 0..30 {
        let actions = simulation.step();
        assert!(
            requested_rpm(&actions).is_none(),
            "one startup spike caused delayed ownership at {}s: {actions:?}",
            simulation.now
        );
    }
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Automatic);
}

#[test]
fn a_spike_after_just_one_cool_startup_reading_does_not_take_ownership() {
    let mut simulation = Simulation::new();
    simulation.tick(0.0);
    simulation.temperature("cpu", Some(90.0));
    assert!(requested_rpm(&simulation.step()).is_none());
    simulation.temperature("cpu", Some(45.0));
    assert!(requested_rpm(&simulation.advance(60)).is_none());
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Automatic);
}

#[test]
fn a_genuinely_hot_startup_still_intervenes_without_utilization_metrics() {
    let mut simulation = Simulation::new();
    simulation.snapshot.cpu_utilization_percent = None;
    simulation.temperature("cpu", Some(90.0));
    simulation.tick(0.0);
    let actions = simulation.advance(60);
    assert!(requested_rpm(&actions).is_some());
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Forced);
    assert!(simulation.applied_rpm().unwrap() > MINIMUM_RPM);
    assert!(simulation.reading().demand_percent > 12.0);
}

#[test]
fn cpu_utilization_jitter_around_confirmation_threshold_stays_quiet() {
    let mut simulation = Simulation::new();
    simulation.tick(0.0);
    for sample in 0..180 {
        simulation.snapshot.cpu_utilization_percent =
            Some(if sample % 2 == 0 { 66.0 } else { 64.0 });
        assert!(requested_rpm(&simulation.step()).is_none());
        assert!(!simulation.reading().load_sustained);
    }
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Automatic);
}

#[test]
fn sustained_work_starts_cooling_while_silicon_and_chassis_are_still_cool() {
    let mut simulation = Simulation::new();
    simulation.snapshot.cpu_utilization_percent = Some(95.0);
    assert!(requested_rpm(&simulation.tick(0.0)).is_none());
    let mut first_start = None;
    for _ in 0..30 {
        let actions = simulation.step();
        if let Some(rpm) = requested_rpm(&actions) {
            first_start.get_or_insert((simulation.now, rpm));
            assert_eq!(actions[0].reason, ActionReason::AdaptivePerformance);
        }
    }
    let (time, rpm) = first_start.expect("sustained work should start cooling");
    assert!(time > 4.0 && time < 60.0);
    assert_eq!(rpm, MINIMUM_RPM);
    assert!(simulation.reading().thermal_load_percent > 30.0);
    assert_eq!(
        simulation.snapshot.thermal_pressure,
        ThermalPressure::Nominal
    );
    assert_eq!(simulation.snapshot.sensors[0].value, Some(45.0));
}

#[test]
fn long_work_then_chip_cooling_keeps_residence_and_reduces_rpm_gradually() {
    let mut simulation = Simulation::new();
    warm_under_load(&mut simulation, 600);
    let hot_demand = simulation.reading().demand_percent;
    simulation.temperature("cpu", Some(45.0));
    simulation.snapshot.cpu_utilization_percent = Some(0.0);
    for sample in 0..45 {
        let previous = simulation.applied_rpm().unwrap();
        let actions = simulation.step();
        assert!(!returns_to_system(&actions));
        assert_normal_slew(previous, simulation.applied_rpm().unwrap(), SAMPLE_SECONDS);
        if sample == 0 {
            assert!(simulation.reading().demand_percent > hot_demand * 0.8);
        }
    }
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Forced);
}

#[test]
fn repeated_multiminute_work_and_idle_bursts_keep_one_control_session() {
    let mut simulation = Simulation::new();
    simulation.tick(0.0);
    let mut owned = false;
    for _ in 0..8 {
        for (load, samples) in [(95.0, 30), (0.0, 45)] {
            simulation.snapshot.cpu_utilization_percent = Some(load);
            for _ in 0..samples {
                let actions = simulation.step();
                if owned {
                    assert!(!returns_to_system(&actions));
                    assert!(simulation.applied_rpm().is_some());
                }
                owned |= requested_rpm(&actions).is_some();
            }
        }
    }
    assert!(owned);
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Forced);
}

#[test]
fn a_hot_chassis_keeps_cooling_when_chip_load_and_temperature_drop() {
    let mut simulation = Simulation::new();
    warm_under_load(&mut simulation, 300);
    simulation.temperature("cpu", Some(45.0));
    simulation.temperature("body", Some(50.0));
    simulation.snapshot.cpu_utilization_percent = None;
    for _ in 0..180 {
        let previous = simulation.applied_rpm().unwrap();
        let actions = simulation.step();
        assert!(!returns_to_system(&actions));
        assert_normal_slew(previous, simulation.applied_rpm().unwrap(), SAMPLE_SECONDS);
    }
    assert!(simulation.controller.thermal_reading().uses_chassis_sensor);
    assert!(simulation.reading().demand_percent > 12.0);
    assert!(simulation.applied_rpm().unwrap() > MINIMUM_RPM + 1000);
    assert_eq!(simulation.reading().cpu_utilization_percent, None);
}

#[test]
fn sustained_gpu_rise_intervenes_without_cpu_utilization_data() {
    let mut simulation = Simulation::new();
    simulation.snapshot.cpu_utilization_percent = None;
    simulation.temperature("gpu", Some(45.0));
    simulation.tick(0.0);
    let mut trend_started_cooling = false;
    for sample in 1..=20 {
        simulation.temperature("gpu", Some(45.0 + sample as f64 * 1.5));
        let actions = simulation.step();
        trend_started_cooling |= actions.iter().any(|action| {
            matches!(action.command, Command::SetRpm { .. })
                && action.reason == ActionReason::AdaptiveTrend
        });
        assert!(!simulation.reading().load_sustained);
    }
    assert!(trend_started_cooling);
    assert!(simulation.reading().silicon_rise_celsius_per_second > 0.15);
    assert!(simulation.applied_rpm().unwrap() > MINIMUM_RPM);
    assert_eq!(
        simulation.snapshot.thermal_pressure,
        ThermalPressure::Nominal
    );
}

#[test]
fn temperature_only_cooling_eventually_returns_after_continuous_quiet() {
    let mut simulation = Simulation::new();
    simulation.snapshot.cpu_utilization_percent = None;
    simulation.temperature("cpu", Some(85.0));
    simulation.temperature("body", Some(45.0));
    simulation.tick(0.0);
    simulation.advance(600);
    assert!(simulation.applied_rpm().unwrap() > MINIMUM_RPM + 1500);
    simulation.temperature("cpu", Some(45.0));
    simulation.temperature("body", Some(30.0));
    let mut last_meaningful_demand = simulation.now;
    let mut low_demand_since = None;
    let mut returned = false;
    for _ in 0..900 {
        let previous = simulation.applied_rpm().unwrap();
        let actions = simulation.step();
        let reading = simulation.reading();
        if reading.demand_percent >= 12.0 {
            last_meaningful_demand = simulation.now;
        }
        if reading.demand_percent <= 5.0 && reading.silicon_rise_celsius_per_second <= 0.15 {
            low_demand_since.get_or_insert(simulation.now);
        } else {
            low_demand_since = None;
        }
        if returns_to_system(&actions) {
            assert_eq!(actions[0].reason, ActionReason::LowDemandSystem);
            assert!(simulation.now - last_meaningful_demand >= 180.0);
            assert!(simulation.now - low_demand_since.unwrap() >= 60.0);
            assert!(previous <= MINIMUM_RPM + 75);
            returned = true;
            break;
        }
        assert_normal_slew(previous, simulation.applied_rpm().unwrap(), SAMPLE_SECONDS);
    }
    assert!(returned, "cooling must eventually return to the system");
    assert!(simulation.applied_rpm().is_none());
    let idle = simulation.advance(600);
    assert!(requested_rpm(&idle).is_none());
    assert!(!returns_to_system(&idle));
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Automatic);
}

#[test]
fn automatic_reconciliation_preserves_live_system_rpm_then_resumes_normal_slew() {
    let mut simulation = Simulation::new();
    simulation.snapshot.cpu_utilization_percent = Some(95.0);
    simulation.tick(0.0);
    simulation.advance(120);
    assert!(simulation.applied_rpm().unwrap() < 5000);
    simulation.snapshot.fans[0].mode = HardwareMode::Automatic;
    simulation.snapshot.fans[0].current_rpm = Some(5000.0);
    let reconcile = simulation.step();
    assert!(requested_rpm(&reconcile).unwrap() >= 5000);
    let previous = simulation.applied_rpm().unwrap();
    simulation.step();
    assert_normal_slew(previous, simulation.applied_rpm().unwrap(), SAMPLE_SECONDS);
}

fn calibrated_config() -> Config {
    Config {
        fans: vec![FanConfig::adaptive(0)],
        thermal_policy: ThermalPolicy {
            comfort_target_celsius: Some(38.0),
            calibration: Some(
                SurfaceCalibration::from_measurements(
                    "simulated-device",
                    "body",
                    36.0,
                    32.0,
                    46.0,
                    42.0,
                )
                .unwrap(),
            ),
        },
        ..Config::default()
    }
}

#[test]
fn a_new_normal_goal_after_long_stability_uses_the_current_tick_for_slew() {
    let mut simulation = Simulation::with_config(calibrated_config());
    simulation.temperature("body", Some(44.0));
    simulation.tick(0.0);
    simulation.advance(300);
    assert!(simulation.applied_rpm().is_some());
    assert!(requested_rpm(&simulation.advance(90)).is_none());
    let previous = simulation.applied_rpm().unwrap();
    simulation.temperature("body", Some(45.0));
    let increase = simulation.step();
    assert!(requested_rpm(&increase).unwrap() > previous);
    assert!(
        simulation
            .controller
            .fan_status(0)
            .unwrap()
            .desired_rpm
            .unwrap()
            > previous + 500
    );
    assert_normal_slew(previous, simulation.applied_rpm().unwrap(), SAMPLE_SECONDS);
    assert_eq!(simulation.reading().comfort_status, ComfortStatus::Active);
    let previous = simulation.applied_rpm().unwrap();
    simulation.tick(simulation.now + 10.0);
    assert_normal_slew(previous, simulation.applied_rpm().unwrap(), 10.0);
}

#[test]
fn missing_live_silicon_cannot_authorize_cooling_from_cpu_load_or_hot_chassis() {
    let mut simulation = Simulation::new();
    simulation.temperature("cpu", None);
    simulation.temperature("body", Some(50.0));
    simulation.snapshot.cpu_utilization_percent = Some(95.0);
    assert!(requested_rpm(&simulation.tick(0.0)).is_none());
    assert!(requested_rpm(&simulation.advance(120)).is_none());
    assert!(!simulation.reading().available);
    assert!(simulation.applied_rpm().is_none());
}

#[test]
fn losing_silicon_while_cooling_holds_briefly_then_safely_returns() {
    let mut simulation = Simulation::new();
    warm_under_load(&mut simulation, 300);
    let previous = simulation.applied_rpm().unwrap();
    simulation.temperature("cpu", None);
    let first_missing = simulation.now + SAMPLE_SECONDS;
    for _ in 0..15 {
        let actions = simulation.step();
        assert!(actions.is_empty());
        assert_eq!(simulation.applied_rpm(), Some(previous));
    }
    let actions = simulation.step();
    assert!(returns_to_system(&actions));
    assert_eq!(actions[0].reason, ActionReason::MissingInput);
    assert!(simulation.now - first_missing >= 30.0);
    assert!(simulation.applied_rpm().is_none());
}

#[test]
fn stale_snapshot_returns_even_when_heat_and_cpu_load_were_high() {
    let mut simulation = Simulation::new();
    warm_under_load(&mut simulation, 300);
    simulation.now += 16.0;
    let actions = simulation
        .controller
        .update(&simulation.snapshot, simulation.now);
    assert!(returns_to_system(&actions));
    assert_eq!(actions[0].reason, ActionReason::StaleSnapshot);
    assert!(!simulation.reading().available);
    simulation.accept(&actions);
    assert!(simulation.applied_rpm().is_none());
}

#[test]
fn failed_first_rpm_is_not_applied_and_hand_back_precedes_retry() {
    let mut simulation = Simulation::new();
    simulation.snapshot.cpu_utilization_percent = Some(95.0);
    simulation.tick(0.0);
    let failed = loop {
        let actions = simulation.poll(simulation.now + SAMPLE_SECONDS);
        if let Some(action) = actions
            .iter()
            .find(|action| matches!(action.command, Command::SetRpm { .. }))
        {
            break action.clone();
        }
        assert!(simulation.now < 60.0);
        simulation.accept(&actions);
    };
    assert!(simulation
        .controller
        .acknowledge(&failed, false, simulation.now));
    assert!(simulation.applied_rpm().is_none());
    assert!(simulation
        .controller
        .fan_status(0)
        .unwrap()
        .last_failure
        .is_some());
    let recovery = simulation.step();
    assert_eq!(recovery.len(), 1);
    assert_eq!(recovery[0].command, Command::SetAutomatic);
    assert_eq!(recovery[0].reason, ActionReason::WriteFailure);
    assert!(simulation.applied_rpm().is_none());
}

#[test]
fn emergency_overrides_start_confirmation_and_normal_ramp() {
    for (pressure, temperature, expected_floor) in [
        (ThermalPressure::Serious, Some(45.0), 4650),
        (ThermalPressure::Critical, Some(45.0), MAXIMUM_RPM),
        (ThermalPressure::Nominal, Some(97.0), 5100),
        (ThermalPressure::Critical, None, MAXIMUM_RPM),
    ] {
        let mut simulation = Simulation::new();
        simulation.tick(0.0);
        simulation.snapshot.thermal_pressure = pressure;
        simulation.temperature("cpu", temperature);
        let actions = simulation.step();
        assert!(requested_rpm(&actions).unwrap() >= expected_floor);
        assert_eq!(actions[0].reason, ActionReason::Emergency);
        assert!(simulation.controller.fan_status(0).unwrap().safety_override);
    }
}

#[test]
fn hot_calibration_boundary_keeps_its_cooling_floor_without_ramp_delay() {
    let mut simulation = Simulation::with_config(calibrated_config());
    simulation.temperature("body", Some(46.1));
    let actions = simulation.tick(0.0);
    let first = requested_rpm(&actions).unwrap();
    assert!(first > MINIMUM_RPM + 2000);
    assert_eq!(
        Some(first),
        simulation.controller.fan_status(0).unwrap().desired_rpm
    );
    assert_eq!(simulation.reading().estimated_surface_celsius, None);
    assert_eq!(
        simulation.reading().comfort_status,
        ComfortStatus::AboveCalibrationRange
    );
    let boundary_demand = simulation.reading().comfort_demand_percent;
    for proxy in [47.0, 48.0, 50.0] {
        simulation.temperature("body", Some(proxy));
        simulation.step();
        assert!(simulation.applied_rpm().unwrap() >= first);
        assert!(simulation.reading().comfort_demand_percent >= boundary_demand);
    }
}

#[test]
fn tuning_orders_everyday_targets_without_calibration_and_keeps_live_bounds() {
    let mut rpms = Vec::new();
    for bias in [-10, 0, 10] {
        let mut simulation = Simulation::with_config(Config {
            fans: vec![FanConfig::adaptive(0)],
            adaptive_tuning: AdaptiveTuning { bias },
            ..Config::default()
        });
        simulation.temperature("cpu", Some(55.0));
        simulation.temperature("body", Some(35.0));
        simulation.snapshot.cpu_utilization_percent = Some(90.0);
        simulation.advance(600);
        rpms.push(simulation.applied_rpm().unwrap());
        assert_eq!(
            simulation.reading().comfort_status,
            ComfortStatus::Uncalibrated
        );
        assert!((MINIMUM_RPM..=MAXIMUM_RPM).contains(&rpms[rpms.len() - 1]));
    }
    assert!(rpms[0] < rpms[1] && rpms[1] < rpms[2]);
}

#[test]
fn tuning_updates_keep_pending_ack_heat_history_and_cooling_residence() {
    let mut simulation = Simulation::new();
    warm_under_load(&mut simulation, 600);
    let now = simulation.now + 2.0;
    let actions = simulation.poll(now);
    let before = simulation.reading().clone();
    let previous = simulation.applied_rpm().unwrap();
    let mut config = simulation.controller.config().clone();
    config.adaptive_tuning.bias = -10;
    simulation.controller.replace_config(config).unwrap();
    assert_eq!(simulation.reading(), &before);
    simulation.accept(&actions);
    simulation.tick(now);
    assert_eq!(simulation.reading(), &before);
    simulation.temperature("cpu", Some(45.0));
    simulation.temperature("body", Some(30.0));
    simulation.snapshot.cpu_utilization_percent = Some(0.0);
    let first = simulation.step();
    assert!(!returns_to_system(&first));
    assert!(simulation.reading().heat_soak_percent > 0.0);
    assert_normal_slew(previous, simulation.applied_rpm().unwrap(), 2.0);
    let all = simulation.advance(1800);
    assert!(returns_to_system(&all));
}

#[test]
fn all_preferences_preserve_emergency_and_neutral_or_quieter_spike_behavior() {
    for bias in -10..=10 {
        let config = Config {
            fans: vec![FanConfig::adaptive(0)],
            adaptive_tuning: AdaptiveTuning { bias },
            ..Config::default()
        };
        // Positive offsets intentionally make short warming more likely to cross the takeover threshold.
        // Neutral/quieter retain their previous spike behavior; all biases retain protection.
        if bias <= 0 {
            let mut simulation = Simulation::with_config(config.clone());
            simulation.tick(0.0);
            simulation.temperature("cpu", Some(85.0));
            simulation.snapshot.cpu_utilization_percent = Some(95.0);
            assert!(!simulation
                .step()
                .iter()
                .any(|a| matches!(a.command, Command::SetRpm { .. })));
            simulation.temperature("cpu", Some(45.0));
            simulation.snapshot.cpu_utilization_percent = Some(0.0);
            assert!(!simulation
                .advance(60)
                .iter()
                .any(|a| matches!(a.command, Command::SetRpm { .. })));
        }
        for (hot, pressure, percent) in [
            (96.0, ThermalPressure::Nominal, 80.0),
            (45.0, ThermalPressure::Serious, 70.0),
            (45.0, ThermalPressure::Critical, 100.0),
        ] {
            let mut urgent = Simulation::with_config(config.clone());
            urgent.temperature("cpu", Some(hot));
            urgent.snapshot.thermal_pressure = pressure;
            let actions = urgent.tick(0.0);
            let rpm = requested_rpm(&actions).unwrap();
            assert!(
                rpm >= (MINIMUM_RPM as f64 + percent / 100.0 * (MAXIMUM_RPM - MINIMUM_RPM) as f64)
                    .ceil() as u32
            );
            assert_eq!(actions[0].reason, ActionReason::Emergency);
        }
    }
}

#[test]
fn legacy_zero_preference_matches_explicit_zero_action_sequences() {
    let mut old = serde_json::to_value(Config {
        fans: vec![FanConfig::adaptive(0)],
        ..Config::default()
    })
    .unwrap();
    old.as_object_mut().unwrap().remove("adaptive_tuning");
    let mut a = Simulation::with_config(Config::from_json(&old.to_string()).unwrap().config);
    let mut b = Simulation::new();
    for second in (0..=1800).step_by(2) {
        let hot = (100..400).contains(&second) || (600..900).contains(&second);
        for sim in [&mut a, &mut b] {
            sim.temperature("cpu", Some(if hot { 75.0 } else { 45.0 }));
            sim.temperature("body", Some(if hot { 45.0 } else { 30.0 }));
            sim.snapshot.cpu_utilization_percent = Some(if hot { 90.0 } else { 0.0 });
        }
        assert_eq!(a.tick(f64::from(second)), b.tick(f64::from(second)));
    }
}

#[test]
fn preferences_do_not_change_manual_or_curve_actions_and_keep_stale_fallback() {
    for fan in [
        FanConfig {
            fan_id: 0,
            mode: ControlMode::Manual { rpm: 2500 },
            curve: None,
        },
        FanConfig::balanced(0),
    ] {
        let mut baseline = Simulation::with_config(Config {
            fans: vec![fan.clone()],
            ..Config::default()
        });
        let mut tuned = Simulation::with_config(Config {
            fans: vec![fan],
            adaptive_tuning: AdaptiveTuning { bias: 10 },
            ..Config::default()
        });
        for second in (0..=100).step_by(2) {
            assert_eq!(
                baseline.tick(f64::from(second)),
                tuned.tick(f64::from(second))
            );
        }
    }
    for bias in [-10, 0, 10] {
        let mut simulation = Simulation::with_config(Config {
            fans: vec![FanConfig::adaptive(0)],
            adaptive_tuning: AdaptiveTuning { bias },
            ..Config::default()
        });
        warm_under_load(&mut simulation, 300);
        let actions = simulation.controller.update(
            &simulation.snapshot,
            simulation.now + SNAPSHOT_MAXIMUM_AGE + 1.0,
        );
        assert!(returns_to_system(&actions));
        assert_eq!(actions[0].reason, ActionReason::StaleSnapshot);
    }
}

#[test]
fn positive_idle_offsets_obey_existing_start_and_release_hysteresis_without_churn() {
    for bias in 1..=10 {
        let config = Config {
            fans: vec![FanConfig::adaptive(0)],
            adaptive_tuning: AdaptiveTuning { bias },
            ..Config::default()
        };
        let mut cold = Simulation::with_config(config.clone());
        let first = cold.tick(0.0);
        let later = cold.advance(600);
        assert!(
            (cold.reading().adjusted_demand_percent.unwrap() - 2.0 * f64::from(bias)).abs() < 1e-10
        );
        if bias >= 6 {
            assert!(requested_rpm(&first).is_some());
            assert!(!returns_to_system(&later));
            assert_eq!(cold.snapshot.fans[0].mode, HardwareMode::Forced);
        } else {
            assert!(requested_rpm(&first).is_none());
            assert!(requested_rpm(&later).is_none());
            assert_eq!(cold.snapshot.fans[0].mode, HardwareMode::Automatic);
        }
        let mut warmed = Simulation::with_config(config);
        warm_under_load(&mut warmed, 600);
        warmed.temperature("cpu", Some(45.0));
        warmed.temperature("body", Some(30.0));
        warmed.snapshot.cpu_utilization_percent = Some(0.0);
        let cooled = warmed.advance(1800);
        if bias <= 2 {
            assert!(returns_to_system(&cooled));
            assert_eq!(warmed.snapshot.fans[0].mode, HardwareMode::Automatic);
            assert!(requested_rpm(&warmed.advance(600)).is_none());
        } else {
            assert!(!returns_to_system(&cooled));
            assert_eq!(warmed.snapshot.fans[0].mode, HardwareMode::Forced);
        }
    }
}

#[test]
fn neutral_after_strong_cooler_offset_can_complete_confirmed_handback() {
    let mut simulation = Simulation::with_config(Config {
        fans: vec![FanConfig::adaptive(0)],
        adaptive_tuning: AdaptiveTuning { bias: 10 },
        ..Config::default()
    });
    simulation.tick(0.0);
    simulation.advance(120);
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Forced);
    let mut config = simulation.controller.config().clone();
    config.adaptive_tuning.bias = 0;
    simulation.controller.replace_config(config).unwrap();
    let cooled = simulation.advance(1200);
    assert!(returns_to_system(&cooled));
    assert_eq!(simulation.snapshot.fans[0].mode, HardwareMode::Automatic);
    assert!(requested_rpm(&simulation.advance(600)).is_none());
}

#[test]
fn positive_offsets_map_independent_dual_fan_ranges_and_return_without_churn() {
    for bias in 1..=10 {
        let mut sim = Simulation::with_config(Config {
            fans: vec![FanConfig::adaptive(0), FanConfig::adaptive(1)],
            adaptive_tuning: AdaptiveTuning { bias },
            ..Config::default()
        });
        sim.snapshot.fan_count = Some(2.);
        sim.snapshot.fans.push(Fan {
            id: 1,
            name: "Second simulated fan".into(),
            min_rpm: Some(800.),
            max_rpm: Some(4500.),
            current_rpm: Some(0.),
            mode: HardwareMode::Automatic,
        });
        let mut previous = [None, None];
        let mut automatic = [0, 0];
        for second in (0..=2400).step_by(2) {
            if second == 600 {
                let mut cfg = sim.controller.config().clone();
                cfg.adaptive_tuning.bias = 0;
                sim.controller.replace_config(cfg).unwrap();
            }
            let actions = sim.poll(second as f64);
            for action in &actions {
                let i = action.fan_id as usize;
                let (lo, hi) = if i == 0 { (1500, 6000) } else { (800, 4500) };
                assert!(sim.controller.acknowledge(action, true, sim.now));
                let fan = &mut sim.snapshot.fans[i];
                match action.command {
                    Command::SetRpm { rpm, .. } => {
                        assert!((lo..=hi).contains(&rpm));
                        if let Some((last, at)) = previous[i] {
                            assert_normal_slew(last, rpm, second as f64 - at);
                        }
                        previous[i] = Some((rpm, second as f64));
                        fan.mode = HardwareMode::Forced;
                        fan.current_rpm = Some(rpm as f64);
                    }
                    Command::SetAutomatic => {
                        if fan.mode == HardwareMode::Forced {
                            automatic[i] += 1;
                        }
                        fan.mode = HardwareMode::Automatic;
                        fan.current_rpm = Some(0.);
                        previous[i] = None;
                    }
                }
            }
            if second == 598 {
                assert_eq!(
                    sim.reading().adjusted_demand_percent,
                    Some(bias as f64 * 2.)
                );
                for (i, fan) in sim.snapshot.fans.iter().enumerate() {
                    assert_eq!(fan.mode == HardwareMode::Forced, bias >= 6);
                    if bias >= 6 {
                        let (lo, hi) = if i == 0 {
                            (1500., 6000.)
                        } else {
                            (800., 4500.)
                        };
                        let expected = (lo + bias as f64 * 0.02 * (hi - lo)).ceil() as u32;
                        assert_eq!(
                            sim.controller.fan_status(fan.id).unwrap().desired_rpm,
                            Some(expected)
                        );
                    }
                }
            }
            if second > 1800 {
                assert!(actions
                    .iter()
                    .all(|a| !matches!(a.command, Command::SetRpm { .. })));
            }
        }
        assert!(sim
            .snapshot
            .fans
            .iter()
            .all(|f| f.mode == HardwareMode::Automatic));
        assert!(automatic
            .iter()
            .all(|&n| n == if bias >= 6 { 1 } else { 0 }));
    }
}

#[test]
fn positive_offset_includes_calibrated_comfort_demand_before_saturation() {
    for bias in 1..=10 {
        for body in [40., 42., 44., 46., 47.] {
            let mut cfg = calibrated_config();
            cfg.adaptive_tuning.bias = bias;
            let mut sim = Simulation::with_config(cfg);
            sim.temperature("body", Some(body));
            sim.advance(600);
            let r = sim.reading();
            assert_eq!(
                r.adjusted_demand_percent,
                Some((r.demand_percent + 2. * bias as f64).min(100.)),
                "bias {bias}, body {body}"
            );
            assert!(r.adjusted_demand_percent.unwrap() >= r.comfort_demand_percent);
        }
    }
}
