use fan_core::*;

#[test]
fn mapping_is_bounded_ordered_monotonic_and_exactly_neutral() {
    for hot in [45.0, 80.0, 85.0, 90.0, 96.0] {
        for position in -10..=10 {
            let bias = AdaptiveTuning { bias: position }.normalized();
            let mut previous = 0.0;
            for tenth in 0..=1000 {
                let demand = f64::from(tenth) / 10.0;
                let value = tuned_adaptive_demand(demand, bias, hot);
                assert!((0.0..=100.0).contains(&value));
                assert!(value >= previous);
                previous = value;
                assert_eq!(tuned_adaptive_demand(demand, 0.0, hot), demand);
                if hot >= 90.0 && bias < 0.0 {
                    assert_eq!(value, demand);
                }
                let quiet = tuned_adaptive_demand(demand, -1.0, hot);
                let cool = tuned_adaptive_demand(demand, 1.0, hot);
                assert!(quiet <= value && value <= cool);
                assert!(
                    (value - demand).abs()
                        <= if bias > 0.0 {
                            20.0 + 1e-10
                        } else {
                            8.0 + 1e-10
                        }
                );
            }
        }
    }
    for demand in 0..=100 {
        for bias in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let values: Vec<_> = (750..=950)
                .map(|hot| tuned_adaptive_demand(f64::from(demand), bias, f64::from(hot) / 10.0))
                .collect();
            assert!(values.windows(2).all(|w| w[0] <= w[1]));
        }
    }
}

#[test]
fn bias_changes_gradually_and_gaps_do_not_skip_the_transition() {
    let target = AdaptiveTuning { bias: 10 };
    let short = smooth_adaptive_bias(-1.0, target, 2.0);
    assert!(short > -1.0 && short < 0.0);
    assert_eq!(
        smooth_adaptive_bias(-1.0, target, 60.0),
        smooth_adaptive_bias(-1.0, target, 5.0)
    );
    assert_eq!(smooth_adaptive_bias(-1.0, target, f64::NAN), -1.0);
    let mut current = -1.0;
    for _ in 0..60 {
        current = smooth_adaptive_bias(current, target, 2.0);
    }
    assert_eq!(current, 1.0);
    for _ in 0..60 {
        current = smooth_adaptive_bias(current, AdaptiveTuning::default(), 2.0);
    }
    assert_eq!(current, 0.0);
}

#[test]
fn old_configs_default_to_zero_and_invalid_preference_preserves_other_settings() {
    for json in [
        r#"{"version":1,"fans":[]}"#,
        r#"{"version":2,"fans":[]}"#,
        r#"[{"fanId":0,"mode":{"manual":{"rpm":2000}}}]"#,
    ] {
        let load = Config::from_json(json).unwrap();
        assert_eq!(load.config.adaptive_tuning.bias, 0);
    }
    let mut config = Config {
        fans: vec![FanConfig::balanced(0)],
        ..Config::default()
    };
    config.adaptive_tuning.bias = -10;
    let original = config.to_json().unwrap();
    assert_eq!(Config::from_json(&original).unwrap().config, config);
    let mut invalid: serde_json::Value = serde_json::from_str(&original).unwrap();
    invalid["adaptive_tuning"]["bias"] = 11.into();
    let recovered = Config::from_json(&invalid.to_string()).unwrap();
    assert!(recovered.migrated && !recovered.warnings.is_empty());
    assert_eq!(recovered.config.adaptive_tuning.bias, 0);
    assert_eq!(recovered.config.fans, config.fans);
    assert_eq!(recovered.config.thermal_policy, config.thermal_policy);
    config.adaptive_tuning.bias = -11;
    assert!(config.to_json().is_err());
}

fn config_with_preserved_controls() -> Config {
    let curve = Curve::balanced("custom-curve-id", "custom-sensor");
    Config {
        fans: vec![
            FanConfig {
                fan_id: 0,
                mode: ControlMode::Curve {
                    curve_id: curve.id.clone(),
                },
                curve: Some(curve),
            },
            FanConfig {
                fan_id: 1,
                mode: ControlMode::Manual { rpm: 2200 },
                curve: None,
            },
        ],
        thermal_policy: ThermalPolicy {
            comfort_target_celsius: Some(35.0),
            calibration: None,
        },
        ..Config::default()
    }
}

#[test]
fn missing_and_all_valid_preferences_preserve_other_settings() {
    let mut config = config_with_preserved_controls();
    let mut value = serde_json::to_value(&config).unwrap();
    value.as_object_mut().unwrap().remove("adaptive_tuning");
    for preference in [None, Some(serde_json::json!({}))] {
        let mut candidate = value.clone();
        if let Some(preference) = preference {
            candidate["adaptive_tuning"] = preference;
        }
        let load = Config::from_json(&candidate.to_string()).unwrap();
        assert_eq!(load.config, config);
        assert!(!load.migrated);
        assert!(load.warnings.is_empty());
    }
    for bias in -10..=10 {
        config.adaptive_tuning.bias = bias;
        let load = Config::from_json(&config.to_json().unwrap()).unwrap();
        assert_eq!(load.config, config);
        assert!(!load.migrated);
        assert!(load.warnings.is_empty());
    }
}

#[test]
fn damaged_preference_only_resets_tuning_and_preserves_valid_controls() {
    let config = config_with_preserved_controls();
    for preference in [
        serde_json::json!({"bias": -11}),
        serde_json::json!({"bias": 11}),
        serde_json::json!({"bias": -129}),
        serde_json::json!({"bias": 128}),
        serde_json::json!({"bias": u64::MAX}),
        serde_json::json!({"bias": null}),
        serde_json::json!({"bias": "10"}),
        serde_json::json!({"bias": 1.5}),
        serde_json::json!({"bias": true}),
        serde_json::json!({"bias": []}),
        serde_json::json!({"bias": {}}),
        serde_json::json!(null),
        serde_json::json!(10),
        serde_json::json!("invalid"),
        serde_json::json!(true),
        serde_json::json!([]),
        serde_json::json!([0]),
        serde_json::json!([10]),
    ] {
        let mut candidate = serde_json::to_value(&config).unwrap();
        candidate["adaptive_tuning"] = preference.clone();
        let load = Config::from_json(&candidate.to_string()).unwrap();
        assert_eq!(load.config, config, "preference: {preference}");
        assert!(load.migrated, "preference: {preference}");
        assert_eq!(load.warnings.len(), 1, "preference: {preference}");
        assert!(load.warnings[0].starts_with("散热偏好无效，已恢复默认："));
    }
}

#[test]
fn preference_recovery_does_not_relax_other_configuration_validation() {
    let mut candidate = serde_json::to_value(config_with_preserved_controls()).unwrap();
    candidate["adaptive_tuning"] = serde_json::json!({"bias": 128});
    let mut unsupported = candidate.clone();
    unsupported["version"] = serde_json::json!(CONFIG_VERSION + 1);
    assert!(matches!(
        Config::from_json(&unsupported.to_string()),
        Err(ConfigError::UnsupportedVersion(_))
    ));
    let mut wrong_fans = candidate.clone();
    wrong_fans["fans"] = serde_json::json!("invalid");
    assert!(matches!(
        Config::from_json(&wrong_fans.to_string()),
        Err(ConfigError::Json(_))
    ));
    let mut wrong_policy = candidate.clone();
    wrong_policy["thermal_policy"] = serde_json::json!("invalid");
    assert!(matches!(
        Config::from_json(&wrong_policy.to_string()),
        Err(ConfigError::Json(_))
    ));
    candidate["fans"][1]["fan_id"] = serde_json::json!(0);
    assert!(matches!(
        Config::from_json(&candidate.to_string()),
        Err(ConfigError::Invalid(_))
    ));
}

#[test]
fn cooler_positions_add_absolute_points_at_idle_and_saturate() {
    for position in 0..=10 {
        let bias = AdaptiveTuning { bias: position }.normalized();
        for demand in [0.0_f64, 5.0, 20.0, 50.0, 90.0, 100.0] {
            for hottest in [45.0, 80.0, 85.0, 90.0, 96.0] {
                let expected = (demand + 2.0 * f64::from(position)).min(100.0);
                assert!((tuned_adaptive_demand(demand, bias, hottest) - expected).abs() < 1e-10);
            }
        }
    }
}

#[test]
fn quieter_mapping_matches_the_previous_envelope_at_every_position() {
    for position in -10..=0 {
        for demand in [0.0_f64, 5.0, 20.0, 50.0, 90.0, 100.0] {
            for hottest in [45.0_f64, 80.0, 85.0, 90.0, 96.0] {
                let x = demand / 100.0;
                let hot = ((hottest - 80.0) / 10.0).clamp(0.0, 1.0);
                let gate = 1.0 - hot * hot * (3.0 - 2.0 * hot);
                let bias = f64::from(position) / 10.0;
                let previous = (demand + bias * 32.0 * x * (1.0 - x) * gate).clamp(0.0, 100.0);
                assert!((tuned_adaptive_demand(demand, bias, hottest) - previous).abs() < 1e-10);
            }
        }
    }
}

#[test]
fn cooler_transition_keeps_ten_second_smoothing_and_no_gap_shortcut() {
    let target = AdaptiveTuning { bias: 10 };
    let mut value = 0.0;
    for _ in 0..5 {
        value = smooth_adaptive_bias(value, target, 2.0);
    }
    assert!((value - (1.0 - (-1.0_f64).exp())).abs() < 1e-10);
    assert!((tuned_adaptive_demand(50.0, value, 45.0) - (50.0 + value * 20.0)).abs() < 1e-10);
    assert_eq!(
        smooth_adaptive_bias(0.0, target, 60.0),
        smooth_adaptive_bias(0.0, target, 5.0)
    );
}
