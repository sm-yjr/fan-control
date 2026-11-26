//! Auto Layout curve editor with in-place state updates (no window teardown
//! or flicker on preset switch, point add/remove, source change, or undo).
use crate::{
    chart::CurvePreview,
    components::{flex_spacer, hstack, label, pin_edges, surface_box, vstack},
    form::fluid_caption,
    presenter::CurvePreset,
    shell::scrollable_host,
    theme::{self, spacing, typography, SurfaceStyle},
    ui::{raw_text, rect, text},
    worker::UiSnapshot,
};
use fan_core::{Config, ControlMode, Curve, CurvePoint, FanConfig, THERMAL_DEMAND_KEY};
use objc2::{rc::Retained, runtime::AnyObject, sel, MainThreadOnly, Message};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSEdgeInsets, NSSize};

const DEFAULT_WIDTH: f64 = 600.;
const DEFAULT_HEIGHT: f64 = 660.;
const MIN_WIDTH: f64 = 520.;
const MIN_HEIGHT: f64 = 560.;

pub(crate) struct CurveEditor {
    pub window: Retained<NSWindow>,
    delegate_ref: Retained<AnyObject>,
    pub fan: FanConfig,
    pub source: Retained<NSPopUpButton>,
    pub sources: Vec<String>,
    pub hysteresis: Retained<NSTextField>,
    hysteresis_unit: Retained<NSTextField>,
    pub presets: Retained<NSSegmentedControl>,
    pub chart: Retained<CurvePreview>,
    live_readout: Retained<NSTextField>,
    axis_title: Retained<NSTextField>,
    axis_max: Retained<NSTextField>,
    table_x_header: Retained<NSTextField>,
    points_stack: Retained<NSStackView>,
    pub rows: Vec<(Retained<NSTextField>, Retained<NSTextField>)>,
    add_button: Retained<NSButton>,
    undo_button: Retained<NSButton>,
    pub all_fans: Retained<NSButton>,
    pub error: Retained<NSTextField>,
    pub undo: Vec<FanConfig>,
    pub drag_recorded: bool,
}

fn push_button(
    mtm: MainThreadMarker,
    target: &AnyObject,
    title: &str,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(&text(title), Some(target), Some(action), mtm)
    }
}

fn numeric_field(mtm: MainThreadMarker, value: &str, width: f64) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&raw_text(value), mtm);
    field.setFont(Some(&theme::rounded_digits(typography::BODY, unsafe {
        NSFontWeightRegular
    })));
    field
        .widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
    field
}

fn source_title(key: &str, snapshot: &fan_core::Snapshot) -> String {
    match key {
        THERMAL_DEMAND_KEY => "热负荷 · 0–100%".into(),
        "Average CPU" => "CPU 平均温度 · °C".into(),
        "Hottest CPU" => "CPU 最高温度 · °C".into(),
        "Average GPU" => "GPU 平均温度 · °C".into(),
        "Hottest GPU" => "GPU 最高温度 · °C".into(),
        _ => snapshot
            .sensors
            .iter()
            .find(|sensor| sensor.key == key)
            .map(|sensor| format!("{} · {} · °C", sensor.name, key))
            .unwrap_or_else(|| format!("{key} · 暂不可用")),
    }
}

impl CurveEditor {
    pub fn new(
        mtm: MainThreadMarker,
        target: &AnyObject,
        mut fan: FanConfig,
        state: &UiSnapshot,
    ) -> Self {
        let curve = fan
            .curve
            .get_or_insert_with(|| Curve::balanced("native-balanced", THERMAL_DEMAND_KEY))
            .clone();

        let visible = NSScreen::mainScreen(mtm).map(|screen| screen.visibleFrame());
        let width = visible
            .map(|f| DEFAULT_WIDTH.min((f.size.width - 40.).max(MIN_WIDTH)))
            .unwrap_or(DEFAULT_WIDTH);
        let height = visible
            .map(|f| DEFAULT_HEIGHT.min((f.size.height - 60.).max(MIN_HEIGHT)))
            .unwrap_or(DEFAULT_HEIGHT);
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0., 0., width, height),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setTitle(&text("温度曲线"));
        window.setMinSize(NSSize::new(MIN_WIDTH, MIN_HEIGHT));
        window.center();

        let root = vstack(mtm, spacing::MD);
        root.setTranslatesAutoresizingMaskIntoConstraints(false);
        root.setEdgeInsets(NSEdgeInsets {
            top: spacing::XL,
            left: spacing::XXL,
            bottom: spacing::LG,
            right: spacing::XXL,
        });

        // Header: title + subtitle + live coordinate pill.
        let header_row = hstack(mtm, spacing::MD);
        let title_col = vstack(mtm, 2.);
        title_col.addArrangedSubview(&label(
            mtm,
            "温度曲线",
            typography::TITLE,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        ));
        title_col.addArrangedSubview(&fluid_caption(
            mtm,
            "拖动图中的圆点，或在下方修改数值。点击“保存并应用”后生效。",
        ));
        let live_readout = label(
            mtm,
            "",
            typography::SMALL,
            unsafe { NSFontWeightSemibold },
            true,
            false,
        );
        live_readout.setFont(Some(&theme::rounded_digits(typography::SMALL, unsafe {
            NSFontWeightSemibold
        })));
        let live_pill = surface_box(
            mtm,
            &live_readout,
            NSSize::new(8., 3.),
            SurfaceStyle::NeutralPill,
        );
        live_pill.setHidden(true);
        header_row.addArrangedSubview(&title_col);
        header_row.addArrangedSubview(&flex_spacer(mtm));
        header_row.addArrangedSubview(&live_pill);
        root.addArrangedSubview(&header_row);
        header_row
            .widthAnchor()
            .constraintEqualToAnchor_constant(&root.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Controls card: presets + hysteresis + control source.
        let controls_body = vstack(mtm, spacing::SM);
        let top_controls = hstack(mtm, spacing::MD);
        let preset_labels: Vec<_> = ["安静", "均衡", "凉爽"].iter().map(|n| text(n)).collect();
        let presets = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&preset_labels),
                NSSegmentSwitchTracking::SelectOne,
                Some(target),
                Some(sel!(curvePreset:)),
                mtm,
            )
        };
        presets.setSegmentDistribution(NSSegmentDistribution::FillEqually);
        presets.setAccessibilityLabel(Some(&text("曲线预设")));
        presets
            .widthAnchor()
            .constraintEqualToConstant(220.)
            .setActive(true);

        let hysteresis = numeric_field(mtm, &crate::presenter::format_value(curve.hysteresis), 56.);
        hysteresis.setToolTip(Some(&text("降温时延后减速的幅度，避免风扇来回变速。")));
        let hysteresis_unit = label(
            mtm,
            "%",
            typography::BODY,
            unsafe { NSFontWeightRegular },
            true,
            false,
        );
        top_controls.addArrangedSubview(&presets);
        top_controls.addArrangedSubview(&flex_spacer(mtm));
        top_controls.addArrangedSubview(&label(
            mtm,
            "迟滞",
            typography::BODY,
            unsafe { NSFontWeightRegular },
            false,
            false,
        ));
        top_controls.addArrangedSubview(&hysteresis);
        top_controls.addArrangedSubview(&hysteresis_unit);
        controls_body.addArrangedSubview(&top_controls);
        top_controls
            .widthAnchor()
            .constraintEqualToAnchor(&controls_body.widthAnchor())
            .setActive(true);

        let source_row = hstack(mtm, spacing::MD);
        source_row.addArrangedSubview(&label(
            mtm,
            "控制依据",
            typography::BODY,
            unsafe { NSFontWeightRegular },
            false,
            false,
        ));
        let source = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(mtm),
            rect(0., 0., 280., 26.),
            false,
        );
        let mut sources = vec![
            THERMAL_DEMAND_KEY.into(),
            "Average CPU".into(),
            "Hottest CPU".into(),
            "Average GPU".into(),
            "Hottest GPU".into(),
        ];
        sources.extend(state.snapshot.sensors.iter().map(|s| s.key.clone()));
        if !sources.contains(&curve.sensor_key) {
            sources.push(curve.sensor_key.clone());
        }
        for key in &sources {
            source.addItemWithTitle(&text(&source_title(key, &state.snapshot)));
        }
        source.setAccessibilityLabel(Some(&text("曲线控制源")));
        unsafe {
            source.setTarget(Some(target));
            source.setAction(Some(sel!(sourceChanged:)));
        }
        source_row.addArrangedSubview(&source);
        controls_body.addArrangedSubview(&source_row);
        source_row
            .widthAnchor()
            .constraintEqualToAnchor(&controls_body.widthAnchor())
            .setActive(true);

        let controls_card = surface_box(
            mtm,
            &controls_body,
            NSSize::new(14., 12.),
            SurfaceStyle::Card,
        );
        root.addArrangedSubview(&controls_card);
        controls_card
            .widthAnchor()
            .constraintEqualToAnchor_constant(&root.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Chart card.
        let chart_body = vstack(mtm, spacing::XS);
        let chart_top_row = hstack(mtm, spacing::SM);
        chart_top_row.addArrangedSubview(&label(
            mtm,
            "100%",
            typography::MICRO,
            unsafe { NSFontWeightMedium },
            true,
            false,
        ));
        chart_top_row.addArrangedSubview(&flex_spacer(mtm));
        chart_top_row.addArrangedSubview(&label(
            mtm,
            "灰色区域：交还系统，风扇可能停转 · 橙线：当前值",
            typography::MICRO,
            unsafe { NSFontWeightRegular },
            true,
            false,
        ));
        chart_body.addArrangedSubview(&chart_top_row);
        chart_top_row
            .widthAnchor()
            .constraintEqualToAnchor(&chart_body.widthAnchor())
            .setActive(true);

        let chart = CurvePreview::new(mtm, rect(0., 0., 500., 190.), curve.clone());
        chart.set_target(target);
        chart.setAccessibilityLabel(Some(&text("曲线预览，可拖动控制点")));
        chart
            .heightAnchor()
            .constraintEqualToConstant(196.)
            .setActive(true);
        chart_body.addArrangedSubview(&chart);
        chart
            .widthAnchor()
            .constraintEqualToAnchor(&chart_body.widthAnchor())
            .setActive(true);

        let axis_row = hstack(mtm, spacing::SM);
        axis_row.addArrangedSubview(&label(
            mtm,
            "0",
            typography::MICRO,
            unsafe { NSFontWeightRegular },
            true,
            false,
        ));
        axis_row.addArrangedSubview(&flex_spacer(mtm));
        let axis_title = label(
            mtm,
            "热负荷 →",
            typography::MICRO,
            unsafe { NSFontWeightMedium },
            true,
            false,
        );
        axis_row.addArrangedSubview(&axis_title);
        axis_row.addArrangedSubview(&flex_spacer(mtm));
        let axis_max = label(
            mtm,
            "100%",
            typography::MICRO,
            unsafe { NSFontWeightRegular },
            true,
            false,
        );
        axis_row.addArrangedSubview(&axis_max);
        chart_body.addArrangedSubview(&axis_row);
        axis_row
            .widthAnchor()
            .constraintEqualToAnchor(&chart_body.widthAnchor())
            .setActive(true);

        let chart_card = surface_box(mtm, &chart_body, NSSize::new(14., 10.), SurfaceStyle::Card);
        root.addArrangedSubview(&chart_card);
        chart_card
            .widthAnchor()
            .constraintEqualToAnchor_constant(&root.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Editable control-point table card.
        let table_body = vstack(mtm, spacing::SM);
        let table_header = hstack(mtm, spacing::MD);
        let table_x_header = label(
            mtm,
            "热负荷 %",
            typography::SMALL,
            unsafe { NSFontWeightSemibold },
            true,
            false,
        );
        table_x_header
            .widthAnchor()
            .constraintEqualToConstant(150.)
            .setActive(true);
        let table_y_header = label(
            mtm,
            "风扇速度 %",
            typography::SMALL,
            unsafe { NSFontWeightSemibold },
            true,
            false,
        );
        table_y_header
            .widthAnchor()
            .constraintEqualToConstant(120.)
            .setActive(true);
        table_header.addArrangedSubview(&table_x_header);
        table_header.addArrangedSubview(&table_y_header);
        table_header.addArrangedSubview(&flex_spacer(mtm));
        table_body.addArrangedSubview(&table_header);
        table_header
            .widthAnchor()
            .constraintEqualToAnchor(&table_body.widthAnchor())
            .setActive(true);

        let points_stack = vstack(mtm, spacing::XS);
        points_stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        let points_scroll = scrollable_host(mtm, &points_stack);
        points_scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(120.)
            .setActive(true);
        table_body.addArrangedSubview(&points_scroll);
        points_scroll
            .widthAnchor()
            .constraintEqualToAnchor(&table_body.widthAnchor())
            .setActive(true);

        let table_actions = hstack(mtm, spacing::SM);
        let add_button = push_button(mtm, target, "添加控制点", sel!(addPoint:));
        add_button.setControlSize(NSControlSize::Small);
        let undo_button = push_button(mtm, target, "撤销", sel!(undoCurve:));
        undo_button.setControlSize(NSControlSize::Small);
        let all_fans = push_button(mtm, target, "同时应用到所有风扇", sel!(curveAllFans:));
        all_fans.setButtonType(NSButtonType::Switch);
        all_fans.setState(NSControlStateValueOn);
        table_actions.addArrangedSubview(&add_button);
        table_actions.addArrangedSubview(&undo_button);
        table_actions.addArrangedSubview(&flex_spacer(mtm));
        table_actions.addArrangedSubview(&all_fans);
        table_body.addArrangedSubview(&table_actions);
        table_actions
            .widthAnchor()
            .constraintEqualToAnchor(&table_body.widthAnchor())
            .setActive(true);

        let table_card = surface_box(mtm, &table_body, NSSize::new(14., 12.), SurfaceStyle::Card);
        root.addArrangedSubview(&table_card);
        table_card
            .widthAnchor()
            .constraintEqualToAnchor_constant(&root.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Footer: error/safety notice + Cancel / Save & Apply.
        let error = fluid_caption(mtm, "低于 0% 时交还系统；过热时安全保护始终生效。");
        root.addArrangedSubview(&error);

        let footer = hstack(mtm, spacing::SM);
        footer.addArrangedSubview(&flex_spacer(mtm));
        let cancel = push_button(mtm, target, "取消", sel!(cancelCurve:));
        cancel.setKeyEquivalent(&raw_text("\u{1b}"));
        let save = push_button(mtm, target, "保存并应用", sel!(saveCurve:));
        save.setKeyEquivalent(&raw_text("\r"));
        footer.addArrangedSubview(&cancel);
        footer.addArrangedSubview(&save);
        root.addArrangedSubview(&footer);
        footer
            .widthAnchor()
            .constraintEqualToAnchor_constant(&root.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        let content_view = window.contentView().expect("editor content view");
        content_view.addSubview(&root);
        pin_edges(&root, &content_view);

        let mut editor = Self {
            window,
            delegate_ref: target.retain(),
            fan,
            source,
            sources,
            hysteresis,
            hysteresis_unit,
            presets,
            chart,
            live_readout,
            axis_title,
            axis_max,
            table_x_header,
            points_stack,
            rows: Vec::new(),
            add_button,
            undo_button,
            all_fans,
            error,
            undo: Vec::new(),
            drag_recorded: false,
        };
        editor.sync_ui_from_fan();
        editor
    }

    /// Updates all fields, labels, chart, and editable point rows in place
    /// without destroying or recreating the NSWindow.
    pub fn apply_draft(&mut self, mut next: FanConfig, record_undo: bool) {
        if record_undo {
            let source = self
                .fan
                .curve
                .as_ref()
                .map(|curve| curve.sensor_key.clone());
            let previous = self
                .read_values(None, source.as_deref())
                .unwrap_or_else(|_| self.fan.clone());
            if self.undo.last() != Some(&previous) {
                self.undo.push(previous);
            }
            if self.undo.len() > 50 {
                self.undo.remove(0);
            }
        }
        next.curve
            .get_or_insert_with(|| Curve::balanced("native-balanced", THERMAL_DEMAND_KEY));
        self.fan = next;
        self.drag_recorded = false;
        self.error
            .setStringValue(&text("低于 0% 时交还系统；过热时安全保护始终生效。"));
        self.sync_ui_from_fan();
    }

    pub fn sync_ui_from_fan(&mut self) {
        let mtm = self.window.mtm();
        let Some(curve) = self.fan.curve.clone() else {
            return;
        };
        let thermal = curve.sensor_key == THERMAL_DEMAND_KEY;
        if let Some(index) = self.sources.iter().position(|key| *key == curve.sensor_key) {
            self.source.selectItemAtIndex(index as isize);
        }
        self.hysteresis
            .setStringValue(&raw_text(&crate::presenter::format_value(curve.hysteresis)));
        self.hysteresis_unit
            .setStringValue(&raw_text(if thermal { "%" } else { "°C" }));
        self.hysteresis
            .setAccessibilityLabel(Some(&text(if thermal {
                "迟滞，热负荷百分点"
            } else {
                "迟滞，摄氏度"
            })));
        self.axis_title.setStringValue(&text(if thermal {
            "热负荷 →"
        } else {
            "温度 →"
        }));
        self.axis_max
            .setStringValue(&raw_text(if thermal { "100%" } else { "120°C" }));
        self.table_x_header.setStringValue(&text(if thermal {
            "热负荷 %"
        } else {
            "温度 °C"
        }));
        match crate::presenter::matching_preset(&curve) {
            Some(preset) => self.presets.setSelectedSegment(
                CurvePreset::ALL
                    .iter()
                    .position(|p| *p == preset)
                    .unwrap_or(1) as isize,
            ),
            None => self.presets.setSelectedSegment(-1),
        }
        self.chart.set_curve(curve.clone());

        // Rebuild only the point rows in place.
        for child in self.points_stack.arrangedSubviews().to_vec() {
            self.points_stack.removeArrangedSubview(&child);
            child.removeFromSuperview();
        }
        self.rows.clear();
        let mut points = curve.points;
        points.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
        let can_delete = points.len() > 2;
        for (i, point) in points.iter().enumerate() {
            let row = hstack(mtm, spacing::MD);
            let x = numeric_field(
                mtm,
                &crate::presenter::format_value(point.temperature),
                150.,
            );
            let speed = numeric_field(
                mtm,
                &crate::presenter::format_value(point.speed_percent),
                120.,
            );
            x.setAccessibilityLabel(Some(&text(&format!(
                "控制点 {} 输入{}",
                i + 1,
                if thermal {
                    "热负荷百分比"
                } else {
                    "温度摄氏度"
                }
            ))));
            speed.setAccessibilityLabel(Some(&text(&format!("控制点 {} 速度百分比", i + 1))));
            let delete = push_button(mtm, &self.delegate_ref, "删除", sel!(removePoint:));
            delete.setControlSize(NSControlSize::Small);
            delete.setTag(i as isize);
            delete.setAccessibilityLabel(Some(&text(&format!("删除控制点 {}", i + 1))));
            delete.setEnabled(can_delete);
            row.addArrangedSubview(&x);
            row.addArrangedSubview(&speed);
            row.addArrangedSubview(&flex_spacer(mtm));
            row.addArrangedSubview(&delete);
            self.points_stack.addArrangedSubview(&row);
            row.widthAnchor()
                .constraintEqualToAnchor(&self.points_stack.widthAnchor())
                .setActive(true);
            self.rows.push((x, speed));
        }
        self.add_button.setEnabled(self.rows.len() < 12);
        self.undo_button.setEnabled(!self.undo.is_empty());
        self.update_live_readout();
    }

    pub fn update_live_readout(&self) {
        let thermal = self
            .fan
            .curve
            .as_ref()
            .is_some_and(|c| c.sensor_key == THERMAL_DEMAND_KEY);
        let unit = if thermal { "%" } else { "°C" };
        if let Some(point) = self.chart.active_point() {
            self.live_readout.setStringValue(&raw_text(&format!(
                "{}{} → {}%",
                crate::presenter::format_value(point.temperature),
                unit,
                crate::presenter::format_value(point.speed_percent),
            )));
            if let Some(pill) = unsafe { self.live_readout.superview() } {
                pill.setHidden(false);
            }
        } else if let Some(pill) = unsafe { self.live_readout.superview() } {
            pill.setHidden(true);
        }
    }

    pub fn read(&self) -> Result<FanConfig, String> {
        let mut fan = self.read_draft()?;
        if let Some(curve) = fan.curve.as_mut() {
            curve
                .points
                .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
        }
        let config = Config {
            version: fan_core::CONFIG_VERSION,
            fans: vec![fan.clone()],
            ..Config::default()
        };
        config.validate().map_err(|e| e.to_string())?;
        Ok(fan)
    }

    pub fn read_draft(&self) -> Result<FanConfig, String> {
        self.read_values(None, None)
    }

    pub fn read_values(
        &self,
        skip: Option<usize>,
        source_override: Option<&str>,
    ) -> Result<FanConfig, String> {
        let mut fan = self.fan.clone();
        let curve = fan.curve.as_mut().ok_or("缺少曲线")?;
        let index = self.source.indexOfSelectedItem().max(0) as usize;
        let source = source_override
            .map(str::to_owned)
            .or_else(|| self.sources.get(index).cloned())
            .ok_or("选择控制源")?;
        if (source == THERMAL_DEMAND_KEY) != (curve.sensor_key == THERMAL_DEMAND_KEY) {
            return Err("更换温度 / 热负荷单位前，请恢复对应默认曲线。".into());
        }
        curve.sensor_key = source;
        curve.hysteresis = self
            .hysteresis
            .stringValue()
            .to_string()
            .trim()
            .parse()
            .map_err(|_| "迟滞必须是数字")?;
        curve.points = self
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| skip != Some(*i))
            .map(|(_, (x, y))| {
                Ok(CurvePoint {
                    temperature: x
                        .stringValue()
                        .to_string()
                        .trim()
                        .parse()
                        .map_err(|_| "输入值必须是数字")?,
                    speed_percent: y
                        .stringValue()
                        .to_string()
                        .trim()
                        .parse()
                        .map_err(|_| "速度必须是数字")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        fan.mode = ControlMode::Curve {
            curve_id: curve.id.clone(),
        };
        Ok(fan)
    }
}
