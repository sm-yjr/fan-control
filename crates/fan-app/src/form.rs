//! Grouped page layout in the style of System Settings: a section title over a
//! layered rounded group of fluid rows for the resizable detail pane.
use crate::{
    components::{flex_spacer, hairline, surface_box, vstack, SurfaceView},
    popover::styled_label,
    theme::{spacing, typography, SurfaceStyle},
};
use objc2::rc::Retained;
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSSize};

const ROW_INSET: f64 = 14.;
pub(crate) const BODY: f64 = typography::BODY;
pub(crate) const CAPTION: f64 = typography::SMALL;
pub(crate) const SECTION_GAP: f64 = 18.;

/// Resizable page container for the modern split-view detail pane.
pub(crate) fn fluid_page(mtm: MainThreadMarker) -> Retained<NSStackView> {
    let page = vstack(mtm, spacing::SM);
    page.setTranslatesAutoresizingMaskIntoConstraints(false);
    page.setEdgeInsets(NSEdgeInsets {
        top: spacing::XL,
        left: spacing::XXL,
        bottom: spacing::XXL,
        right: spacing::XXL,
    });
    page
}

pub(crate) fn section_title(mtm: MainThreadMarker, title: &str) -> Retained<NSTextField> {
    let label = styled_label(mtm, title, BODY, true, false, None);
    label.setFont(Some(&NSFont::systemFontOfSize_weight(BODY, unsafe {
        NSFontWeightSemibold
    })));
    label
}

pub(crate) fn fluid_caption(mtm: MainThreadMarker, value: &str) -> Retained<NSTextField> {
    crate::components::label(
        mtm,
        value,
        CAPTION,
        unsafe { NSFontWeightRegular },
        true,
        true,
    )
}

/// Fluid-width card group whose rows and hairlines expand with the detail pane.
pub(crate) fn fluid_group(mtm: MainThreadMarker, rows: &[&NSView]) -> Retained<SurfaceView> {
    let content = vstack(mtm, 10.);
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            let line = hairline(mtm);
            content.addArrangedSubview(&line);
            line.widthAnchor()
                .constraintEqualToAnchor(&content.widthAnchor())
                .setActive(true);
        }
        content.addArrangedSubview(row);
        row.widthAnchor()
            .constraintEqualToAnchor(&content.widthAnchor())
            .setActive(true);
    }
    surface_box(
        mtm,
        &content,
        NSSize::new(ROW_INSET, 12.),
        SurfaceStyle::Card,
    )
}

pub(crate) struct Row {
    pub view: Retained<NSStackView>,
    pub title: Retained<NSTextField>,
    pub detail: Retained<NSTextField>,
}

/// Resizable row where the text stack yields width to trailing accessories naturally.
pub(crate) fn fluid_row(
    mtm: MainThreadMarker,
    title: &str,
    detail: &str,
    accessories: &[&NSView],
) -> Row {
    let words = vstack(mtm, 2.);
    let title = crate::components::label(
        mtm,
        title,
        BODY,
        unsafe { NSFontWeightRegular },
        false,
        true,
    );
    let detail = crate::components::label(
        mtm,
        detail,
        CAPTION,
        unsafe { NSFontWeightRegular },
        true,
        true,
    );
    detail.setHidden(detail.stringValue().length() == 0);
    words.addArrangedSubview(&title);
    words.addArrangedSubview(&detail);
    words.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityDefaultLow,
        NSLayoutConstraintOrientation::Horizontal,
    );
    let view = crate::components::hstack(mtm, spacing::MD);
    view.addArrangedSubview(&words);
    view.addArrangedSubview(&flex_spacer(mtm));
    for accessory in accessories {
        accessory.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        accessory.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        view.addArrangedSubview(accessory);
    }
    Row {
        view,
        title,
        detail,
    }
}

pub(crate) fn set_detail(row: &Row, value: &str) {
    row.detail.setStringValue(&crate::ui::text(value));
    row.detail.setHidden(value.is_empty());
}
