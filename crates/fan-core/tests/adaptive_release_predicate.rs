use fan_core::{AdaptiveReading, ADAPTIVE_RELEASE_DEMAND_PERCENT};

#[test]
fn release_prerequisite_requires_both_demands_and_quiet_inputs() {
    let mut reading = AdaptiveReading {
        demand_percent: ADAPTIVE_RELEASE_DEMAND_PERCENT,
        ..AdaptiveReading::default()
    };
    assert!(reading.low_demand_for_release(5.));
    assert!(!reading.low_demand_for_release(5.0001));
    reading.demand_percent = 5.0001;
    assert!(!reading.low_demand_for_release(4.));
    reading.demand_percent = 5.;
    reading.load_sustained = true;
    assert!(!reading.low_demand_for_release(5.));
    reading.load_sustained = false;
    reading.silicon_rise_celsius_per_second = 0.15;
    assert!(reading.low_demand_for_release(5.));
    reading.silicon_rise_celsius_per_second = 0.150001;
    assert!(!reading.low_demand_for_release(5.));
    reading.silicon_rise_celsius_per_second = 0.;
    reading.comfort_demand_percent = 0.0001;
    assert!(!reading.low_demand_for_release(5.));
    reading.comfort_demand_percent = 0.;
    for invalid in [f64::NAN, f64::INFINITY] {
        assert!(!reading.low_demand_for_release(invalid));
    }
}
