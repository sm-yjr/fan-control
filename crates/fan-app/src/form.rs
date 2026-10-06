//! Grouped page layout in the style of System Settings: a section title over a
//! rounded group of rows, each row a title, an optional explanation and one
//! control on the trailing edge.
use crate::popover::{stack, styled_label, tinted_box, TintedView};
use objc2::rc::Retained;
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSSize};

pub(crate) const WIDTH: f64 = crate::ui::tokens::WIDTH;
pub(crate) const INSET: f64 = 24.;
pub(crate) const CONTENT: f64 = WIDTH - INSET * 2.;
const ROW_INSET: f64 = 14.;
/// Width available to a row's own content inside a group.
pub(crate) const ROW: f64 = CONTENT - ROW_INSET * 2.;
pub(crate) const BODY: f64 = 13.;
pub(crate) const CAPTION: f64 = 11.;
pub(crate) const SECTION_GAP: f64 = 18.;

pub(crate) fn page(mtm: MainThreadMarker) -> Retained<NSStackView> {
    let page = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 8.);
    page.setEdgeInsets(NSEdgeInsets {
        top: 20.,
        left: INSET,
        bottom: 22.,
        right: INSET,
    });
    page.widthAnchor()
        .constraintEqualToConstant(WIDTH)
        .setActive(true);
    page
}

/// A view controller sized to its page so the tab window fits the content.
pub(crate) fn controller(mtm: MainThreadMarker, view: &NSView) -> Retained<NSViewController> {
    let controller = NSViewController::new(mtm);
    controller.setView(view);
    fit(&controller, view);
    controller
}

/// Sizes the page to its natural height. The tab controller resizes the
/// window from frame-based child views, so the page keeps autoresizing.
pub(crate) fn fit(controller: &NSViewController, view: &NSView) {
    view.setTranslatesAutoresizingMaskIntoConstraints(true);
    view.layoutSubtreeIfNeeded();
    let size = view.fittingSize();
    view.setFrameSize(size);
    controller.setPreferredContentSize(size);
}

/// Refits a page whose content grew or shrank, such as a fan switching to a
/// fixed speed and showing its slider.
pub(crate) fn refit(controller: &NSViewController, view: &NSView) {
    let size = view.fittingSize();
    if (size.height - view.frame().size.height).abs() > 0.5 {
        fit(controller, view);
    }
}

pub(crate) fn spacer(mtm: MainThreadMarker) -> Retained<NSView> {
    let spacer = NSView::new(mtm);
    spacer.setContentHuggingPriority_forOrientation(1., NSLayoutConstraintOrientation::Horizontal);
    spacer
}

pub(crate) fn section_title(mtm: MainThreadMarker, title: &str) -> Retained<NSTextField> {
    let label = styled_label(mtm, title, BODY, true, false, None);
    label.setFont(Some(&NSFont::boldSystemFontOfSize(BODY)));
    label
}

pub(crate) fn caption(mtm: MainThreadMarker, value: &str, width: f64) -> Retained<NSTextField> {
    styled_label(mtm, value, CAPTION, false, true, Some(width))
}

pub(crate) fn separator(mtm: MainThreadMarker, width: f64) -> Retained<NSBox> {
    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    line.widthAnchor()
        .constraintEqualToConstant(width)
        .setActive(true);
    line
}

/// A rounded group; rows are separated by hairlines.
pub(crate) fn group(mtm: MainThreadMarker, rows: &[&NSView]) -> Retained<TintedView> {
    let content = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 10.);
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            content.addArrangedSubview(&separator(mtm, ROW));
        }
        content.addArrangedSubview(row);
    }
    let group = tinted_box(mtm, &content, NSSize::new(ROW_INSET, 11.));
    group.set_fill(&NSColor::quaternarySystemFillColor());
    group
        .widthAnchor()
        .constraintEqualToConstant(CONTENT)
        .setActive(true);
    group
}

pub(crate) struct Row {
    pub view: Retained<NSStackView>,
    pub title: Retained<NSTextField>,
    pub detail: Retained<NSTextField>,
}

/// Title and explanation on the leading edge, `accessory` on the trailing edge.
pub(crate) fn row(
    mtm: MainThreadMarker,
    title: &str,
    detail: &str,
    accessories: &[&NSView],
) -> Row {
    let accessory_width: f64 = accessories
        .iter()
        .map(|view| view.fittingSize().width + 8.)
        .sum();
    let words_width = (ROW - accessory_width - 12.).max(180.);
    let words = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 2.);
    let title = styled_label(mtm, title, BODY, false, false, Some(words_width));
    let detail = styled_label(mtm, detail, CAPTION, false, true, Some(words_width));
    detail.setHidden(detail.stringValue().length() == 0);
    words.addArrangedSubview(&title);
    words.addArrangedSubview(&detail);
    let view = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.);
    view.addArrangedSubview(&words);
    view.addArrangedSubview(&spacer(mtm));
    for accessory in accessories {
        view.addArrangedSubview(accessory);
    }
    view.widthAnchor()
        .constraintEqualToConstant(ROW)
        .setActive(true);
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
