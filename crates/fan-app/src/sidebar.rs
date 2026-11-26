//! Native macOS translucent sidebar navigation with live status footer.
use crate::{
    components::{hstack, pin_edges, surface_box, vstack, StatusBadge},
    presenter::{Plan, Presentation},
    theme::{self, spacing, typography, SurfaceStyle},
    ui::{raw_text, rect, text},
};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel, DefinedClass, MainThreadOnly, Message,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSEdgeInsets, NSIndexSet, NSNotification, NSObject, NSObjectProtocol, NSSize,
};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(isize)]
pub enum Section {
    Overview = 0,
    Fans = 1,
    Smart = 2,
    Details = 3,
    Settings = 4,
}

impl Section {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Fans,
        Self::Smart,
        Self::Details,
        Self::Settings,
    ];

    pub fn from_index(index: isize) -> Option<Self> {
        Self::ALL.get(usize::try_from(index).ok()?).copied()
    }

    pub fn index(self) -> isize {
        self as isize
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Fans => "风扇",
            Self::Smart => "智能散热",
            Self::Details => "温度详情",
            Self::Settings => "设置",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Overview => "square.grid.2x2",
            Self::Fans => "fanblades",
            Self::Smart => "sparkles",
            Self::Details => "thermometer.medium",
            Self::Settings => "gearshape",
        }
    }
}

const CELL_ID: &str = "SidebarItem";
const ROW_HEIGHT: f64 = 32.0;

#[derive(Default)]
struct SidebarSourceIvars {
    target: RefCell<Option<Retained<AnyObject>>>,
    suppress_callback: Cell<bool>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = SidebarSourceIvars]
    struct SidebarSource;
    unsafe impl NSObjectProtocol for SidebarSource {}
    unsafe impl NSControlTextEditingDelegate for SidebarSource {}
    unsafe impl NSTableViewDataSource for SidebarSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn row_count(&self, _table: &NSTableView) -> isize {
            Section::ALL.len() as isize
        }
    }
    unsafe impl NSTableViewDelegate for SidebarSource {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn cell(
            &self,
            table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.cell_view(table, row)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, notification: &NSNotification) {
            if self.ivars().suppress_callback.get() {
                return;
            }
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(table) = object.downcast::<NSTableView>() else {
                return;
            };
            let row = table.selectedRow();
            if row < 0 {
                return;
            }
            if let Some(target) = self.ivars().target.borrow().as_ref() {
                let _: () = unsafe { msg_send![&**target, sidebarSelected: row] };
            }
        }
    }
);

impl SidebarSource {
    fn cell_view(&self, table: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let section = Section::from_index(row)?;
        let id = raw_text(CELL_ID);
        let mtm = self.mtm();
        let cell = unsafe { table.makeViewWithIdentifier_owner(&id, None) }
            .and_then(|v| v.downcast::<NSTableCellView>().ok())
            .unwrap_or_else(|| {
                let cell = NSTableCellView::initWithFrame(
                    NSTableCellView::alloc(mtm),
                    rect(0., 0., 180., ROW_HEIGHT),
                );
                cell.setIdentifier(Some(&id));
                let row_stack = hstack(mtm, spacing::SM);
                row_stack.setEdgeInsets(NSEdgeInsets {
                    top: 0.,
                    left: 6.,
                    bottom: 0.,
                    right: 6.,
                });
                let icon = NSImageView::new(mtm);
                icon.setTranslatesAutoresizingMaskIntoConstraints(false);
                icon.widthAnchor()
                    .constraintEqualToConstant(16.)
                    .setActive(true);
                icon.heightAnchor()
                    .constraintEqualToConstant(16.)
                    .setActive(true);
                let title = crate::components::label(
                    mtm,
                    "",
                    typography::BODY,
                    unsafe { NSFontWeightMedium },
                    false,
                    false,
                );
                row_stack.addArrangedSubview(&icon);
                row_stack.addArrangedSubview(&title);
                cell.addSubview(&row_stack);
                pin_edges(&row_stack, &cell);
                unsafe {
                    cell.setImageView(Some(&icon));
                    cell.setTextField(Some(&title));
                }
                cell
            });
        if let Some(icon) = unsafe { cell.imageView() } {
            if let Some(img) = theme::symbol_image(section.symbol(), None, 14.0) {
                icon.setImage(Some(&img));
            }
            icon.setContentTintColor(Some(&NSColor::controlAccentColor()));
        }
        if let Some(label) = unsafe { cell.textField() } {
            label.setStringValue(&text(section.title()));
        }
        Some(Retained::into_super(cell))
    }
}

pub(crate) struct Sidebar {
    pub controller: Retained<NSViewController>,
    table: Retained<NSTableView>,
    source: Retained<SidebarSource>,
    badge: StatusBadge,
    summary: Retained<NSTextField>,
    reset: Retained<NSButton>,
}

impl Sidebar {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let material = NSVisualEffectView::new(mtm);
        material.setMaterial(NSVisualEffectMaterial::Sidebar);
        material.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        material.setState(NSVisualEffectState::FollowsWindowActiveState);

        let container = vstack(mtm, spacing::SM);
        container.setEdgeInsets(NSEdgeInsets {
            top: 44.,
            left: spacing::MD,
            bottom: spacing::MD,
            right: spacing::MD,
        });
        container.setTranslatesAutoresizingMaskIntoConstraints(false);

        // Brand title above navigation list.
        let brand_row = hstack(mtm, spacing::SM);
        if let Some(icon) = theme::symbol_image("fanblades.fill", None, 15.0)
            .or_else(|| theme::symbol_image("fanblades", None, 15.0))
        {
            let img = NSImageView::imageViewWithImage(&icon, mtm);
            img.setContentTintColor(Some(&NSColor::controlAccentColor()));
            brand_row.addArrangedSubview(&img);
        }
        let brand = crate::components::label(
            mtm,
            "Fan Control",
            typography::SECTION,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        );
        brand_row.addArrangedSubview(&brand);
        container.addArrangedSubview(&brand_row);
        container.setCustomSpacing_afterView(spacing::MD, &brand_row);

        // Native SourceList table for keyboard + VoiceOver navigation.
        let scroll = NSScrollView::new(mtm);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setHasVerticalScroller(false);
        scroll.setDrawsBackground(false);
        let table = NSTableView::new(mtm);
        table.setHeaderView(None);
        table.setRowHeight(ROW_HEIGHT);
        table.setIntercellSpacing(NSSize::new(0., 2.));
        table.setStyle(NSTableViewStyle::SourceList);
        table.setBackgroundColor(&NSColor::clearColor());
        table.setAllowsEmptySelection(false);
        table.setAllowsMultipleSelection(false);
        table.setAccessibilityLabel(Some(&text("主导航")));
        let column =
            NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &raw_text(CELL_ID));
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        table.addTableColumn(&column);
        table.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        let source = SidebarSource::alloc(mtm).set_ivars(SidebarSourceIvars {
            target: RefCell::new(Some(target.retain())),
            suppress_callback: Cell::new(false),
        });
        let source: Retained<SidebarSource> = unsafe { msg_send![super(source), init] };
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }
        scroll.setDocumentView(Some(&table));
        container.addArrangedSubview(&scroll);
        scroll
            .widthAnchor()
            .constraintEqualToAnchor_constant(&container.widthAnchor(), -spacing::MD * 2.)
            .setActive(true);
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(ROW_HEIGHT * 5. + 16.)
            .setActive(true);

        // Push footer status card to the bottom of the sidebar.
        let vspacer = NSView::new(mtm);
        vspacer
            .setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Vertical);
        vspacer.setContentCompressionResistancePriority_forOrientation(
            1.0,
            NSLayoutConstraintOrientation::Vertical,
        );
        container.addArrangedSubview(&vspacer);

        let footer_stack = vstack(mtm, spacing::SM);
        let badge = StatusBadge::new(mtm, "正在检测…", crate::presenter::Tone::Info);
        let summary = crate::components::label(
            mtm,
            "—",
            typography::SMALL,
            unsafe { NSFontWeightRegular },
            true,
            true,
        );
        summary.setFont(Some(&theme::rounded_digits(typography::SMALL, unsafe {
            NSFontWeightMedium
        })));
        let reset = unsafe {
            NSButton::buttonWithTitle_target_action(
                &text("恢复系统自动"),
                Some(target),
                Some(sel!(reset:)),
                mtm,
            )
        };
        reset.setControlSize(NSControlSize::Small);
        footer_stack.addArrangedSubview(&badge.view);
        footer_stack.addArrangedSubview(&summary);
        footer_stack.addArrangedSubview(&reset);

        let footer_card = surface_box(
            mtm,
            &footer_stack,
            NSSize::new(10., 10.),
            SurfaceStyle::Card,
        );
        container.addArrangedSubview(&footer_card);
        footer_card
            .widthAnchor()
            .constraintEqualToAnchor_constant(&container.widthAnchor(), -spacing::MD * 2.)
            .setActive(true);

        material.addSubview(&container);
        pin_edges(&container, &material);

        let controller = NSViewController::new(mtm);
        controller.setView(&material);

        let sidebar = Self {
            controller,
            table,
            source,
            badge,
            summary,
            reset,
        };
        sidebar.select(Section::Overview);
        sidebar
    }

    pub fn select(&self, section: Section) {
        let index = section.index();
        if self.table.selectedRow() != index {
            self.source.ivars().suppress_callback.set(true);
            let set = NSIndexSet::indexSetWithIndex(index as usize);
            self.table
                .selectRowIndexes_byExtendingSelection(&set, false);
            self.source.ivars().suppress_callback.set(false);
        }
    }

    pub fn refresh(&self, view: &Presentation) {
        self.badge.set(
            view.notice.tone,
            match view.notice.tone {
                crate::presenter::Tone::Good => "运行正常",
                crate::presenter::Tone::Info => "提示",
                crate::presenter::Tone::Warning => "需要注意",
                crate::presenter::Tone::Danger => "温度偏高",
            },
        );
        let temp = view
            .temperature
            .map(|v| format!("{v:.0}°C"))
            .unwrap_or_else(|| "—".into());
        let plan_name = crate::i18n::translate(match view.plan {
            Plan::System => "系统默认",
            Plan::Smart => "智能散热",
            Plan::Custom => "自定义",
            Plan::Mixed => "各风扇分别设置",
        });
        self.summary
            .setStringValue(&raw_text(&format!("{temp} · {plan_name}")));
        self.reset.setEnabled(view.controls_enabled);
    }
}
