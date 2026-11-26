//! Resizable macOS split-view main window hosting a frosted Settings-style sidebar
//! and airy detail pane that never jumps in height when switching sections.
use crate::{
    components::{flex_spacer, hstack, pin_edges},
    presenter::Presentation,
    sidebar::{Section, Sidebar},
    theme::{layout, spacing, typography},
    ui::{raw_text, rect, text},
};
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyObject, MainThreadOnly, Message};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSSize};
use std::cell::Cell;

define_class!(
    /// Semantic window-background canvas for the detail pane so Light and Dark modes
    /// paint an opaque native surface consistently in live and offscreen rendering.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct DetailCanvasView;
    impl DetailCanvasView {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: objc2_foundation::NSRect) {
            NSColor::windowBackgroundColor().setFill();
            NSBezierPath::fillRect(self.bounds());
        }
    }
);

impl DetailCanvasView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }
}

define_class!(
    /// Top-aligned document container for NSScrollView so scrollable pages
    /// always anchor to the top edge cleanly.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct FlippedDocument;
    impl FlippedDocument {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedDocument {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }
}

/// Wraps a fluid Auto Layout page inside a top-anchored vertical scroll view.
pub(crate) fn scrollable_host(mtm: MainThreadMarker, page: &NSView) -> Retained<NSScrollView> {
    let scroll = NSScrollView::new(mtm);
    scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
    scroll.setHasVerticalScroller(true);
    scroll.setHasHorizontalScroller(false);
    scroll.setAutohidesScrollers(true);
    scroll.setDrawsBackground(false);

    let document = FlippedDocument::new(mtm);
    page.setTranslatesAutoresizingMaskIntoConstraints(false);
    document.addSubview(page);
    for c in [
        page.topAnchor()
            .constraintEqualToAnchor(&document.topAnchor()),
        page.leadingAnchor()
            .constraintEqualToAnchor(&document.leadingAnchor()),
        page.trailingAnchor()
            .constraintEqualToAnchor(&document.trailingAnchor()),
        document
            .bottomAnchor()
            .constraintGreaterThanOrEqualToAnchor(&page.bottomAnchor()),
    ] {
        c.setActive(true);
    }

    scroll.setDocumentView(Some(&document));
    let clip = scroll.contentView();
    document
        .widthAnchor()
        .constraintEqualToAnchor(&clip.widthAnchor())
        .setActive(true);
    scroll
}

pub(crate) struct MainShell {
    pub window: Retained<NSWindow>,
    pub sidebar: Sidebar,
    _split: Retained<NSSplitViewController>,
    header_title: Retained<NSTextField>,
    stage: Retained<NSView>,
    pages: [Retained<NSView>; 5],
    active: Cell<Section>,
}

impl MainShell {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mtm: MainThreadMarker,
        target: &AnyObject,
        demo: bool,
        overview: &NSView,
        fans: &NSView,
        smart_placeholder: &NSView,
        details: &NSView,
        settings: &NSView,
    ) -> Self {
        let sidebar = Sidebar::new(mtm, target);

        let detail_root = DetailCanvasView::new(mtm);

        // Airy macOS Settings header bar (no harsh full-width rule).
        let header_bar = hstack(mtm, spacing::MD);
        header_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
        header_bar.setEdgeInsets(NSEdgeInsets {
            top: 14.,
            left: 28.,
            bottom: 4.,
            right: 28.,
        });
        let header_title = crate::components::label(
            mtm,
            Section::Overview.title(),
            typography::TITLE,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        );
        header_bar.addArrangedSubview(&header_title);
        header_bar.addArrangedSubview(&flex_spacer(mtm));

        let stage = NSView::new(mtm);
        stage.setTranslatesAutoresizingMaskIntoConstraints(false);

        detail_root.addSubview(&header_bar);
        detail_root.addSubview(&stage);

        for c in [
            header_bar
                .topAnchor()
                .constraintEqualToAnchor(&detail_root.topAnchor()),
            header_bar
                .leadingAnchor()
                .constraintEqualToAnchor(&detail_root.leadingAnchor()),
            header_bar
                .trailingAnchor()
                .constraintEqualToAnchor(&detail_root.trailingAnchor()),
            header_bar.heightAnchor().constraintEqualToConstant(48.0),
            stage
                .topAnchor()
                .constraintEqualToAnchor(&header_bar.bottomAnchor()),
            stage
                .leadingAnchor()
                .constraintEqualToAnchor(&detail_root.leadingAnchor()),
            stage
                .trailingAnchor()
                .constraintEqualToAnchor(&detail_root.trailingAnchor()),
            stage
                .bottomAnchor()
                .constraintEqualToAnchor(&detail_root.bottomAnchor()),
        ] {
            c.setActive(true);
        }

        let pages: [Retained<NSView>; 5] = [
            Retained::into_super(scrollable_host(mtm, overview)),
            Retained::into_super(scrollable_host(mtm, fans)),
            Retained::into_super(scrollable_host(mtm, smart_placeholder)),
            details.retain(),
            Retained::into_super(scrollable_host(mtm, settings)),
        ];
        for (idx, page) in pages.iter().enumerate() {
            stage.addSubview(page);
            pin_edges(page, &stage);
            page.setHidden(idx != 0);
        }

        let detail_vc = NSViewController::new(mtm);
        detail_vc.setView(&detail_root);

        let split = NSSplitViewController::new(mtm);
        let sidebar_item = NSSplitViewItem::sidebarWithViewController(&sidebar.controller);
        sidebar_item.setMinimumThickness(196.0);
        sidebar_item.setMaximumThickness(240.0);
        sidebar_item.setCanCollapse(false);
        let detail_item = NSSplitViewItem::splitViewItemWithViewController(&detail_vc);
        detail_item.setMinimumThickness(layout::WINDOW_MIN_WIDTH - layout::SIDEBAR_WIDTH);
        split.addSplitViewItem(&sidebar_item);
        split.addSplitViewItem(&detail_item);

        let visible = NSScreen::mainScreen(mtm).map(|s| s.visibleFrame());
        let default_w = visible
            .map(|f| {
                layout::WINDOW_DEFAULT_WIDTH.min((f.size.width - 40.).max(layout::WINDOW_MIN_WIDTH))
            })
            .unwrap_or(layout::WINDOW_DEFAULT_WIDTH);
        let default_h = visible
            .map(|f| {
                layout::WINDOW_DEFAULT_HEIGHT
                    .min((f.size.height - 60.).max(layout::WINDOW_MIN_HEIGHT))
            })
            .unwrap_or(layout::WINDOW_DEFAULT_HEIGHT);

        let window = NSWindow::windowWithContentViewController(&split);
        window.setStyleMask(
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable
                | NSWindowStyleMask::FullSizeContentView,
        );
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitle(&text("Fan Control"));
        window.setMinSize(NSSize::new(
            layout::WINDOW_MIN_WIDTH,
            layout::WINDOW_MIN_HEIGHT,
        ));
        window.setContentSize(NSSize::new(default_w, default_h));
        window.setFrame_display(rect(0., 0., default_w, default_h), false);
        window.center();
        if !demo {
            let _ = window.setFrameAutosaveName(&raw_text("FanControlMainWindow"));
        }
        unsafe {
            window.setReleasedWhenClosed(false);
        }

        Self {
            window,
            sidebar,
            _split: split,
            header_title,
            stage,
            pages,
            active: Cell::new(Section::Overview),
        }
    }

    pub fn select(&self, section: Section) {
        self.active.set(section);
        self.sidebar.select(section);
        self.header_title.setStringValue(&text(section.title()));
        let target_idx = section.index() as usize;
        for (idx, page) in self.pages.iter().enumerate() {
            page.setHidden(idx != target_idx);
        }
    }

    #[allow(dead_code)]
    pub fn active_section(&self) -> Section {
        self.active.get()
    }

    pub fn set_smart_view(&mut self, mtm: MainThreadMarker, smart_view: &NSView) {
        let idx = Section::Smart.index() as usize;
        let was_hidden = self.pages[idx].isHidden();
        self.pages[idx].removeFromSuperview();
        let next: Retained<NSView> = Retained::into_super(scrollable_host(mtm, smart_view));
        self.stage.addSubview(&next);
        pin_edges(&next, &self.stage);
        next.setHidden(was_hidden);
        self.pages[idx] = next;
    }

    pub fn refresh(&self, view: &Presentation) {
        self.sidebar.refresh(view);
    }

    pub fn show(&self, section: Section) {
        self.select(section);
        self.window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        NSApplication::sharedApplication(self.window.mtm()).activateIgnoringOtherApps(true);
    }
}
