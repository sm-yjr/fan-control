//! Temperature details: headline components, battery and every sensor as a
//! colored bar, so hot spots stand out without reading numbers.
use crate::form::{self, caption, spacer, CONTENT};
use crate::gauge::{temperature_color, Bar};
use crate::popover::{stack, styled_label, tinted_box, TintedView};
use crate::ui::{raw_text, text};
use crate::worker::UiSnapshot;
use fan_core::SensorGroup;
use objc2::{define_class, msg_send, rc::Retained, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSSize};

const TILE_GAP: f64 = 10.;
const TILE: f64 = (CONTENT - TILE_GAP * 3.) / 4.;
const LIST_HEIGHT: f64 = 250.;
const LIST_WIDTH: f64 = CONTENT - 16.;
const BAR: f64 = 150.;
/// Bars span this range so typical readings spread across the width.
const COOL: f64 = 20.;
const HOT: f64 = 110.;

define_class!(
    /// Top-left origin, so a scrolled list starts at its first row.
    #[unsafe(super=NSView)]
    #[thread_kind=MainThreadOnly]
    pub(crate) struct Flipped;
    impl Flipped {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

struct Tile {
    value: Retained<NSTextField>,
    bar: Retained<Bar>,
}

struct SensorRow {
    value: Retained<NSTextField>,
    bar: Retained<Bar>,
}

pub(crate) struct DetailsPage {
    pub view: Retained<NSStackView>,
    cpu: Tile,
    gpu: Tile,
    pressure: Tile,
    rise: Tile,
    battery_section: Vec<Retained<NSView>>,
    charge: Retained<NSTextField>,
    charge_bar: Retained<Bar>,
    battery_state: Retained<NSTextField>,
    battery_detail: Retained<NSTextField>,
    pub search: Retained<NSSearchField>,
    list: Retained<NSStackView>,
    rows: Vec<SensorRow>,
    /// Sensor keys and the filter the current rows were built for.
    layout: Option<(String, Vec<String>)>,
}

fn tile(mtm: MainThreadMarker, title: &str) -> (Retained<TintedView>, Tile) {
    let content = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 4.);
    content.addArrangedSubview(&styled_label(mtm, title, form::CAPTION, false, true, None));
    let value = styled_label(mtm, "--", 20., true, false, None);
    value.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
        20.,
        unsafe { NSFontWeightMedium },
    )));
    value.setAccessibilityLabel(Some(&text(title)));
    content.addArrangedSubview(&value);
    let bar = Bar::new(mtm, TILE - 24., 4.);
    content.addArrangedSubview(&bar);
    let view = tinted_box(mtm, &content, NSSize::new(12., 10.));
    view.set_fill(&NSColor::quaternarySystemFillColor());
    view.widthAnchor()
        .constraintEqualToConstant(TILE)
        .setActive(true);
    (view, Tile { value, bar })
}

fn group_label(group: SensorGroup) -> &'static str {
    match group {
        SensorGroup::Cpu => "处理器",
        SensorGroup::Gpu => "图形处理器",
        SensorGroup::System => "机身内部（不代表键盘表面温度）",
        SensorGroup::Other => "其他",
    }
}

fn group_order(group: SensorGroup) -> u8 {
    match group {
        SensorGroup::Cpu => 0,
        SensorGroup::Gpu => 1,
        SensorGroup::System => 2,
        SensorGroup::Other => 3,
    }
}

fn fraction(celsius: f64) -> f64 {
    ((celsius - COOL) / (HOT - COOL)).clamp(0., 1.)
}

impl DetailsPage {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let view = form::page(mtm);
        let tiles = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, TILE_GAP);
        let (cpu_box, cpu) = tile(mtm, "处理器");
        let (gpu_box, gpu) = tile(mtm, "图形处理器");
        let (pressure_box, pressure) = tile(mtm, "系统热压力");
        let (rise_box, rise) = tile(mtm, "机身升温");
        for tile in [&cpu_box, &gpu_box, &pressure_box, &rise_box] {
            tiles.addArrangedSubview(tile);
        }
        view.addArrangedSubview(&tiles);
        view.setCustomSpacing_afterView(form::SECTION_GAP, &tiles);

        let battery_title = form::section_title(mtm, "电池");
        let charge = styled_label(mtm, "--", 20., true, false, None);
        charge.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            20.,
            unsafe { NSFontWeightMedium },
        )));
        let charge_bar = Bar::new(mtm, 120., 8.);
        let battery_words = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 2.);
        let battery_state = styled_label(mtm, "", form::BODY, false, false, None);
        let battery_detail = styled_label(mtm, "", form::CAPTION, false, true, None);
        battery_words.addArrangedSubview(&battery_state);
        battery_words.addArrangedSubview(&battery_detail);
        let battery_row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 14.);
        battery_row.addArrangedSubview(&charge);
        battery_row.addArrangedSubview(&charge_bar);
        battery_row.addArrangedSubview(&battery_words);
        battery_row.addArrangedSubview(&spacer(mtm));
        battery_row
            .widthAnchor()
            .constraintEqualToConstant(form::ROW)
            .setActive(true);
        let battery_group = form::group(mtm, &[&battery_row]);
        view.addArrangedSubview(&battery_title);
        view.addArrangedSubview(&battery_group);
        view.setCustomSpacing_afterView(form::SECTION_GAP, &battery_group);

        let header = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
        header.addArrangedSubview(&form::section_title(mtm, "全部传感器"));
        header.addArrangedSubview(&spacer(mtm));
        let search = NSSearchField::new(mtm);
        search.setPlaceholderString(Some(&text("搜索传感器")));
        search.setAccessibilityLabel(Some(&text("搜索传感器")));
        search
            .widthAnchor()
            .constraintEqualToConstant(200.)
            .setActive(true);
        header.addArrangedSubview(&search);
        header
            .widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        view.addArrangedSubview(&header);

        let list = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 6.);
        list.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top: 10.,
            left: 12.,
            bottom: 10.,
            right: 12.,
        });
        list.setTranslatesAutoresizingMaskIntoConstraints(false);
        let document: Retained<Flipped> = unsafe { msg_send![Flipped::alloc(mtm), init] };
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        document.addSubview(&list);
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&document));
        let clip = scroll.contentView();
        for constraint in [
            list.topAnchor()
                .constraintEqualToAnchor(&document.topAnchor()),
            list.leadingAnchor()
                .constraintEqualToAnchor(&document.leadingAnchor()),
            document
                .bottomAnchor()
                .constraintEqualToAnchor(&list.bottomAnchor()),
            document.widthAnchor().constraintEqualToConstant(LIST_WIDTH),
            document
                .topAnchor()
                .constraintEqualToAnchor(&clip.topAnchor()),
            document
                .leadingAnchor()
                .constraintEqualToAnchor(&clip.leadingAnchor()),
        ] {
            constraint.setActive(true);
        }
        let list_box = tinted_box(mtm, &scroll, NSSize::new(0., 0.));
        list_box.set_fill(&NSColor::quaternarySystemFillColor());
        list_box
            .widthAnchor()
            .constraintEqualToConstant(CONTENT)
            .setActive(true);
        list_box
            .heightAnchor()
            .constraintEqualToConstant(LIST_HEIGHT)
            .setActive(true);
        view.addArrangedSubview(&list_box);
        view.addArrangedSubview(&caption(
            mtm,
            "颜色表示温度高低：绿色较凉，黄色温热，橙色偏热，红色过热。芯片在高负载下短时达到 90°C 以上属于正常现象。",
            CONTENT,
        ));
        Self {
            view,
            cpu,
            gpu,
            pressure,
            rise,
            battery_section: vec![
                Retained::into_super(Retained::into_super(battery_title)),
                Retained::into_super(battery_group),
            ],
            charge,
            charge_bar,
            battery_state,
            battery_detail,
            search,
            list,
            rows: Vec::new(),
            layout: None,
        }
    }

    pub fn refresh(&mut self, state: &UiSnapshot, fresh: bool) {
        let mtm = MainThreadMarker::new().expect("UI main thread");
        let celsius = |tile: &Tile, value: Option<f64>| match value.filter(|_| fresh) {
            Some(value) => {
                tile.value
                    .setStringValue(&raw_text(&format!("{value:.0}°C")));
                tile.bar
                    .set(Some(fraction(value)), &temperature_color(value));
            }
            None => {
                tile.value.setStringValue(&raw_text("--"));
                tile.bar.set(None, &NSColor::secondaryLabelColor());
            }
        };
        celsius(
            &self.cpu,
            state.snapshot.input_value("Average CPU", &state.thermal),
        );
        celsius(
            &self.gpu,
            state.snapshot.input_value("Average GPU", &state.thermal),
        );
        let (word, level, color) = match state.snapshot.thermal_pressure {
            fan_core::ThermalPressure::Nominal => ("正常", 0.15, NSColor::systemGreenColor()),
            fan_core::ThermalPressure::Fair => ("升高", 0.45, NSColor::systemYellowColor()),
            fan_core::ThermalPressure::Serious => ("严重", 0.75, NSColor::systemOrangeColor()),
            fan_core::ThermalPressure::Critical => ("临界", 1., NSColor::systemRedColor()),
        };
        self.pressure
            .value
            .setStringValue(&text(if fresh { word } else { "--" }));
        let word_color = if fresh {
            color.clone()
        } else {
            NSColor::labelColor()
        };
        self.pressure.value.setTextColor(Some(&word_color));
        self.pressure.bar.set(fresh.then_some(level), &color);
        let rise = state.thermal.chassis_rise_per_minute;
        if fresh && state.thermal.uses_chassis_sensor && rise.is_finite() {
            self.rise
                .value
                .setStringValue(&text(&format!("{rise:+.1}°C/分")));
            let rise_color = if rise > 1. {
                NSColor::systemOrangeColor()
            } else {
                NSColor::systemGreenColor()
            };
            self.rise
                .bar
                .set(Some((rise.max(0.) / 2.).min(1.)), &rise_color);
        } else {
            self.rise.value.setStringValue(&raw_text("--"));
            self.rise.bar.set(None, &NSColor::secondaryLabelColor());
        }

        for view in &self.battery_section {
            view.setHidden(state.battery.is_none());
        }
        if let Some(battery) = state.battery.as_ref() {
            let charge = battery.charge_percent;
            self.charge.setStringValue(&raw_text(
                &charge
                    .map(|value| format!("{value:.0}%"))
                    .unwrap_or_else(|| "--".into()),
            ));
            let charge_color = if charge.is_some_and(|value| value <= 20.) {
                NSColor::systemOrangeColor()
            } else {
                NSColor::systemGreenColor()
            };
            self.charge_bar
                .set(charge.map(|value| value / 100.), &charge_color);
            let (line, detail) = crate::presenter::battery_lines(battery);
            self.battery_state.setStringValue(&text(&line));
            let detail: Vec<_> = detail
                .iter()
                .map(|part| crate::i18n::translate(part))
                .collect();
            self.battery_detail
                .setStringValue(&raw_text(&detail.join(" · ")));
        }

        let filter = self.search.stringValue().to_string().trim().to_lowercase();
        let mut sensors: Vec<_> = state
            .snapshot
            .sensors
            .iter()
            .filter(|sensor| {
                filter.is_empty()
                    || sensor.name.to_lowercase().contains(&filter)
                    || crate::i18n::translate(&sensor.name)
                        .to_lowercase()
                        .contains(&filter)
                    || sensor.key.to_lowercase().contains(&filter)
            })
            .collect();
        sensors.sort_by_key(|sensor| (group_order(sensor.group), sensor.key.clone()));
        let keys: Vec<_> = sensors.iter().map(|sensor| sensor.key.clone()).collect();
        let layout = Some((filter, keys));
        if self.layout != layout {
            for view in self.list.arrangedSubviews().iter() {
                view.removeFromSuperview();
            }
            self.rows.clear();
            let mut group = None;
            for sensor in &sensors {
                if group != Some(sensor.group) {
                    let header = styled_label(
                        mtm,
                        group_label(sensor.group),
                        form::CAPTION,
                        true,
                        true,
                        None,
                    );
                    if group.is_some() {
                        if let Some(last) = self.list.arrangedSubviews().lastObject() {
                            self.list.setCustomSpacing_afterView(14., &last);
                        }
                    }
                    self.list.addArrangedSubview(&header);
                    group = Some(sensor.group);
                }
                let row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
                let name = styled_label(mtm, &sensor.name, form::BODY, false, false, None);
                let key = styled_label(mtm, &sensor.key, form::CAPTION, false, true, None);
                key.setTextColor(Some(&NSColor::tertiaryLabelColor()));
                let bar = Bar::new(mtm, BAR, 6.);
                let value = styled_label(mtm, "--", form::BODY, false, false, None);
                value.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                    form::BODY,
                    unsafe { NSFontWeightRegular },
                )));
                value.setAlignment(NSTextAlignment::Right);
                value
                    .widthAnchor()
                    .constraintEqualToConstant(56.)
                    .setActive(true);
                row.addArrangedSubview(&name);
                row.addArrangedSubview(&key);
                row.addArrangedSubview(&spacer(mtm));
                row.addArrangedSubview(&bar);
                row.addArrangedSubview(&value);
                row.widthAnchor()
                    .constraintEqualToConstant(LIST_WIDTH - 24.)
                    .setActive(true);
                self.list.addArrangedSubview(&row);
                self.rows.push(SensorRow { value, bar });
            }
            if sensors.is_empty() {
                self.list.addArrangedSubview(&styled_label(
                    mtm,
                    if state.snapshot.sensors.is_empty() {
                        "正在读取传感器…"
                    } else {
                        "没有匹配的传感器。"
                    },
                    form::BODY,
                    false,
                    true,
                    None,
                ));
            }
            self.layout = layout;
        }
        for (row, sensor) in self.rows.iter().zip(&sensors) {
            match sensor.value.filter(|_| fresh) {
                Some(value) => {
                    row.value.setStringValue(&raw_text(&format!("{value:.1}°")));
                    row.bar
                        .set(Some(fraction(value)), &temperature_color(value));
                }
                None => {
                    row.value.setStringValue(&raw_text("--"));
                    row.bar.set(None, &NSColor::secondaryLabelColor());
                }
            }
        }
    }
}
