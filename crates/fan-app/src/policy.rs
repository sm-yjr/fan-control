//! User-facing thermal goals and two-point, device-bound surface calibration.
use crate::{
    ui::{raw_text, rect, text, tokens},
    worker::UiSnapshot,
};
use fan_core::{ComfortStatus, SensorGroup, Snapshot, SurfaceCalibration, ThermalPolicy};
use objc2::{rc::Retained, runtime::AnyObject, sel, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;

const RING: f64 = 96.;

pub struct PolicyEditor {
    pub view: Retained<NSStackView>,
    ring: Retained<crate::gauge::Ring>,
    demand: Retained<NSTextField>,
    level: Retained<NSTextField>,
    reason: Retained<NSTextField>,
    status: Retained<NSTextField>,
    usage: Retained<NSTextField>,
    use_smart: Retained<NSButton>,
    comfort: crate::form::Row,
    pub notice: Retained<NSTextField>,
    enabled: Retained<NSSwitch>,
    target: Retained<NSTextField>,
    source: Retained<NSPopUpButton>,
    sources: Vec<String>,
    measured: [Retained<NSTextField>; 2],
    record_buttons: [Retained<NSButton>; 2],
    captured: [Retained<NSTextField>; 2],
    records: RefCell<[Option<(f64, f64)>; 2]>,
    base: RefCell<ThermalPolicy>,
    machine_id: Option<String>,
    save: Retained<NSButton>,
}
fn button(
    mtm: MainThreadMarker,
    target: &AnyObject,
    title: &str,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(&text(title), Some(target), Some(action), mtm)
    }
}
fn field(mtm: MainThreadMarker, value: &str, width: f64) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&raw_text(value), mtm);
    field
        .widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
    field
}
/// Sensor names are enough for people; the SMC key only disambiguates repeats.
fn sensor_title(sensors: &[&fan_core::Sensor], key: &str) -> String {
    match sensors.iter().find(|s| s.key == key) {
        Some(sensor) if sensors.iter().filter(|s| s.name == sensor.name).count() > 1 => {
            format!("{} ({})", crate::i18n::translate(&sensor.name), key)
        }
        Some(sensor) => crate::i18n::translate(&sensor.name),
        None => crate::i18n::translate(&format!("{} · 暂不可用", key)),
    }
}
impl PolicyEditor {
    pub fn new(mtm: MainThreadMarker, delegate: &AnyObject, state: &UiSnapshot) -> Self {
        use crate::form::{caption, group, row, section_title, spacer, CONTENT};
        use crate::popover::{stack, styled_label, tinted_box};
        let view = crate::form::page(mtm);

        // Now: how much cooling is needed and why, in one sentence.
        let ring = crate::gauge::Ring::new(mtm, RING, 9.);
        let demand = styled_label(mtm, "--", 22., true, false, None);
        demand.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            22.,
            unsafe { NSFontWeightMedium },
        )));
        let center = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 0.);
        center.setAlignment(NSLayoutAttribute::CenterX);
        center.addArrangedSubview(&demand);
        let caption_label = styled_label(
            mtm,
            "散热需求",
            tokens::CAPTION,
            false,
            true,
            Some(RING - 30.),
        );
        caption_label.setAlignment(NSTextAlignment::Center);
        center.addArrangedSubview(&caption_label);
        center.setTranslatesAutoresizingMaskIntoConstraints(false);
        ring.addSubview(&center);
        center
            .centerXAnchor()
            .constraintEqualToAnchor(&ring.centerXAnchor())
            .setActive(true);
        center
            .centerYAnchor()
            .constraintEqualToAnchor(&ring.centerYAnchor())
            .setActive(true);
        let words_width = CONTENT - RING - 28. - 20.;
        let words = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 4.);
        let level = styled_label(mtm, "", 15., true, false, Some(words_width));
        let reason = styled_label(mtm, "", tokens::BODY, false, false, Some(words_width));
        let usage = styled_label(mtm, "", tokens::CAPTION, false, true, Some(words_width));
        let status_row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
        let status = styled_label(mtm, "", tokens::CAPTION, true, false, None);
        let use_smart = button(mtm, delegate, "改用智能散热", sel!(useSmart:));
        use_smart.setControlSize(NSControlSize::Small);
        status_row.addArrangedSubview(&status);
        status_row.addArrangedSubview(&use_smart);
        words.addArrangedSubview(&level);
        words.addArrangedSubview(&reason);
        words.addArrangedSubview(&usage);
        words.setCustomSpacing_afterView(10., &usage);
        words.addArrangedSubview(&status_row);
        let hero = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 20.);
        hero.addArrangedSubview(&ring);
        hero.addArrangedSubview(&words);
        hero.addArrangedSubview(&spacer(mtm));
        let hero_box = tinted_box(mtm, &hero, objc2_foundation::NSSize::new(14., 14.));
        hero_box.set_fill(&NSColor::quaternarySystemFillColor());
        hero_box
            .widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        view.addArrangedSubview(&hero_box);
        let how = caption(
            mtm,
            "智能散热综合持续负载、机身蓄热和温升趋势平稳调节风扇；短时温度波动不急升急降，持续冷却后交还系统。",
            CONTENT,
        );
        view.addArrangedSubview(&how);
        view.setCustomSpacing_afterView(crate::form::SECTION_GAP, &how);

        // Optional comfort target, only active after a two-point calibration.
        view.addArrangedSubview(&section_title(mtm, "键盘体感目标（可选）"));
        let enabled = NSSwitch::new(mtm);
        unsafe {
            enabled.setTarget(Some(delegate));
            enabled.setAction(Some(sel!(comfortToggle:)));
        }
        enabled.setState(
            if state.config.thermal_policy.comfort_target_celsius.is_some() {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            },
        );
        enabled.setAccessibilityLabel(Some(&text("启用体感温度目标")));
        let target = field(
            mtm,
            &crate::presenter::format_value(
                state
                    .config
                    .thermal_policy
                    .comfort_target_celsius
                    .unwrap_or(38.),
            ),
            48.,
        );
        target.setAccessibilityLabel(Some(&text("键盘体感温度目标，摄氏度")));
        target.setToolTip(Some(&text("30–45°C")));
        let unit = styled_label(mtm, "°C", tokens::BODY, false, false, None);
        let comfort = row(mtm, "让键盘区域不超过", "", &[&target, &unit, &enabled]);

        let source = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(0., 0., 220., 26.),
            false,
        );
        source
            .widthAnchor()
            .constraintEqualToConstant(220.)
            .setActive(true);
        let mut sensors: Vec<_> = state
            .snapshot
            .sensors
            .iter()
            .filter(|sensor| sensor.group == SensorGroup::System)
            .collect();
        sensors.sort_by_key(|sensor| sensor.key.as_str());
        let mut sources = Vec::new();
        for sensor in &sensors {
            sources.push(sensor.key.clone());
            source.addItemWithTitle(&raw_text(&sensor_title(&sensors, &sensor.key)));
        }
        if let Some(calibration) = state.config.thermal_policy.calibration.as_ref() {
            if !sources.contains(&calibration.sensor_key) {
                sources.push(calibration.sensor_key.clone());
                source
                    .addItemWithTitle(&raw_text(&sensor_title(&sensors, &calibration.sensor_key)));
            }
            if let Some(index) = sources
                .iter()
                .position(|key| *key == calibration.sensor_key)
            {
                source.selectItemAtIndex(index as isize);
            }
        }
        source.setEnabled(!sources.is_empty());
        source.setAccessibilityLabel(Some(&text("校准用内部参考传感器")));
        unsafe {
            source.setTarget(Some(delegate));
            source.setAction(Some(sel!(calibrationSource:)));
        }
        let source_row = row(
            mtm,
            "参考传感器",
            "选一个靠近键盘的机身传感器。",
            &[&source],
        );
        let mut measured = Vec::new();
        let mut captured = Vec::new();
        let mut record_buttons = Vec::new();
        let mut point_rows = Vec::new();
        for i in 0..2 {
            let input = field(mtm, "", 56.);
            input.setPlaceholderString(Some(&raw_text("°C")));
            input.setAccessibilityLabel(Some(&text(&format!("校准点 {} 键盘表面实测温度", i + 1))));
            let record = button(mtm, delegate, "记录", sel!(recordCalibration:));
            record.setTag(i as isize);
            record.setEnabled(state.snapshot.machine_id.is_some() && !sources.is_empty());
            let point = row(
                mtm,
                &format!("校准点 {}", i + 1),
                "未记录",
                &[
                    &styled_label(mtm, "温度计读数", tokens::CAPTION, false, true, None),
                    &input,
                    &record,
                ],
            );
            captured.push(point.detail.clone());
            measured.push(input);
            record_buttons.push(record);
            point_rows.push(point);
        }
        view.addArrangedSubview(&group(
            mtm,
            &[
                &comfort.view,
                &source_row.view,
                &point_rows[0].view,
                &point_rows[1].view,
            ],
        ));
        view.addArrangedSubview(&caption(
            mtm,
            "电脑内部没有键盘表面的温度计，所以需要校准：在两个不同的使用状态下，用外部温度计测量键盘中央并记录，两次内部读数至少相差 3°C。校准只对本机和测量过的温度范围有效；固定速度与温度曲线不受这个目标影响。",
            CONTENT,
        ));
        let notice = styled_label(
            mtm,
            if state.config.thermal_policy.calibration.is_some() {
                "已保存的校准会保留；新校准需完成两个记录点后保存。"
            } else {
                "尚未校准，当前仅启用性能与热趋势策略。"
            },
            tokens::BODY,
            false,
            false,
            Some(CONTENT),
        );
        view.addArrangedSubview(&notice);

        let footer = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
        let clear = crate::popover::link_button(mtm, "清除校准", delegate, sel!(clearCalibration:));
        footer.addArrangedSubview(&clear);
        footer.addArrangedSubview(&spacer(mtm));
        let cancel = button(mtm, delegate, "还原", sel!(cancelPolicy:));
        cancel.setToolTip(Some(&text("放弃未保存的修改")));
        let save = button(mtm, delegate, "保存", sel!(savePolicy:));
        save.setKeyEquivalent(&raw_text("\r"));
        footer.addArrangedSubview(&cancel);
        footer.addArrangedSubview(&save);
        footer
            .widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        view.setCustomSpacing_afterView(crate::form::SECTION_GAP, &notice);
        view.addArrangedSubview(&footer);
        let mut editor = Self {
            view,
            ring,
            demand,
            level,
            reason,
            status,
            usage,
            use_smart,
            comfort,
            notice,
            enabled,
            target,
            source,
            sources,
            measured: measured.try_into().unwrap(),
            record_buttons: record_buttons.try_into().unwrap(),
            captured: captured.try_into().unwrap(),
            records: RefCell::new([None, None]),
            base: RefCell::new(state.config.thermal_policy.clone()),
            machine_id: state.snapshot.machine_id.clone(),
            save,
        };
        editor.refresh(state);
        editor
    }
    pub fn refresh(&mut self, state: &UiSnapshot) {
        let reading = &state.thermal.adaptive;
        let age = state.sample_age_secs + state.published_at.elapsed().as_secs_f64();
        let fresh = age <= fan_core::SNAPSHOT_MAXIMUM_AGE;
        if self.machine_id != state.snapshot.machine_id {
            self.machine_id.clone_from(&state.snapshot.machine_id);
            self.source_changed();
            self.notice
                .setStringValue(&text("本机校准身份已更新，请重新记录两个校准点。"));
        }
        let selected_key = self
            .sources
            .get(self.source.indexOfSelectedItem().max(0) as usize)
            .cloned();
        let mut sensors: Vec<_> = state
            .snapshot
            .sensors
            .iter()
            .filter(|s| s.group == SensorGroup::System)
            .collect();
        sensors.sort_by_key(|s| s.key.as_str());
        let mut available: Vec<_> = sensors.iter().map(|s| s.key.clone()).collect();
        let preserved = selected_key.or_else(|| {
            self.base
                .borrow()
                .calibration
                .as_ref()
                .map(|c| c.sensor_key.clone())
        });
        if let Some(key) = preserved.as_ref().filter(|key| !available.contains(key)) {
            available.push(key.clone());
        }
        if self.sources != available {
            self.source.removeAllItems();
            for key in &available {
                self.source
                    .addItemWithTitle(&raw_text(&sensor_title(&sensors, key)));
            }
            if let Some(index) = preserved
                .as_ref()
                .and_then(|key| available.iter().position(|k| k == key))
            {
                self.source.selectItemAtIndex(index as isize);
            }
            self.sources = available;
            self.source.setEnabled(!self.sources.is_empty());
        }
        let reference_ready = self
            .sources
            .get(self.source.indexOfSelectedItem().max(0) as usize)
            .is_some_and(|key| {
                state
                    .snapshot
                    .sensors
                    .iter()
                    .any(|s| &s.key == key && s.value.is_some_and(fan_core::valid_temperature))
            });
        for button in &self.record_buttons {
            button.setEnabled(fresh && self.machine_id.is_some() && reference_ready);
        }
        let smart = crate::presenter::smart_status(state, fresh);
        let tint = match smart.demand {
            Some(value) if value >= 70. => NSColor::systemOrangeColor(),
            Some(value) if value >= 35. => NSColor::systemYellowColor(),
            _ => NSColor::systemGreenColor(),
        };
        self.ring.set(smart.demand.map(|value| value / 100.), &tint);
        self.demand.setStringValue(&raw_text(
            &smart
                .demand
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| "--".into()),
        ));
        self.level.setStringValue(&raw_text(&format!(
            "{}{}",
            crate::i18n::translate("当前散热需求："),
            crate::i18n::translate(smart.level)
        )));
        self.reason.setStringValue(&text(smart.reason));
        let mut usage = Vec::new();
        if let Some(cpu) = reading.cpu_utilization_percent.filter(|_| fresh) {
            usage.push(format!(
                "{} {cpu:.0}%",
                crate::i18n::translate("处理器使用率")
            ));
        }
        if let Some(chip) = reading.predicted_silicon_celsius.filter(|_| fresh) {
            usage.push(format!(
                "{} {chip:.0}°C",
                crate::i18n::translate("芯片温度趋向")
            ));
        }
        self.usage.setStringValue(&raw_text(&usage.join(" · ")));
        self.status.setStringValue(&text(if smart.in_use {
            "智能散热正在使用"
        } else {
            "当前没有使用智能散热。"
        }));
        let status_color = if smart.in_use {
            NSColor::systemGreenColor()
        } else {
            NSColor::secondaryLabelColor()
        };
        self.status.setTextColor(Some(&status_color));
        self.use_smart.setHidden(smart.in_use);
        self.use_smart
            .setEnabled(state.helper_ready && !state.installing && fresh);
        let comfort = match (
            self.enabled.state() == NSControlStateValueOn,
            reading.estimated_surface_celsius.filter(|_| fresh),
        ) {
            (false, _) => "关闭时只按负载和芯片温度散热。".to_string(),
            (true, Some(surface)) => format!(
                "{} {surface:.0}°C · {}",
                crate::i18n::translate("估计当前"),
                crate::i18n::translate(comfort_label(reading.comfort_status))
            ),
            (true, None) => crate::i18n::translate(comfort_label(reading.comfort_status)),
        };
        crate::form::set_detail(&self.comfort, &comfort);
        self.target
            .setEnabled(self.enabled.state() == NSControlStateValueOn);
        self.save.setEnabled(!state.installing);
    }
    pub fn record(&self, index: usize, state: &UiSnapshot) -> Result<(), String> {
        if index >= 2 {
            return Err("校准点无效。".into());
        }
        let age = state.sample_age_secs + state.published_at.elapsed().as_secs_f64();
        if age > fan_core::SNAPSHOT_MAXIMUM_AGE || !age.is_finite() {
            return Err("采样已过期，请等待新数据后记录。".into());
        }
        if self.machine_id.is_none() || self.machine_id != state.snapshot.machine_id {
            return Err("无法确认本机校准身份，请重新打开设置。".into());
        }
        let source = self
            .sources
            .get(self.source.indexOfSelectedItem().max(0) as usize)
            .ok_or("请选择可用的内部参考传感器。")?;
        let proxy = state
            .snapshot
            .sensors
            .iter()
            .find(|sensor| sensor.key == *source)
            .and_then(|sensor| sensor.value)
            .filter(|value| fan_core::valid_temperature(*value))
            .ok_or("参考传感器暂不可用，未记录校准点。")?;
        let surface = self.measured[index]
            .stringValue()
            .to_string()
            .trim()
            .parse::<f64>()
            .map_err(|_| "请输入键盘表面实测温度。")?;
        if !surface.is_finite() || !(15.0..=60.0).contains(&surface) {
            return Err("表面实测温度必须为 15–60°C。".into());
        }
        self.records.borrow_mut()[index] = Some((proxy, surface));
        self.captured[index].setStringValue(&raw_text(&format!(
            "{} {proxy:.1}°C · {} {surface:.1}°C",
            crate::i18n::translate("内部读数"),
            crate::i18n::translate("实测")
        )));
        self.notice.setStringValue(&text(&format!(
            "已记录点 {}。两个记录点完成后，点击保存。",
            index + 1
        )));
        Ok(())
    }
    pub fn source_changed(&self) {
        self.records.replace([None, None]);
        for field in &self.captured {
            field.setStringValue(&text("未记录"));
        }
        self.notice
            .setStringValue(&text("参考传感器已改变，请重新记录两个校准点。"));
    }
    pub fn clear(&self) {
        self.base.borrow_mut().calibration = None;
        self.source_changed();
        self.notice
            .setStringValue(&text("校准已从草稿清除，点击保存后生效。"));
    }
    pub fn read(&self, snapshot: &Snapshot) -> Result<ThermalPolicy, String> {
        let mut policy = self.base.borrow().clone();
        policy.comfort_target_celsius = if self.enabled.state() == NSControlStateValueOn {
            Some(
                self.target
                    .stringValue()
                    .to_string()
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| "舒适目标必须是数字。")?,
            )
        } else {
            None
        };
        let source = self
            .sources
            .get(self.source.indexOfSelectedItem().max(0) as usize);
        match *self.records.borrow() {
            [Some((proxy_a, surface_a)), Some((proxy_b, surface_b))] => {
                verify_recorded_measurements(
                    [surface_a, surface_b],
                    [
                        self.measured[0].stringValue().to_string(),
                        self.measured[1].stringValue().to_string(),
                    ],
                )?;
                let machine = snapshot
                    .machine_id
                    .clone()
                    .filter(|value| Some(value) == self.machine_id.as_ref())
                    .ok_or("本机身份发生变化，请重新记录校准。")?;
                policy.calibration = Some(
                    SurfaceCalibration::from_measurements(
                        machine,
                        source.ok_or("请选择参考传感器。")?.clone(),
                        proxy_a,
                        surface_a,
                        proxy_b,
                        surface_b,
                    )
                    .map_err(|e| e.to_string())?,
                );
            }
            [None, None] => {
                if let Some(calibration) = policy.calibration.as_ref() {
                    if source != Some(&calibration.sensor_key) {
                        return Err("更换参考传感器后，需要两个新的校准记录点。".into());
                    }
                }
            }
            _ => return Err("校准需要两个完整记录点。".into()),
        }
        policy.validate().map_err(|e| e.to_string())?;
        Ok(policy)
    }
}
fn verify_recorded_measurements(recorded: [f64; 2], displayed: [String; 2]) -> Result<(), String> {
    for (recorded, displayed) in recorded.into_iter().zip(displayed) {
        if displayed.trim().parse::<f64>().ok() != Some(recorded) {
            return Err("实测值已修改，请重新记录对应校准点后保存。".into());
        }
    }
    Ok(())
}
pub fn comfort_label(status: ComfortStatus) -> &'static str {
    match status {
        ComfortStatus::NotRequested => "体感目标未启用",
        ComfortStatus::Uncalibrated => "未校准，仅启用性能策略",
        ComfortStatus::MachineMismatch => "校准不属于本机，体感调节已暂停",
        ComfortStatus::MissingSensor => "校准传感器不可用，体感调节已暂停",
        ComfortStatus::OutOfRange => "超出校准范围，体感调节已暂停",
        ComfortStatus::AboveCalibrationRange => "高于校准范围，保守保持散热",
        ComfortStatus::BelowCalibrationRange => "低于校准范围，表面估计已暂停",
        ComfortStatus::Active => "校准范围内的温度估计",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edited_measurement_cannot_save_a_previous_calibration_point() {
        assert!(verify_recorded_measurements([32.0, 38.0], ["32".into(), "39".into()]).is_err());
        assert!(verify_recorded_measurements([32.0, 38.0], ["32".into(), "NaN".into()]).is_err());
        assert!(verify_recorded_measurements([32.0, 38.0], ["32".into(), "".into()]).is_err());
    }
    #[test]
    fn equivalent_measurement_format_can_save() {
        assert!(
            verify_recorded_measurements([32.0, 38.0], ["32.00".into(), " 38.0 ".into()]).is_ok()
        );
    }
}
