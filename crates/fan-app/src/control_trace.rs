//! Bounded, local control history: measurements and actions, without device identity/configuration.
use fan_core::{Action, Command, Controller, HardwareMode, Snapshot, SNAPSHOT_MAXIMUM_AGE};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

const CAPACITY: usize = 256;
const SAMPLE_INTERVAL: f64 = 30.0;

#[derive(Default)]
pub struct ControlTrace {
    events: VecDeque<Value>,
    last_sample: Option<f64>,
    last_recorded: Option<f64>,
    previous: Option<Value>,
}

impl ControlTrace {
    pub fn events(&self) -> &VecDeque<Value> {
        &self.events
    }

    pub fn record(&mut self, event: &str, data: Value) {
        let entry = json!({
            "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH)
                .unwrap_or_default().as_millis(),
            "event": event,
            "data": data,
        });
        #[cfg(not(test))]
        crate::telemetry::event(&entry.to_string());
        if self.events.len() == CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(entry);
    }

    pub fn sample(&mut self, snapshot: &Snapshot, controller: &Controller, now: f64, ready: bool) {
        let state = json!({
            "helper_ready": ready,
            "fresh": snapshot.fresh(now, SNAPSHOT_MAXIMUM_AGE),
            "thermal_pressure": snapshot.thermal_pressure,
            "fans": snapshot.fans.iter().map(|fan| json!({
                "id": fan.id,
                "mode": fan.mode,
                "stopped": fan.current_rpm.map(|rpm| rpm <= 50.0),
                "reason": controller.fan_status(fan.id).and_then(|status| status.status_reason),
            })).collect::<Vec<_>>(),
        });
        let gap = self.last_sample.map(|last| snapshot.sampled_at - last);
        self.last_sample = Some(snapshot.sampled_at);
        let changed = self.previous.as_ref() != Some(&state);
        self.previous = Some(state);
        if !changed
            && !gap.is_some_and(|gap| gap > SNAPSHOT_MAXIMUM_AGE)
            && self
                .last_recorded
                .is_some_and(|last| now - last < SAMPLE_INTERVAL)
        {
            return;
        }
        self.last_recorded = Some(now);
        self.record("control.sample", json!({
            "sample_age_secs": (now - snapshot.sampled_at).max(0.0),
            "sample_gap_secs": gap,
            "helper_ready": ready,
            "thermal_pressure": snapshot.thermal_pressure,
            "demand_percent": controller.thermal_reading().adaptive.demand_percent,
            "adjusted_demand_percent": controller.thermal_reading().adaptive.adjusted_demand_percent,
            "fans": snapshot.fans.iter().map(|fan| {
                let status = controller.fan_status(fan.id);
                json!({
                    "id": fan.id,
                    "mode": fan.mode,
                    "measured_rpm": fan.current_rpm,
                    "desired_rpm": status.and_then(|s| s.desired_rpm),
                    "confirmed_target_rpm": status.and_then(|s| s.applied_rpm),
                    "reason": status.and_then(|s| s.status_reason),
                })
            }).collect::<Vec<_>>(),
        }));
    }

    pub fn action(
        &mut self,
        action: &Action,
        snapshot: &Snapshot,
        controller: &Controller,
        attempted: bool,
        confirmed: bool,
    ) {
        let fan = snapshot.fans.iter().find(|fan| fan.id == action.fan_id);
        let prior = controller
            .fan_status(action.fan_id)
            .and_then(|s| s.applied_rpm);
        // Keep transitions/failures immediately, without logging every routine ramp step.
        if confirmed
            && matches!(action.command, Command::SetRpm { .. })
            && prior.is_some()
            && fan.is_some_and(|f| f.mode == HardwareMode::Forced)
        {
            return;
        }
        self.record(
            "control.action",
            json!({
                "action": action,
                "attempted": attempted,
                "confirmed": confirmed,
                "measured_rpm": fan.and_then(|f| f.current_rpm),
                "observed_mode": fan.map(|f| f.mode),
                "previous_target_rpm": prior,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fan_core::{Config, Fan, ThermalPressure};

    #[test]
    fn bounded_history_has_wall_clock_and_keeps_newest_events() {
        let mut trace = ControlTrace::default();
        for n in 0..CAPACITY + 5 {
            trace.record("test", json!({"n": n}));
        }
        assert_eq!(trace.events().len(), CAPACITY);
        assert_eq!(trace.events()[0]["data"]["n"], 5);
        assert!(trace.events()[0]["unix_ms"].as_u64().unwrap() > 0);
    }

    #[test]
    fn mode_stop_and_sampling_gaps_are_recorded_before_periodic_deadline() {
        let mut trace = ControlTrace::default();
        let controller = Controller::new(Config::default());
        let mut snapshot = Snapshot {
            sampled_at: 0.,
            fan_count: Some(1.),
            thermal_pressure: ThermalPressure::Nominal,
            cpu_utilization_percent: None,
            machine_id: Some("must-not-export".into()),
            sensors: vec![],
            fans: vec![Fan {
                id: 0,
                name: "must-not-export".into(),
                min_rpm: Some(1500.),
                max_rpm: Some(5000.),
                current_rpm: Some(2500.),
                mode: HardwareMode::Forced,
            }],
        };
        trace.sample(&snapshot, &controller, 0., true);
        snapshot.sampled_at = 2.;
        trace.sample(&snapshot, &controller, 2., true);
        assert_eq!(trace.events().len(), 1);
        snapshot.fans[0].mode = HardwareMode::Automatic;
        snapshot.sampled_at = 4.;
        trace.sample(&snapshot, &controller, 4., true);
        snapshot.fans[0].current_rpm = Some(0.);
        snapshot.sampled_at = 6.;
        trace.sample(&snapshot, &controller, 6., true);
        snapshot.sampled_at = 22.;
        trace.sample(&snapshot, &controller, 22., true);
        assert_eq!(trace.events().len(), 4);
        assert_eq!(trace.events()[3]["data"]["sample_gap_secs"], 16.);
        assert!(!serde_json::to_string(trace.events())
            .unwrap()
            .contains("must-not-export"));
    }
}
