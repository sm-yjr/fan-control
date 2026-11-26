//! Temperature details: resizable headline KPI tiles, battery summary and
//! full-height filterable/sortable native sensor table.
use crate::{
    components::{flex_spacer, hstack, surface_box, vstack, MetricTile},
    form::{self, fluid_caption},
    gauge::{temperature_color, Bar},
    popover::styled_label,
    sensor_list::{GroupFilter, SortOrder},
    theme::{self, spacing, typography, SurfaceStyle},
    ui::{raw_text, text},
    worker::UiSnapshot,
};
use objc2::{rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSEdgeInsets, NSSize};

const COOL: f64 = 20.;
const HOT: f64 = 110.;

pub(crate) struct DetailsPage {
    pub view: Retained<NSView>,
    cpu: MetricTile,
    gpu: MetricTile,
    pressure: MetricTile,
    rise: MetricTile,
    battery_card: Retained<NSView>,
    charge: Retained<NSTextField>,
    charge_bar: Retained<Bar>,
    battery_state: Retained<NSTextField>,
    battery_detail: Retained<NSTextField>,
    pub group_filter: Retained<NSSegmentedControl>,
    pub sort_order: Retained<NSSegmentedControl>,
    pub search: Retained<NSSearchField>,
    list: crate::sensor_list::SensorList,
}

fn fraction(celsius: f64) -> f64 {
    ((celsius - COOL) / (HOT - COOL)).clamp(0., 1.)
}

impl DetailsPage {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let root = NSView::new(mtm);
        root.setTranslatesAutoresizingMaskIntoConstraints(false);

        let top_stack = vstack(mtm, spacing::MD);
        top_stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        top_stack.setEdgeInsets(NSEdgeInsets {
            top: spacing::XL,
            left: spacing::XXL,
            bottom: 0.,
            right: spacing::XXL,
        });

        // 4 equal-width KPI tiles.
        let tiles = hstack(mtm, spacing::MD);
        tiles.setDistribution(NSStackViewDistribution::FillEqually);
        let cpu = MetricTile::new(mtm, "处理器", true);
        let gpu = MetricTile::new(mtm, "图形处理器", true);
        let pressure = MetricTile::new(mtm, "系统热压力", true);
        let rise = MetricTile::new(mtm, "机身升温", true);
        for tile in [&cpu.view, &gpu.view, &pressure.view, &rise.view] {
            tiles.addArrangedSubview(tile);
        }
        top_stack.addArrangedSubview(&tiles);
        tiles
            .widthAnchor()
            .constraintEqualToAnchor_constant(&top_stack.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Compact battery card (auto-hidden on desktop Macs).
        let charge = styled_label(mtm, "—", typography::METRIC_MEDIUM, true, false, None);
        charge.setFont(Some(&theme::rounded_digits(
            typography::METRIC_MEDIUM,
            unsafe { NSFontWeightSemibold },
        )));
        let charge_bar = Bar::new(mtm, 110., 7.);
        let battery_words = vstack(mtm, 2.);
        let battery_state = styled_label(mtm, "", form::BODY, true, false, None);
        let battery_detail = styled_label(mtm, "", form::CAPTION, false, true, None);
        battery_words.addArrangedSubview(&battery_state);
        battery_words.addArrangedSubview(&battery_detail);
        let battery_row = hstack(mtm, 14.);
        battery_row.addArrangedSubview(&styled_label(
            mtm,
            "电池",
            typography::SMALL,
            true,
            true,
            None,
        ));
        battery_row.addArrangedSubview(&charge);
        battery_row.addArrangedSubview(&charge_bar);
        battery_row.addArrangedSubview(&battery_words);
        battery_row.addArrangedSubview(&flex_spacer(mtm));
        let battery_group =
            surface_box(mtm, &battery_row, NSSize::new(14., 10.), SurfaceStyle::Card);
        top_stack.addArrangedSubview(&battery_group);
        battery_group
            .widthAnchor()
            .constraintEqualToAnchor_constant(&top_stack.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Sensor toolbar: group filter pills + sort toggle + search field.
        let toolbar = hstack(mtm, spacing::SM);
        let group_labels: Vec<_> = GroupFilter::ALL.iter().map(|g| text(g.title())).collect();
        let group_filter = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&group_labels),
                NSSegmentSwitchTracking::SelectOne,
                Some(target),
                Some(sel!(sensorFilterChanged:)),
                mtm,
            )
        };
        group_filter.setSelectedSegment(0);
        group_filter.setControlSize(NSControlSize::Small);
        group_filter.setAccessibilityLabel(Some(&text("按分组筛选传感器")));

        let sort_labels: Vec<_> = SortOrder::ALL.iter().map(|s| text(s.title())).collect();
        let sort_order = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&sort_labels),
                NSSegmentSwitchTracking::SelectOne,
                Some(target),
                Some(sel!(sensorFilterChanged:)),
                mtm,
            )
        };
        sort_order.setSelectedSegment(0);
        sort_order.setControlSize(NSControlSize::Small);
        sort_order.setAccessibilityLabel(Some(&text("传感器排序方式")));

        let search = NSSearchField::new(mtm);
        search.setPlaceholderString(Some(&text("搜索传感器")));
        search.setAccessibilityLabel(Some(&text("搜索传感器")));
        search
            .widthAnchor()
            .constraintEqualToConstant(172.)
            .setActive(true);

        toolbar.addArrangedSubview(&group_filter);
        toolbar.addArrangedSubview(&flex_spacer(mtm));
        toolbar.addArrangedSubview(&sort_order);
        toolbar.addArrangedSubview(&search);
        top_stack.addArrangedSubview(&toolbar);
        toolbar
            .widthAnchor()
            .constraintEqualToAnchor_constant(&top_stack.widthAnchor(), -spacing::XXL * 2.)
            .setActive(true);

        // Resizable sensor table card filling all remaining vertical space.
        let list = crate::sensor_list::SensorList::new(mtm, 560., 260.);
        let list_box = surface_box(mtm, &list.scroll, NSSize::new(0., 0.), SurfaceStyle::Card);

        let footer_note = fluid_caption(
            mtm,
            "颜色表示温度高低：绿色较凉，黄色温热，橙色偏热，红色过热。名称按芯片型号核对，未确认项保留原始标识，虚拟温度与热余量单独标明。",
        );
        footer_note.setTranslatesAutoresizingMaskIntoConstraints(false);

        root.addSubview(&top_stack);
        root.addSubview(&list_box);
        root.addSubview(&footer_note);

        for c in [
            top_stack
                .topAnchor()
                .constraintEqualToAnchor(&root.topAnchor()),
            top_stack
                .leadingAnchor()
                .constraintEqualToAnchor(&root.leadingAnchor()),
            top_stack
                .trailingAnchor()
                .constraintEqualToAnchor(&root.trailingAnchor()),
            list_box
                .topAnchor()
                .constraintEqualToAnchor_constant(&top_stack.bottomAnchor(), spacing::SM),
            list_box
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&root.leadingAnchor(), spacing::XXL),
            list_box
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&root.trailingAnchor(), -spacing::XXL),
            list_box
                .heightAnchor()
                .constraintGreaterThanOrEqualToConstant(180.),
            footer_note
                .topAnchor()
                .constraintEqualToAnchor_constant(&list_box.bottomAnchor(), spacing::SM),
            footer_note
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&root.leadingAnchor(), spacing::XXL),
            footer_note
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&root.trailingAnchor(), -spacing::XXL),
            footer_note
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&root.bottomAnchor(), -spacing::LG),
        ] {
            c.setActive(true);
        }

        Self {
            view: root,
            cpu,
            gpu,
            pressure,
            rise,
            battery_card: Retained::into_super(battery_group),
            charge,
            charge_bar,
            battery_state,
            battery_detail,
            group_filter,
            sort_order,
            search,
            list,
        }
    }

    pub fn verify_native_list(&mut self, window: &NSWindow) {
        self.list.verify_native_list(window);
    }

    pub fn refresh(&mut self, state: &UiSnapshot, fresh: bool) {
        let celsius = |tile: &MetricTile, value: Option<f64>| match value.filter(|_| fresh) {
            Some(value) => {
                tile.value
                    .setStringValue(&raw_text(&format!("{value:.0}°C")));
                if let Some(bar) = &tile.bar {
                    bar.set(Some(fraction(value)), &temperature_color(value));
                }
            }
            None => {
                tile.value.setStringValue(&raw_text("—"));
                if let Some(bar) = &tile.bar {
                    bar.set(None, &NSColor::secondaryLabelColor());
                }
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
            .setStringValue(&text(if fresh { word } else { "—" }));
        let word_color = if fresh {
            color.clone()
        } else {
            NSColor::labelColor()
        };
        self.pressure.value.setTextColor(Some(&word_color));
        if let Some(bar) = &self.pressure.bar {
            bar.set(fresh.then_some(level), &color);
        }
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
            if let Some(bar) = &self.rise.bar {
                bar.set(Some((rise.max(0.) / 2.).min(1.)), &rise_color);
            }
        } else {
            self.rise.value.setStringValue(&raw_text("—"));
            if let Some(bar) = &self.rise.bar {
                bar.set(None, &NSColor::secondaryLabelColor());
            }
        }

        self.battery_card.setHidden(state.battery.is_none());
        if let Some(battery) = state.battery.as_ref() {
            let charge = battery.charge_percent;
            self.charge.setStringValue(&raw_text(
                &charge
                    .map(|value| format!("{value:.0}%"))
                    .unwrap_or_else(|| "—".into()),
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

        let group_filter = GroupFilter::ALL
            .get(self.group_filter.selectedSegment().max(0) as usize)
            .copied()
            .unwrap_or(GroupFilter::All);
        let sort_order = SortOrder::ALL
            .get(self.sort_order.selectedSegment().max(0) as usize)
            .copied()
            .unwrap_or(SortOrder::Grouped);
        self.list.refresh(
            &state.snapshot.sensors,
            &self.search.stringValue().to_string(),
            group_filter,
            sort_order,
            fresh,
        );
    }
}
