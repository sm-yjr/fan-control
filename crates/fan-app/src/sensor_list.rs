//! A view-based native table: AppKit owns scrolling and recycles offscreen cells.
//! Sampling updates data for every row, but touches only instantiated visible cells.
use crate::{
    form,
    gauge::{temperature_color, Bar},
    theme,
    ui::{raw_text, rect, text},
};
use fan_core::{Sensor, SensorGroup};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::ProtocolObject, DefinedClass, MainThreadOnly,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSSize};
use std::cell::{Cell, RefCell};

const ROW_HEIGHT: f64 = 38.;
const BAR_WIDTH: f64 = 156.;
const VALUE_WIDTH: f64 = 60.;
const CELL_ID: &str = "SensorReading";
const HEADER_ID: &str = "SensorGroup";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GroupFilter {
    #[default]
    All,
    Cpu,
    Gpu,
    System,
    Other,
}

impl GroupFilter {
    pub const ALL: [Self; 5] = [Self::All, Self::Cpu, Self::Gpu, Self::System, Self::Other];

    pub fn title(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Cpu => "处理器",
            Self::Gpu => "图形",
            Self::System => "机身",
            Self::Other => "其他",
        }
    }

    fn matches(self, group: SensorGroup) -> bool {
        match self {
            Self::All => true,
            Self::Cpu => group == SensorGroup::Cpu,
            Self::Gpu => group == SensorGroup::Gpu,
            Self::System => group == SensorGroup::System,
            Self::Other => group == SensorGroup::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SortOrder {
    #[default]
    Grouped,
    HottestFirst,
}

impl SortOrder {
    pub const ALL: [Self; 2] = [Self::Grouped, Self::HottestFirst];

    pub fn title(self) -> &'static str {
        match self {
            Self::Grouped => "按分组",
            Self::HottestFirst => "按最热",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Header(String),
    Reading(Sensor),
}

impl Entry {
    fn same_layout(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Header(a), Self::Header(b)) => a == b,
            (Self::Reading(a), Self::Reading(b)) => {
                a.key == b.key && a.name == b.name && a.group == b.group
            }
            _ => false,
        }
    }
}

fn group(group: SensorGroup) -> (u8, &'static str) {
    match group {
        SensorGroup::Cpu => (0, "处理器"),
        SensorGroup::Gpu => (1, "图形处理器"),
        SensorGroup::System => (2, "机身内部（不代表键盘表面温度）"),
        SensorGroup::Other => (3, "其他"),
    }
}

fn display_name(sensor: &Sensor) -> String {
    crate::i18n::translate(if sensor.name == sensor.key {
        "未确认的传感器"
    } else {
        &sensor.name
    })
}

fn is_headroom(sensor: &Sensor) -> bool {
    sensor.name == "CPU 热余量（非绝对温度）"
}

fn entries(
    sensors: &[Sensor],
    filter: &str,
    group_filter: GroupFilter,
    sort_order: SortOrder,
    fresh: bool,
) -> Vec<Entry> {
    let mut sensors: Vec<Sensor> = sensors
        .iter()
        .filter(|sensor| {
            group_filter.matches(sensor.group)
                && (filter.is_empty()
                    || sensor.key.to_lowercase().contains(filter)
                    || sensor.name.to_lowercase().contains(filter)
                    || display_name(sensor).to_lowercase().contains(filter))
        })
        .cloned()
        .map(|mut sensor| {
            sensor.value = sensor
                .value
                .filter(|value| fresh && fan_core::valid_temperature(*value));
            sensor
        })
        .collect();

    match sort_order {
        SortOrder::Grouped => {
            sensors.sort_by(|a, b| {
                group(a.group)
                    .0
                    .cmp(&group(b.group).0)
                    .then(a.key.cmp(&b.key))
            });
            let mut rows = Vec::with_capacity(sensors.len() + 4);
            let mut previous = None;
            for sensor in sensors {
                if previous != Some(sensor.group) {
                    rows.push(Entry::Header(group(sensor.group).1.into()));
                    previous = Some(sensor.group);
                }
                rows.push(Entry::Reading(sensor));
            }
            if rows.is_empty() {
                rows.push(Entry::Header(
                    if filter.is_empty() && group_filter == GroupFilter::All {
                        "正在读取传感器…"
                    } else {
                        "没有匹配的传感器。"
                    }
                    .into(),
                ));
            }
            rows
        }
        SortOrder::HottestFirst => {
            // Headroom is a margin rather than an absolute temperature, so sort it below real temperatures.
            sensors.sort_by(|a, b| {
                let rank = |s: &Sensor| match s.value {
                    Some(v) if !is_headroom(s) => (0u8, v),
                    Some(v) => (1u8, v),
                    None => (2u8, -1.0),
                };
                let (ra, va) = rank(a);
                let (rb, vb) = rank(b);
                ra.cmp(&rb)
                    .then_with(|| vb.total_cmp(&va))
                    .then_with(|| a.key.cmp(&b.key))
            });
            if sensors.is_empty() {
                vec![Entry::Header(
                    if filter.is_empty() && group_filter == GroupFilter::All {
                        "正在读取传感器…"
                    } else {
                        "没有匹配的传感器。"
                    }
                    .into(),
                )]
            } else {
                sensors.into_iter().map(Entry::Reading).collect()
            }
        }
    }
}

struct ReadingIvars {
    name: Retained<NSTextField>,
    key: Retained<NSTextField>,
    value: Retained<NSTextField>,
    bar: Retained<Bar>,
}

define_class!(
    #[unsafe(super=NSTableCellView)]
    #[thread_kind=MainThreadOnly]
    #[ivars=ReadingIvars]
    struct ReadingCell;
);

fn field(mtm: MainThreadMarker, size: f64, secondary: bool) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&raw_text(""), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize(size)));
    let color = if secondary {
        NSColor::secondaryLabelColor()
    } else {
        NSColor::labelColor()
    };
    field.setTextColor(Some(&color));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field
}

impl ReadingCell {
    fn new(mtm: MainThreadMarker, width: f64) -> Retained<Self> {
        let name = field(mtm, form::BODY, false);
        let key = field(mtm, form::CAPTION, true);
        let value = field(mtm, form::BODY, false);
        value.setFont(Some(&theme::rounded_digits(form::BODY, unsafe {
            NSFontWeightMedium
        })));
        value.setAlignment(NSTextAlignment::Right);
        let bar = Bar::new(mtm, BAR_WIDTH, 6.);
        // Local frame layout avoids an Auto Layout graph across the whole table.
        bar.setTranslatesAutoresizingMaskIntoConstraints(true);
        let this = Self::alloc(mtm).set_ivars(ReadingIvars {
            name,
            key,
            value,
            bar,
        });
        let this: Retained<Self> =
            unsafe { msg_send![super(this), initWithFrame: rect(0., 0., width, ROW_HEIGHT)] };
        this.setIdentifier(Some(&raw_text(CELL_ID)));
        let words_width = (width - BAR_WIDTH - VALUE_WIDTH - 48.).max(30.);
        let ivars = this.ivars();
        ivars.name.setFrame(rect(14., 18., words_width, 17.));
        ivars.key.setFrame(rect(14., 3., words_width, 14.));
        for field in [&ivars.name, &ivars.key] {
            field.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
            this.addSubview(field);
        }
        ivars.bar.setFrame(rect(
            width - VALUE_WIDTH - BAR_WIDTH - 22.,
            15.,
            BAR_WIDTH,
            8.,
        ));
        ivars
            .value
            .setFrame(rect(width - VALUE_WIDTH - 14., 10., VALUE_WIDTH, 18.));
        ivars
            .bar
            .setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        ivars
            .value
            .setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        this.addSubview(&ivars.bar);
        this.addSubview(&ivars.value);
        unsafe {
            this.setTextField(Some(&ivars.name));
        }
        this
    }

    fn update(&self, sensor: &Sensor) {
        let ivars = self.ivars();
        set_text(&ivars.name, &display_name(sensor));
        set_text(&ivars.key, &sensor.key);
        ivars.name.setToolTip(Some(&raw_text(&format!(
            "{} [{}]",
            display_name(sensor),
            sensor.key
        ))));
        match sensor.value {
            Some(value) => {
                set_text(&ivars.value, &format!("{value:.1}°C"));
                if is_headroom(sensor) {
                    ivars.bar.set(None, &NSColor::secondaryLabelColor());
                } else {
                    ivars.bar.set(
                        Some(((value - 20.) / 90.).clamp(0., 1.)),
                        &temperature_color(value),
                    );
                }
            }
            None => {
                set_text(&ivars.value, "--");
                ivars.bar.set(None, &NSColor::secondaryLabelColor());
            }
        }
        ivars.value.setAccessibilityLabel(Some(&raw_text(&format!(
            "{} [{}]",
            display_name(sensor),
            sensor.key
        ))));
    }
}

fn set_text(field: &NSTextField, value: &str) {
    if field.stringValue().to_string() != value {
        field.setStringValue(&raw_text(value));
    }
}

#[derive(Default)]
struct SourceIvars {
    rows: RefCell<Vec<Entry>>,
    created: Cell<usize>,
}

define_class!(
    #[unsafe(super=NSObject)]
    #[thread_kind=MainThreadOnly]
    #[ivars=SourceIvars]
    struct Source;
    unsafe impl NSObjectProtocol for Source {}
    unsafe impl NSControlTextEditingDelegate for Source {}
    unsafe impl NSTableViewDataSource for Source {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn row_count(&self, _table: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }
    unsafe impl NSTableViewDelegate for Source {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(
            &self,
            table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.cell_view(table, row)
        }
        #[unsafe(method(tableView:isGroupRow:))]
        fn group_row(&self, _table: &NSTableView, row: isize) -> bool {
            usize::try_from(row)
                .ok()
                .and_then(|i| {
                    self.ivars()
                        .rows
                        .borrow()
                        .get(i)
                        .map(|row| matches!(row, Entry::Header(_)))
                })
                .unwrap_or(false)
        }
    }
);

impl Source {
    fn cell_view(&self, table: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let rows = self.ivars().rows.borrow();
        let entry = rows.get(usize::try_from(row).ok()?)?;
        let id = raw_text(match entry {
            Entry::Header(_) => HEADER_ID,
            Entry::Reading(_) => CELL_ID,
        });
        let reused = unsafe { table.makeViewWithIdentifier_owner(&id, None) };
        if reused.is_none() {
            self.ivars().created.set(self.ivars().created.get() + 1);
        }
        match entry {
            Entry::Reading(sensor) => {
                let cell = reused
                    .and_then(|view| view.downcast::<ReadingCell>().ok())
                    .unwrap_or_else(|| {
                        ReadingCell::new(self.mtm(), table.tableColumns().objectAtIndex(0).width())
                    });
                cell.update(sensor);
                Some(Retained::into_super(Retained::into_super(cell)))
            }
            Entry::Header(title) => {
                let cell = reused
                    .and_then(|view| view.downcast::<NSTableCellView>().ok())
                    .unwrap_or_else(|| {
                        let width = table.tableColumns().objectAtIndex(0).width();
                        let cell = NSTableCellView::initWithFrame(
                            NSTableCellView::alloc(self.mtm()),
                            rect(0., 0., width, ROW_HEIGHT),
                        );
                        cell.setIdentifier(Some(&id));
                        let label = field(self.mtm(), form::CAPTION, true);
                        label.setFont(Some(&NSFont::systemFontOfSize_weight(
                            form::CAPTION,
                            unsafe { NSFontWeightSemibold },
                        )));
                        label.setFrame(rect(14., 10., width - 28., 18.));
                        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
                        cell.addSubview(&label);
                        unsafe {
                            cell.setTextField(Some(&label));
                        }
                        cell
                    });
                if let Some(label) = unsafe { cell.textField() } {
                    set_text(&label, &crate::i18n::translate(title));
                }
                Some(Retained::into_super(cell))
            }
        }
    }
}

pub(crate) struct SensorList {
    pub scroll: Retained<NSScrollView>,
    table: Retained<NSTableView>,
    source: Retained<Source>,
    filter: String,
    group_filter: GroupFilter,
    sort_order: SortOrder,
}

impl SensorList {
    pub fn new(mtm: MainThreadMarker, width: f64, height: f64) -> Self {
        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0., 0., width, height));
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        let table =
            NSTableView::initWithFrame(NSTableView::alloc(mtm), rect(0., 0., width, height));
        table.setHeaderView(None);
        table.setRowHeight(ROW_HEIGHT);
        table.setIntercellSpacing(NSSize::new(0., 0.));
        table.setStyle(NSTableViewStyle::Plain);
        table.setBackgroundColor(&NSColor::clearColor());
        table.setFloatsGroupRows(false);
        table.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::LastColumnOnlyAutoresizingStyle,
        );
        table.setAccessibilityLabel(Some(&text("全部传感器")));
        let column =
            NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &raw_text(CELL_ID));
        column.setWidth(width);
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        table.addTableColumn(&column);
        let source = Source::alloc(mtm).set_ivars(SourceIvars::default());
        let source: Retained<Source> = unsafe { msg_send![super(source), init] };
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }
        scroll.setDocumentView(Some(&table));
        Self {
            scroll,
            table,
            source,
            filter: String::new(),
            group_filter: GroupFilter::All,
            sort_order: SortOrder::Grouped,
        }
    }

    pub fn refresh(
        &mut self,
        sensors: &[Sensor],
        filter: &str,
        group_filter: GroupFilter,
        sort_order: SortOrder,
        fresh: bool,
    ) {
        let filter = filter.trim().to_lowercase();
        let next = entries(sensors, &filter, group_filter, sort_order, fresh);
        let changed = {
            let previous = self.source.ivars().rows.borrow();
            previous.len() != next.len()
                || !previous.iter().zip(&next).all(|(a, b)| a.same_layout(b))
        };
        self.source.ivars().rows.replace(next);
        if changed {
            self.table.reloadData();
            if filter != self.filter
                || group_filter != self.group_filter
                || sort_order != self.sort_order
            {
                self.table.scrollRowToVisible(0);
            }
        } else {
            let visible = self.table.rowsInRect(self.table.visibleRect());
            let rows = self.source.ivars().rows.borrow();
            for index in visible.location
                ..visible
                    .location
                    .saturating_add(visible.length)
                    .min(rows.len())
            {
                if let Entry::Reading(sensor) = &rows[index] {
                    if let Some(view) =
                        self.table
                            .viewAtColumn_row_makeIfNecessary(0, index as isize, false)
                    {
                        if let Ok(cell) = view.downcast::<ReadingCell>() {
                            cell.update(sensor);
                        }
                    }
                }
            }
        }
        self.filter = filter;
        self.group_filter = group_filter;
        self.sort_order = sort_order;
    }
}

impl SensorList {
    /// Runs only from --ui-smoke on the real details tab, using synthetic data.
    pub(crate) fn verify_native_list(&mut self, window: &NSWindow) {
        let list = self;
        let mut sensors: Vec<_> = (0..1000)
            .map(|index| Sensor {
                key: format!("X{index:03}"),
                name: format!("Probe {index:03}"),
                group: SensorGroup::Other,
                value: Some(45.),
            })
            .collect();
        list.refresh(&sensors, "", GroupFilter::All, SortOrder::Grouped, true);
        window.layoutIfNeeded();
        window.displayIfNeeded();
        assert_eq!(list.table.numberOfRows(), 1001);
        assert!(list.source.ivars().created.get() > 0);
        let mut durations = Vec::new();
        for index in [1, 51, 251, 501, 751, 1000, 501, 1] {
            let started = std::time::Instant::now();
            list.table.scrollRowToVisible(index);
            window.layoutIfNeeded();
            window.displayIfNeeded();
            durations.push(started.elapsed().as_secs_f64() * 1000.);
            let before = list.scroll.contentView().bounds().origin;
            sensors[(index - 1) as usize].value = Some(76.5);
            list.refresh(&sensors, "", GroupFilter::All, SortOrder::Grouped, true);
            window.layoutIfNeeded();
            assert_eq!(
                list.scroll.contentView().bounds().origin,
                before,
                "readings must not reset scrolling"
            );
            let view = list
                .table
                .viewAtColumn_row_makeIfNecessary(0, index, false)
                .expect("visible cell");
            let cell = view.downcast::<ReadingCell>().expect("reading cell");
            assert_eq!(cell.ivars().value.stringValue().to_string(), "76.5°C");
            list.refresh(&sensors, "", GroupFilter::All, SortOrder::Grouped, false);
            assert_eq!(
                cell.ivars().value.stringValue().to_string(),
                "--",
                "stale readings must disappear"
            );
        }
        let created = list.source.ivars().created.get();
        assert!(
            created < 100,
            "1000 readings must use a bounded set of cells: {created}"
        );
        list.refresh(&sensors, "x999", GroupFilter::All, SortOrder::Grouped, true);
        window.layoutIfNeeded();
        window.displayIfNeeded();
        assert_eq!(list.table.numberOfRows(), 2);
        let cell = list
            .table
            .viewAtColumn_row_makeIfNecessary(0, 1, false)
            .expect("filtered cell")
            .downcast::<ReadingCell>()
            .expect("reading cell");
        assert_eq!(cell.ivars().key.stringValue().to_string(), "X999");
        assert_eq!(cell.ivars().value.stringValue().to_string(), "76.5°C");
        list.refresh(
            &sensors,
            "no match",
            GroupFilter::All,
            SortOrder::Grouped,
            true,
        );
        window.layoutIfNeeded();
        window.displayIfNeeded();
        assert_eq!(list.table.numberOfRows(), 1);
        // Exercise hottest-first sort mode.
        list.refresh(
            &sensors,
            "",
            GroupFilter::All,
            SortOrder::HottestFirst,
            true,
        );
        window.layoutIfNeeded();
        window.displayIfNeeded();
        assert_eq!(list.table.numberOfRows(), 1000);
        list.refresh(&sensors, "", GroupFilter::All, SortOrder::Grouped, true);
        window.layoutIfNeeded();
        window.displayIfNeeded();
        assert_eq!(list.table.numberOfRows(), 1001);
        let max_ms = durations.into_iter().fold(0_f64, f64::max);
        println!("Native sensor table verified: 1000 readings, {created} cells allocated across 8 scrolls; slowest synchronous layout/display {max_ms:.2} ms; fresh/stale values, scroll position, sort and search passed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sensor(key: &str, group: SensorGroup, value: Option<f64>) -> Sensor {
        Sensor {
            key: key.into(),
            name: format!("Sensor {key}"),
            group,
            value,
        }
    }
    #[test]
    fn grouped_search_retains_identity_and_validity() {
        let sensors = [
            sensor("ZZ01", SensorGroup::Other, Some(40.)),
            sensor("Tp01", SensorGroup::Cpu, Some(65.)),
            sensor("Tp02", SensorGroup::Cpu, Some(f64::NAN)),
        ];
        let rows = entries(&sensors, "", GroupFilter::All, SortOrder::Grouped, true);
        assert!(matches!(&rows[1], Entry::Reading(s) if s.key == "Tp01" && s.value == Some(65.)));
        assert!(matches!(&rows[2], Entry::Reading(s) if s.key == "Tp02" && s.value.is_none()));
        assert_eq!(
            entries(&sensors, "tp01", GroupFilter::All, SortOrder::Grouped, true).len(),
            2
        );
        assert_eq!(
            entries(
                &sensors,
                "sensor zz",
                GroupFilter::All,
                SortOrder::Grouped,
                true
            )
            .len(),
            2
        );
        assert_eq!(
            entries(
                &sensors,
                "absent",
                GroupFilter::All,
                SortOrder::Grouped,
                true
            ),
            vec![Entry::Header("没有匹配的传感器。".into())]
        );
        assert!(
            entries(&sensors, "", GroupFilter::All, SortOrder::Grouped, false)
                .iter()
                .all(|row| !matches!(row, Entry::Reading(s) if s.value.is_some()))
        );
    }

    #[test]
    fn group_filter_and_hottest_sort_work_accurately() {
        let sensors = [
            sensor("ZZ01", SensorGroup::Other, Some(40.)),
            sensor("Tp01", SensorGroup::Cpu, Some(65.)),
            sensor("Tg01", SensorGroup::Gpu, Some(82.)),
            Sensor {
                key: "Th01".into(),
                name: "CPU 热余量（非绝对温度）".into(),
                group: SensorGroup::Cpu,
                value: Some(95.),
            },
        ];
        let cpu_only = entries(&sensors, "", GroupFilter::Cpu, SortOrder::Grouped, true);
        assert_eq!(cpu_only.len(), 3); // 1 header + 2 cpu rows

        let hottest = entries(
            &sensors,
            "",
            GroupFilter::All,
            SortOrder::HottestFirst,
            true,
        );
        assert_eq!(hottest.len(), 4);
        assert!(matches!(&hottest[0], Entry::Reading(s) if s.key == "Tg01"));
        assert!(matches!(&hottest[1], Entry::Reading(s) if s.key == "Tp01"));
        assert!(matches!(&hottest[2], Entry::Reading(s) if s.key == "ZZ01"));
        // Thermal headroom ranks below real temperatures even if numerically larger.
        assert!(matches!(&hottest[3], Entry::Reading(s) if s.key == "Th01"));
    }

    #[test]
    fn readings_do_not_invalidate_layout_but_semantics_do() {
        let first = Entry::Reading(sensor("Tp01", SensorGroup::Cpu, Some(65.)));
        let mut next = sensor("Tp01", SensorGroup::Cpu, None);
        assert!(first.same_layout(&Entry::Reading(next.clone())));
        next.name = "Confirmed meaning".into();
        assert!(!first.same_layout(&Entry::Reading(next.clone())));
        next.name = "Sensor Tp01".into();
        next.group = SensorGroup::Other;
        assert!(!first.same_layout(&Entry::Reading(next)));
    }
}
