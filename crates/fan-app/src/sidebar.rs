//! Native macOS translucent sidebar styled after System Settings: top app status
//! hero, colorful squircle navigation icons, and a quiet safety handback action.
use crate::{
    components::{hstack, pin_edges, vstack, BadgeTint, SquircleIcon},
    presenter::{Plan, Presentation, Tone},
    theme::{self, spacing, typography},
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
            Self::Overview => "square.grid.2x2.fill",
            Self::Fans => "fanblades.fill",
            Self::Smart => "sparkles",
            Self::Details => "thermometer.medium",
            Self::Settings => "gearshape.fill",
        }
    }

    fn badge_tint(self) -> BadgeTint {
        match self {
            Self::Overview => BadgeTint::Blue,
            Self::Fans => BadgeTint::Teal,
            Self::Smart => BadgeTint::Purple,
            Self::Details => BadgeTint::Orange,
            Self::Settings => BadgeTint::Gray,
        }
    }
}

const CELL_ID: &str = "SidebarItem";
const ROW_HEIGHT: f64 = 34.0;
const SIDEBAR_COLUMN_INSET: f64 = 18.0;

define_class!(
    /// Subtle translucent rounded pill selection matching modern macOS System Settings
    /// without inverting text color or breaking offscreen rendering.
    #[unsafe(super = NSTableRowView)]
    #[thread_kind = MainThreadOnly]
    struct SidebarRowView;
    impl SidebarRowView {
        #[unsafe(method(interiorBackgroundStyle))]
        fn interior_background_style(&self) -> NSBackgroundStyle {
            NSBackgroundStyle::Normal
        }

        #[unsafe(method(drawSelectionInRect:))]
        fn draw_selection(&self, _dirty: objc2_foundation::NSRect) {
            if !self.isSelected() {
                return;
            }
            let b = self.bounds();
            let pill = objc2_foundation::NSRect::new(
                objc2_foundation::NSPoint::new(b.origin.x + 10.0, b.origin.y + 1.0),
                NSSize::new((b.size.width - 20.0).max(0.0), (b.size.height - 2.0).max(0.0)),
            );
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(pill, 8.0, 8.0);
            NSColor::labelColor()
                .colorWithAlphaComponent(0.085)
                .setFill();
            path.fill();
        }
    }
);

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
        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn row_view(&self, _table: &NSTableView, _row: isize) -> Option<Retained<NSTableRowView>> {
            let this = SidebarRowView::alloc(self.mtm()).set_ivars(());
            let row_view: Retained<SidebarRowView> = unsafe { msg_send![super(this), init] };
            Some(Retained::into_super(row_view))
        }

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
    fn cell_view(&self, _table: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let section = Section::from_index(row)?;
        let mtm = self.mtm();
        let cell = NSTableCellView::initWithFrame(
            NSTableCellView::alloc(mtm),
            rect(0., 0., 196., ROW_HEIGHT),
        );
        cell.setIdentifier(Some(&raw_text(CELL_ID)));
        let badge = SquircleIcon::new(mtm, section.symbol(), section.badge_tint(), 22.0, false);
        let title = crate::components::label(
            mtm,
            section.title(),
            typography::BODY,
            unsafe { NSFontWeightMedium },
            false,
            false,
        );
        title.setTranslatesAutoresizingMaskIntoConstraints(false);
        cell.addSubview(&badge);
        cell.addSubview(&title);
        for c in [
            badge
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&cell.leadingAnchor(), SIDEBAR_COLUMN_INSET),
            badge
                .centerYAnchor()
                .constraintEqualToAnchor(&cell.centerYAnchor()),
            title
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&badge.trailingAnchor(), 10.0),
            title
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&cell.trailingAnchor(), -14.0),
            title
                .centerYAnchor()
                .constraintEqualToAnchor(&cell.centerYAnchor()),
        ] {
            c.setActive(true);
        }
        Some(Retained::into_super(cell))
    }
}

pub(crate) struct Sidebar {
    pub controller: Retained<NSViewController>,
    table: Retained<NSTableView>,
    source: Retained<SidebarSource>,
    hero_icon: Retained<SquircleIcon>,
    hero_status: Retained<NSTextField>,
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
            left: 0.,
            bottom: spacing::MD,
            right: 0.,
        });
        container.setTranslatesAutoresizingMaskIntoConstraints(false);

        // Top status hero (mirrors macOS Settings top identity/service row).
        let hero_row = hstack(mtm, 10.0);
        hero_row.setEdgeInsets(NSEdgeInsets {
            top: 4.,
            left: SIDEBAR_COLUMN_INSET,
            bottom: 4.,
            right: 14.,
        });
        let hero_icon = SquircleIcon::new(mtm, "fanblades.fill", BadgeTint::Blue, 34.0, true);
        let hero_words = vstack(mtm, 1.0);
        let brand = crate::components::label(
            mtm,
            "Fan Control",
            typography::BODY,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        );
        let hero_status = crate::components::label(
            mtm,
            "正在检测…",
            typography::SMALL,
            unsafe { NSFontWeightRegular },
            true,
            false,
        );
        hero_status.setFont(Some(&theme::rounded_digits(typography::SMALL, unsafe {
            NSFontWeightRegular
        })));
        hero_status.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        hero_words.addArrangedSubview(&brand);
        hero_words.addArrangedSubview(&hero_status);
        hero_row.addArrangedSubview(&hero_icon);
        hero_row.addArrangedSubview(&hero_words);
        container.addArrangedSubview(&hero_row);
        container.setCustomSpacing_afterView(14., &hero_row);

        // Native table with custom Settings-style pill selection for keyboard + VoiceOver navigation.
        let scroll = NSScrollView::new(mtm);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setHasVerticalScroller(false);
        scroll.setDrawsBackground(false);
        let table = NSTableView::new(mtm);
        table.setHeaderView(None);
        table.setRowHeight(ROW_HEIGHT);
        table.setIntercellSpacing(NSSize::new(0., 3.));
        table.setStyle(NSTableViewStyle::Plain);
        table.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::Regular);
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
            .constraintEqualToAnchor(&container.widthAnchor())
            .setActive(true);
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(ROW_HEIGHT * 5. + 24.)
            .setActive(true);

        let vspacer = NSView::new(mtm);
        vspacer
            .setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Vertical);
        vspacer.setContentCompressionResistancePriority_forOrientation(
            1.0,
            NSLayoutConstraintOrientation::Vertical,
        );
        container.addArrangedSubview(&vspacer);

        // Quiet bottom safety action (no heavy card clutter).
        let reset = unsafe {
            NSButton::buttonWithTitle_target_action(
                &text("恢复系统自动"),
                Some(target),
                Some(sel!(reset:)),
                mtm,
            )
        };
        reset.setBordered(false);
        reset.setFont(Some(&NSFont::systemFontOfSize_weight(
            typography::CAPTION,
            unsafe { NSFontWeightMedium },
        )));
        reset.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
        if let Some(icon) = theme::symbol_image("arrow.counterclockwise", None, 11.5) {
            reset.setImage(Some(&icon));
            reset.setImagePosition(NSCellImagePosition::ImageLeading);
        }
        let footer = hstack(mtm, spacing::XS);
        footer.setEdgeInsets(NSEdgeInsets {
            top: 4.,
            left: SIDEBAR_COLUMN_INSET,
            bottom: 4.,
            right: 14.,
        });
        footer.addArrangedSubview(&reset);
        container.addArrangedSubview(&footer);

        material.addSubview(&container);
        pin_edges(&container, &material);

        let controller = NSViewController::new(mtm);
        controller.setView(&material);

        let sidebar = Self {
            controller,
            table,
            source,
            hero_icon,
            hero_status,
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
        self.hero_icon.set_tint(match view.notice.tone {
            Tone::Good => BadgeTint::Blue,
            Tone::Info => BadgeTint::Purple,
            Tone::Warning | Tone::Danger => BadgeTint::Orange,
        });
        let state_word = crate::i18n::translate(match view.notice.tone {
            Tone::Good => match view.plan {
                Plan::System => "系统默认",
                Plan::Smart => "智能散热",
                Plan::Custom => "自定义",
                Plan::Mixed => "分别设置",
            },
            Tone::Info => "提示",
            Tone::Warning => "需要注意",
            Tone::Danger => "温度偏高",
        });
        let summary = match view.temperature {
            Some(temp) => format!("{state_word} · {temp:.0}°C"),
            None => state_word,
        };
        self.hero_status.setStringValue(&raw_text(&summary));
        self.reset.setEnabled(view.controls_enabled);
    }
}
