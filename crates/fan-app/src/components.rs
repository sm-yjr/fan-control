//! Reusable native AppKit building blocks backed by `theme` semantic tokens.
use crate::{
    presenter::Tone,
    theme::{self, layout, spacing, typography, SurfaceStyle},
    ui::{raw_text, text},
};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, DefinedClass, MainThreadOnly, Message,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSRect, NSSize};
use std::cell::Cell;

pub(crate) struct SurfaceIvars {
    style: Cell<SurfaceStyle>,
}

define_class!(
    /// Auto Layout-friendly rounded surface with dynamic semantic fill and 0.5pt border.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = SurfaceIvars]
    pub(crate) struct SurfaceView;
    impl SurfaceView {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let style = self.ivars().style.get();
            let (fill, border) = style.colors();
            if fill.is_none() && border.is_none() {
                return;
            }
            let bounds = self.bounds();
            if bounds.size.width <= 0. || bounds.size.height <= 0. {
                return;
            }
            let inset = if border.is_some() { layout::HAIRLINE / 2.0 } else { 0.0 };
            let draw_rect = NSRect::new(
                objc2_foundation::NSPoint::new(bounds.origin.x + inset, bounds.origin.y + inset),
                NSSize::new(
                    (bounds.size.width - inset * 2.0).max(0.0),
                    (bounds.size.height - inset * 2.0).max(0.0),
                ),
            );
            let max_r = (draw_rect.size.width.min(draw_rect.size.height) / 2.0).max(0.0);
            let r = style.corner_radius().min(max_r);
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(draw_rect, r, r);
            if let Some(fill) = fill {
                fill.setFill();
                path.fill();
            }
            if let Some(border) = border {
                border.setStroke();
                path.setLineWidth(layout::HAIRLINE);
                path.stroke();
            }
        }
    }
);

impl SurfaceView {
    pub fn new(mtm: MainThreadMarker, style: SurfaceStyle) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SurfaceIvars {
            style: Cell::new(style),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn set_style(&self, style: SurfaceStyle) {
        if self.ivars().style.get() != style {
            self.ivars().style.set(style);
            self.setNeedsDisplay(true);
        }
    }
}

pub(crate) fn surface_box(
    mtm: MainThreadMarker,
    content: &NSView,
    margin: NSSize,
    style: SurfaceStyle,
) -> Retained<SurfaceView> {
    let tile = SurfaceView::new(mtm, style);
    tile.setTranslatesAutoresizingMaskIntoConstraints(false);
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    tile.addSubview(content);
    for constraint in [
        content
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&tile.leadingAnchor(), margin.width),
        tile.trailingAnchor()
            .constraintEqualToAnchor_constant(&content.trailingAnchor(), margin.width),
        content
            .topAnchor()
            .constraintEqualToAnchor_constant(&tile.topAnchor(), margin.height),
        tile.bottomAnchor()
            .constraintEqualToAnchor_constant(&content.bottomAnchor(), margin.height),
    ] {
        constraint.setActive(true);
    }
    tile.setContentHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultHigh,
        NSLayoutConstraintOrientation::Vertical,
    );
    tile
}

pub(crate) fn vstack(mtm: MainThreadMarker, gap: f64) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::new(), mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setSpacing(gap);
    stack.setAlignment(NSLayoutAttribute::Leading);
    stack.setDetachesHiddenViews(true);
    stack.setHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultHigh,
        NSLayoutConstraintOrientation::Vertical,
    );
    stack
}

pub(crate) fn hstack(mtm: MainThreadMarker, gap: f64) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::new(), mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setSpacing(gap);
    stack.setAlignment(NSLayoutAttribute::CenterY);
    stack.setDetachesHiddenViews(true);
    stack.setHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultHigh,
        NSLayoutConstraintOrientation::Vertical,
    );
    stack
}

pub(crate) fn flex_spacer(mtm: MainThreadMarker) -> Retained<NSView> {
    let spacer = NSView::new(mtm);
    spacer.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    spacer.setContentCompressionResistancePriority_forOrientation(
        1.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    spacer
}

pub(crate) fn hairline(mtm: MainThreadMarker) -> Retained<NSBox> {
    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    line.setTranslatesAutoresizingMaskIntoConstraints(false);
    line
}

pub(crate) fn label(
    mtm: MainThreadMarker,
    value: &str,
    size: f64,
    weight: NSFontWeight,
    secondary: bool,
    wrapping: bool,
) -> Retained<NSTextField> {
    let field = if wrapping {
        NSTextField::wrappingLabelWithString(&text(value), mtm)
    } else {
        NSTextField::labelWithString(&text(value), mtm)
    };
    field.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
    if secondary {
        field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
    field.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityRequired,
        NSLayoutConstraintOrientation::Vertical,
    );
    if wrapping {
        field.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
    }
    field.setSelectable(false);
    field
}

/// Semantic pill with an SF Symbol icon and concise status text so state is
/// never conveyed by color alone.
pub(crate) struct StatusBadge {
    pub view: Retained<SurfaceView>,
    icon: Retained<NSImageView>,
    pub label: Retained<NSTextField>,
}

impl StatusBadge {
    pub fn new(mtm: MainThreadMarker, initial: &str, tone: Tone) -> Self {
        let row = hstack(mtm, spacing::XS);
        let icon = NSImageView::new(mtm);
        icon.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon.widthAnchor()
            .constraintEqualToConstant(12.0)
            .setActive(true);
        icon.heightAnchor()
            .constraintEqualToConstant(12.0)
            .setActive(true);
        let title = label(
            mtm,
            initial,
            typography::SMALL,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        );
        row.addArrangedSubview(&icon);
        row.addArrangedSubview(&title);
        let view = surface_box(
            mtm,
            &row,
            NSSize::new(8.0, 3.0),
            SurfaceStyle::TonePill(tone),
        );
        let badge = Self {
            view,
            icon,
            label: title,
        };
        badge.set(tone, initial);
        badge
    }

    pub fn set(&self, tone: Tone, title: &str) {
        let color = theme::tone_color(tone);
        self.view.set_style(SurfaceStyle::TonePill(tone));
        if let Some(img) = theme::symbol_image(theme::tone_symbol(tone), None, 11.0) {
            self.icon.setImage(Some(&img));
        }
        self.icon.setContentTintColor(Some(&color));
        self.label.setStringValue(&text(title));
        self.label.setTextColor(Some(&color));
    }
}

/// Standard metric tile: title + rounded monospaced reading + optional subtitle & bar.
pub(crate) struct MetricTile {
    pub view: Retained<SurfaceView>,
    pub value: Retained<NSTextField>,
    pub subtitle: Retained<NSTextField>,
    pub bar: Option<Retained<crate::gauge::Bar>>,
}

impl MetricTile {
    pub fn new(mtm: MainThreadMarker, title: &str, with_bar: bool) -> Self {
        let content = vstack(mtm, spacing::XS);
        content.addArrangedSubview(&label(
            mtm,
            title,
            typography::SMALL,
            unsafe { NSFontWeightMedium },
            true,
            false,
        ));
        let value = label(
            mtm,
            "—",
            typography::METRIC_LARGE,
            unsafe { NSFontWeightSemibold },
            false,
            false,
        );
        value.setFont(Some(&theme::rounded_digits(
            typography::METRIC_LARGE,
            unsafe { NSFontWeightSemibold },
        )));
        value.setAccessibilityLabel(Some(&text(title)));
        content.addArrangedSubview(&value);

        let subtitle = label(
            mtm,
            "",
            typography::SMALL,
            unsafe { NSFontWeightRegular },
            true,
            false,
        );
        subtitle.setHidden(true);
        content.addArrangedSubview(&subtitle);

        let bar = if with_bar {
            let bar = crate::gauge::Bar::new_flexible(mtm, 5.0);
            content.addArrangedSubview(&bar);
            bar.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
            Some(bar)
        } else {
            None
        };

        let view = surface_box(mtm, &content, NSSize::new(14.0, 12.0), SurfaceStyle::Card);
        Self {
            view,
            value,
            subtitle,
            bar,
        }
    }

    pub fn set_subtitle(&self, value: &str) {
        self.subtitle.setStringValue(&text(value));
        self.subtitle.setHidden(value.is_empty());
    }
}

/// Native disclosure header + collapsible content wrapper.
#[allow(dead_code)]
pub(crate) struct DisclosureSection {
    pub view: Retained<NSStackView>,
    pub toggle: Retained<NSButton>,
    pub summary: Retained<NSTextField>,
    body: Retained<NSView>,
    expanded: Cell<bool>,
}

#[allow(dead_code)]
impl DisclosureSection {
    pub fn new(
        mtm: MainThreadMarker,
        title: &str,
        summary_text: &str,
        body: &NSView,
        initially_expanded: bool,
        target: &AnyObject,
        action: objc2::runtime::Sel,
    ) -> Self {
        let view = vstack(mtm, spacing::SM);
        let header = hstack(mtm, spacing::SM);
        let toggle = unsafe {
            NSButton::buttonWithTitle_target_action(&raw_text(""), Some(target), Some(action), mtm)
        };
        toggle.setBezelStyle(NSBezelStyle::Disclosure);
        toggle.setButtonType(NSButtonType::OnOff);
        toggle.setState(if initially_expanded {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        toggle.setAccessibilityLabel(Some(&text(title)));

        let title_btn = unsafe {
            NSButton::buttonWithTitle_target_action(&text(title), Some(target), Some(action), mtm)
        };
        title_btn.setBordered(false);
        title_btn.setFont(Some(&NSFont::systemFontOfSize_weight(
            typography::HEADLINE,
            unsafe { NSFontWeightSemibold },
        )));
        title_btn.setContentTintColor(Some(&NSColor::labelColor()));

        let summary = label(
            mtm,
            summary_text,
            typography::SMALL,
            unsafe { NSFontWeightRegular },
            true,
            false,
        );
        summary.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);

        header.addArrangedSubview(&toggle);
        header.addArrangedSubview(&title_btn);
        header.addArrangedSubview(&flex_spacer(mtm));
        header.addArrangedSubview(&summary);

        let header_card = surface_box(mtm, &header, NSSize::new(12.0, 9.0), SurfaceStyle::Card);
        view.addArrangedSubview(&header_card);
        header_card
            .widthAnchor()
            .constraintEqualToAnchor(&view.widthAnchor())
            .setActive(true);

        body.setHidden(!initially_expanded);
        view.addArrangedSubview(body);
        body.widthAnchor()
            .constraintEqualToAnchor(&view.widthAnchor())
            .setActive(true);

        Self {
            view,
            toggle,
            summary,
            body: body.retain(),
            expanded: Cell::new(initially_expanded),
        }
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded.get()
    }

    pub fn set_expanded(&self, expanded: bool) {
        self.expanded.set(expanded);
        self.toggle.setState(if expanded {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        self.body.setHidden(!expanded);
    }

    pub fn flip(&self) -> bool {
        let next = !self.expanded.get();
        self.set_expanded(next);
        next
    }
}

pub(crate) fn pin_edges(child: &NSView, parent: &NSView) {
    child.setTranslatesAutoresizingMaskIntoConstraints(false);
    for constraint in [
        child
            .leadingAnchor()
            .constraintEqualToAnchor(&parent.leadingAnchor()),
        child
            .trailingAnchor()
            .constraintEqualToAnchor(&parent.trailingAnchor()),
        child
            .topAnchor()
            .constraintEqualToAnchor(&parent.topAnchor()),
        child
            .bottomAnchor()
            .constraintEqualToAnchor(&parent.bottomAnchor()),
    ] {
        constraint.setActive(true);
    }
}
