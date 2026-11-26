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

pub(crate) const PAGE_INSET: f64 = 28.;
const ROW_INSET: f64 = 16.;
pub(crate) const BODY: f64 = typography::BODY;
pub(crate) const CAPTION: f64 = typography::SMALL;
pub(crate) const SECTION_GAP: f64 = 20.;

/// Resizable page container for the modern split-view detail pane.
pub(crate) fn fluid_page(mtm: MainThreadMarker) -> Retained<NSStackView> {
    let page = vstack(mtm, spacing::SM);
    page.setTranslatesAutoresizingMaskIntoConstraints(false);
    page.setEdgeInsets(NSEdgeInsets {
        top: spacing::MD,
        left: PAGE_INSET,
        bottom: spacing::XXL,
        right: PAGE_INSET,
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

/// Indented section header matching macOS Settings ("Appearance", "Toolbar", ...).
pub(crate) fn settings_section_title(
    mtm: MainThreadMarker,
    title: &str,
    top_padding: f64,
) -> Retained<NSStackView> {
    let row = crate::components::hstack(mtm, 0.);
    row.setEdgeInsets(NSEdgeInsets {
        top: top_padding,
        left: 6.,
        bottom: 2.,
        right: 6.,
    });
    row.addArrangedSubview(&section_title(mtm, title));
    row
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
    let content = vstack(mtm, 11.);
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
        NSSize::new(ROW_INSET, 11.),
        SurfaceStyle::Card,
    )
}

/// Clean single-line macOS Settings row with comfortable height and no subtitle clutter.
pub(crate) fn settings_row(
    mtm: MainThreadMarker,
    title: &str,
    accessories: &[&NSView],
) -> (Retained<NSStackView>, Retained<NSTextField>) {
    let title_label = crate::components::label(
        mtm,
        title,
        BODY,
        unsafe { NSFontWeightRegular },
        false,
        false,
    );
    let view = crate::components::hstack(mtm, spacing::SM);
    view.heightAnchor()
        .constraintGreaterThanOrEqualToConstant(24.)
        .setActive(true);
    view.addArrangedSubview(&title_label);
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
    (view, title_label)
}

pub(crate) struct Row {
    pub view: Retained<NSStackView>,
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
    Row { view, detail }
}

pub(crate) fn set_detail(row: &Row, value: &str) {
    row.detail.setStringValue(&crate::ui::text(value));
    row.detail.setHidden(value.is_empty());
}
