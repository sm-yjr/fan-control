//! Reusable Liquid Glass GPUI Kit primitives styled after modern macOS Tahoe System Settings.
use crate::gpui_ui::theme::{GlassTheme, IconTint};
use crate::presenter::Tone;
use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, rgba, svg, AnyElement, App, BoxShadow,
    Div, ElementId, FontWeight, Hsla, MouseButton, Pixels, Point, SharedString, Stateful, Window,
};
use gpui_component::{switch::Switch, Disableable};
use gpui_kit_assets::IconName;

/// Translates UI text through `crate::i18n::translate` and returns a `SharedString`.
pub fn t(value: &str) -> SharedString {
    SharedString::from(crate::i18n::translate(value))
}

/// Crisp vector Lucide SVG icon from GPUI Kit assets.
pub fn vector_icon(icon: IconName, size: f32, color: impl Into<Hsla>) -> impl IntoElement {
    svg()
        .path(icon.path())
        .flex_shrink_0()
        .size(px(size))
        .text_color(color.into())
}

/// 14px rounded Liquid Glass grouped card container with subtle border and top sheen.
pub fn glass_card(theme: &GlassTheme) -> Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .rounded(px(14.0))
        .bg(theme.card_bg)
        .border_1()
        .border_color(theme.card_border)
        .shadow(vec![BoxShadow {
            color: theme.card_sheen,
            offset: Point::new(px(0.0), px(1.0)),
            blur_radius: px(0.0),
            spread_radius: px(0.0),
            inset: true,
        }])
}

/// Elevated 10px rounded inset well for charts, gauges, and interactive curve preview.
pub fn inset_well(theme: &GlassTheme) -> Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .rounded(px(10.0))
        .bg(theme.well_bg)
        .border_1()
        .border_color(theme.well_border)
}

/// Whisper-thin 1px horizontal divider between rows inside a grouped card.
pub fn hairline(theme: &GlassTheme) -> Div {
    div().w_full().h(px(1.0)).bg(theme.hairline)
}

/// Indented bold section header ("外观与通用", "风扇控制服务", ...).
pub fn section_title(theme: &GlassTheme, title: &str) -> Div {
    div()
        .px(px(6.0))
        .pt(px(4.0))
        .pb(px(2.0))
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text_primary)
        .child(t(title))
}

/// Subtle `(i)` footnote caption placed beneath a grouped card.
pub fn info_footnote(theme: &GlassTheme, message: &str) -> Div {
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(6.0))
        .px(px(6.0))
        .pt(px(2.0))
        .child(
            div()
                .mt(px(1.5))
                .child(vector_icon(IconName::Info, 13.0, theme.text_secondary)),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(11.5))
                .line_height(px(16.0))
                .text_color(theme.text_secondary)
                .child(t(message)),
        )
}

/// Colorful macOS Settings gradient squircle or circular icon badge with vector Lucide SVG.
pub fn squircle_badge(icon: IconName, tint: IconTint, size: f32, circle: bool) -> Div {
    let (top, bottom) = GlassTheme::icon_gradient(tint);
    let radius = if circle {
        px(size / 2.0)
    } else {
        px((size * 0.27).max(5.5))
    };
    div()
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
        .rounded(radius)
        .bg(linear_gradient(
            180.0,
            linear_color_stop(top, 0.0),
            linear_color_stop(bottom, 1.0),
        ))
        .border_1()
        .border_color(rgba(0xFFFFFF2E))
        .shadow(vec![BoxShadow {
            color: rgba(0x0000001F).into(),
            offset: Point::new(px(0.0), px(1.0)),
            blur_radius: px(2.5),
            spread_radius: px(0.0),
            inset: false,
        }])
        .flex()
        .items_center()
        .justify_center()
        .child(vector_icon(icon, size * 0.54, Hsla::from(rgba(0xFFFFFFFF))))
}

/// Semantic tinted capsule pill with status icon and concise text.
pub fn status_badge(theme: &GlassTheme, tone: Tone, label: &str) -> Div {
    let (fg, bg) = theme.tone_colors(tone);
    let icon = match tone {
        Tone::Good => IconName::CircleCheck,
        Tone::Info => IconName::Info,
        Tone::Warning => IconName::Info,
        Tone::Danger => IconName::Flame,
    };
    div()
        .flex()
        .flex_row()
        .flex_shrink_0()
        .items_center()
        .gap(px(5.0))
        .px(px(9.0))
        .py(px(3.0))
        .rounded_full()
        .bg(bg)
        .border_1()
        .border_color(fg.opacity(0.24))
        .whitespace_nowrap()
        .child(vector_icon(icon, 13.0, fg))
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .whitespace_nowrap()
                .child(t(label)),
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Solid Apple blue capsule (`Sign In`, `启用…`, `应用`).
    Primary,
    /// Soft neutral glass capsule (`Check Now`, `检查更新…`, `全部交还`).
    Secondary,
    /// Soft rose destructive capsule (`Stop Server...`, `移除…`).
    DangerSoft,
    /// Clean blue inline text link (`What's new`, `重新检测`).
    Link,
}

/// Clickable Liquid Glass capsule button modeled directly on the Rex macOS Tahoe buttons.
pub fn pill_button(
    id: impl Into<ElementId>,
    label: &str,
    style: ButtonStyle,
    enabled: bool,
    theme: &GlassTheme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, hover_bg, fg, border) = match style {
        ButtonStyle::Primary => (
            theme.accent,
            theme.accent.opacity(0.88),
            rgba(0xFFFFFFFF).into(),
            rgba(0xFFFFFF24).into(),
        ),
        ButtonStyle::Secondary => (
            theme.capsule_bg,
            theme.capsule_hover,
            theme.text_primary,
            theme.capsule_border,
        ),
        ButtonStyle::DangerSoft => (
            theme.danger_pill_bg,
            theme.danger_pill_hover,
            theme.danger,
            theme.danger.opacity(0.18),
        ),
        ButtonStyle::Link => (
            rgba(0x00000000).into(),
            theme.accent_soft,
            theme.accent,
            rgba(0x00000000).into(),
        ),
    };

    let mut btn = div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .px(px(if style == ButtonStyle::Link {
            8.0
        } else {
            13.0
        }))
        .py(px(5.0))
        .rounded_full()
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(12.5))
        .whitespace_nowrap()
        .font_weight(
            if style == ButtonStyle::Primary || style == ButtonStyle::DangerSoft {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            },
        )
        .text_color(fg)
        .child(t(label));

    if enabled {
        btn = btn
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                on_click(window, cx);
            });
    } else {
        btn = btn.opacity(0.42);
    }
    btn
}

/// Iconic macOS Tahoe Liquid Glass `< | >` Back/Forward navigation capsule.
pub fn nav_capsule(
    theme: &GlassTheme,
    can_back: bool,
    can_fwd: bool,
    on_back: impl Fn(&mut Window, &mut App) + 'static,
    on_fwd: impl Fn(&mut Window, &mut App) + 'static,
) -> Div {
    let mut back = div()
        .id("nav-back")
        .w(px(30.0))
        .h(px(28.0))
        .rounded_l_full()
        .flex()
        .items_center()
        .justify_center()
        .child(vector_icon(
            IconName::ChevronLeft,
            14.0,
            if can_back {
                theme.text_primary
            } else {
                theme.text_muted
            },
        ));
    if can_back {
        let hover = theme.capsule_hover;
        back = back
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                on_back(window, cx);
            });
    }

    let mut fwd = div()
        .id("nav-forward")
        .w(px(30.0))
        .h(px(28.0))
        .rounded_r_full()
        .flex()
        .items_center()
        .justify_center()
        .child(vector_icon(
            IconName::ChevronRight,
            14.0,
            if can_fwd {
                theme.text_primary
            } else {
                theme.text_muted
            },
        ));
    if can_fwd {
        let hover = theme.capsule_hover;
        fwd = fwd
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                on_fwd(window, cx);
            });
    }

    div()
        .flex()
        .flex_row()
        .items_center()
        .rounded_full()
        .bg(theme.capsule_bg)
        .border_1()
        .border_color(theme.capsule_border)
        .shadow(vec![BoxShadow {
            color: theme.shadow,
            offset: Point::new(px(0.0), px(1.0)),
            blur_radius: px(4.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .child(back)
        .child(div().w(px(1.0)).h(px(14.0)).bg(theme.hairline))
        .child(fwd)
}

/// Spring-animated GPUI Kit Switch with VoiceOver accessibility support.
pub fn glass_toggle(
    id: impl Into<ElementId>,
    checked: bool,
    enabled: bool,
    theme: &GlassTheme,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Switch {
    Switch::new(id)
        .checked(checked)
        .disabled(!enabled)
        .color(theme.accent)
        .on_click(move |&next, window, cx| {
            on_toggle(next, window, cx);
        })
}

/// Full-pill Liquid Glass segmented control (`Comfortable | Compact`, `系统默认 | 智能散热 | 自定义`).
pub fn glass_segmented(
    id_prefix: &'static str,
    options: &[&'static str],
    selected: Option<usize>,
    enabled: bool,
    full_width: bool,
    theme: &GlassTheme,
    on_select: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    let mut track = div()
        .flex()
        .flex_row()
        .items_center()
        .p(px(2.5))
        .gap(px(2.0))
        .rounded_full()
        .bg(theme.segment_track)
        .border_1()
        .border_color(theme.card_border);
    if full_width {
        track = track.w_full();
    }

    for (idx, label) in options.iter().enumerate() {
        let is_active = selected == Some(idx);
        let cb = on_select.clone();
        let mut seg = div()
            .id((id_prefix, idx))
            .flex()
            .items_center()
            .justify_center()
            .px(px(if full_width { 8.0 } else { 11.0 }))
            .py(px(4.5))
            .rounded_full()
            .text_size(px(12.0))
            .whitespace_nowrap()
            .font_weight(if is_active {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .text_color(if is_active {
                theme.text_primary
            } else {
                theme.text_secondary
            })
            .child(t(label));

        if full_width {
            seg = seg.flex_1().min_w_0();
        } else {
            seg = seg.flex_shrink_0();
        }
        if is_active {
            seg = seg
                .bg(theme.segment_active)
                .border_1()
                .border_color(theme.segment_active_border)
                .shadow(vec![BoxShadow {
                    color: theme.shadow,
                    offset: Point::new(px(0.0), px(1.0)),
                    blur_radius: px(3.0),
                    spread_radius: px(0.0),
                    inset: false,
                }]);
        }
        if enabled {
            let hover = theme.sidebar_hover_pill;
            seg = seg
                .cursor_pointer()
                .when(!is_active, move |s| s.hover(move |h| h.bg(hover)))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cb(idx, window, cx);
                });
        } else {
            seg = seg.opacity(0.48);
        }
        track = track.child(seg);
    }
    track
}

/// Inline borderless macOS Settings pop-up trigger (`简体中文  ⌄`) with circular chevron badge.
pub fn inline_picker_trigger(
    id: impl Into<ElementId>,
    current_label: &str,
    enabled: bool,
    theme: &GlassTheme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let mut trigger = div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(7.0))
        .px(px(8.0))
        .py(px(3.0))
        .rounded_full()
        .text_size(px(13.0))
        .text_color(theme.text_primary)
        .child(t(current_label))
        .child(
            div()
                .w(px(19.0))
                .h(px(19.0))
                .rounded_full()
                .bg(theme.segment_track)
                .flex()
                .items_center()
                .justify_center()
                .child(vector_icon(
                    IconName::ChevronsUpDown,
                    11.0,
                    theme.text_secondary,
                )),
        );

    if enabled {
        let hover = theme.sidebar_hover_pill;
        trigger = trigger
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                on_click(window, cx);
            });
    } else {
        trigger = trigger.opacity(0.45);
    }
    trigger
}

/// Single-line or title+subtitle Settings row inside a `glass_card`.
pub fn settings_row(
    theme: &GlassTheme,
    title: &str,
    subtitle: Option<&str>,
    trailing: impl IntoElement,
) -> Div {
    let mut left = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_primary)
                .child(t(title)),
        );
    if let Some(sub) = subtitle {
        if !sub.is_empty() {
            left = left.child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.text_secondary)
                    .child(t(sub)),
            );
        }
    }

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(11.0))
        .min_h(px(44.0))
        .child(left)
        .child(
            div()
                .flex()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .gap(px(8.0))
                .child(trailing),
        )
}

/// Interactive chevron navigation row inside a `glass_card`.
pub fn chevron_row(
    id: impl Into<ElementId>,
    title: &str,
    trailing_hint: Option<&str>,
    theme: &GlassTheme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let hover = theme.sidebar_hover_pill;
    let mut right = div().flex().flex_row().items_center().gap(px(8.0));
    if let Some(hint) = trailing_hint {
        right = right.child(
            div()
                .text_size(px(12.5))
                .text_color(theme.text_secondary)
                .child(t(hint)),
        );
    }
    right = right.child(vector_icon(IconName::ChevronRight, 14.0, theme.text_muted));

    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .py(px(11.0))
        .min_h(px(44.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_click(window, cx);
        })
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_primary)
                .child(t(title)),
        )
        .child(right)
}

/// Discrete step bar / Tahoe slider rendered as clickable segments for crisp mouse precision.
/// Supports both 21-step cooling preference (`-10..=+10`) and smooth percentage (`0..=100`, step 5).
pub fn step_slider(
    id_prefix: &'static str,
    steps: &[i32],
    current: i32,
    center_mark: Option<i32>,
    enabled: bool,
    theme: &GlassTheme,
    on_select: impl Fn(i32, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    let total = steps.len().max(2);
    let current_idx = steps.iter().position(|v| *v == current).unwrap_or_else(|| {
        steps
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| (**v - current).abs())
            .map(|(i, _)| i)
            .unwrap_or(0)
    });
    let frac = current_idx as f32 / (total - 1) as f32;

    let mut hit_cells = div()
        .relative()
        .w_full()
        .h(px(24.0))
        .flex()
        .flex_row()
        .items_center();

    // Background full track + active accent fill + Tahoe pill thumb, inset by 7px so the
    // 14px thumb never clips outside the container at 0% or 100%.
    let track_layer = div()
        .absolute()
        .left(px(7.0))
        .right(px(7.0))
        .top_0()
        .bottom_0()
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(10.0))
                .h(px(4.0))
                .rounded_full()
                .bg(theme.slider_track)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(frac))
                        .rounded_full()
                        .bg(if enabled {
                            theme.accent
                        } else {
                            theme.text_muted
                        }),
                ),
        )
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(gpui::relative(frac))
                .ml(px(-7.0))
                .w(px(14.0))
                .h(px(18.0))
                .rounded_full()
                .bg(theme.toggle_thumb)
                .border_1()
                .border_color(rgba(0x00000018))
                .shadow(vec![BoxShadow {
                    color: rgba(0x0000002B).into(),
                    offset: Point::new(px(0.0), px(1.5)),
                    blur_radius: px(4.0),
                    spread_radius: px(0.0),
                    inset: false,
                }]),
        );

    hit_cells = hit_cells.child(track_layer);

    // Interactive transparent hit cells on TOP of track & thumb across every discrete step,
    // plus subtle tick dots below.
    let mut dots = div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(6.0));

    for (idx, &value) in steps.iter().enumerate() {
        let cb = on_select.clone();
        let mut cell = div().id((id_prefix, idx)).flex_1().h_full();
        if enabled {
            cell = cell
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cb(value, window, cx);
                });
        }
        hit_cells = hit_cells.child(cell);

        let is_center = center_mark == Some(value);
        let dot_size: Pixels = if is_center { px(4.0) } else { px(2.5) };
        dots = dots.child(
            div()
                .w(dot_size)
                .h(dot_size)
                .rounded_full()
                .bg(if is_center {
                    theme.text_secondary
                } else {
                    theme.text_muted.opacity(0.6)
                }),
        );
    }

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .when(!enabled, |this| this.opacity(0.5))
        .child(hit_cells)
        .child(dots)
}

/// Visual macOS Tahoe Liquid Glass thumbnail tile for Menu Bar Display Mode selection.
pub fn menu_bar_preview_tile(
    id: impl Into<ElementId>,
    sample_text: &'static str,
    caption: &'static str,
    selected: bool,
    theme: &GlassTheme,
    on_select: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let outer_ring = if selected {
        theme.accent
    } else {
        rgba(0x00000000).into()
    };

    let mini_bar = div()
        .w_full()
        .h(px(21.0))
        .bg(rgba(0xFFFFFFDE))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(3.5))
        .child(vector_icon(
            IconName::Fan,
            10.5,
            Hsla::from(rgba(0x181C24E8)),
        ))
        .when(!sample_text.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(0x181C24E8))
                    .child(sample_text),
            )
        });

    let mini_glass_card = div()
        .mx(px(11.0))
        .mt(px(7.0))
        .h(px(22.0))
        .rounded(px(5.0))
        .bg(rgba(0xFFFFFF3D))
        .border_1()
        .border_color(rgba(0xFFFFFF55));

    let wallpaper = div()
        .w_full()
        .h(px(56.0))
        .rounded(px(9.5))
        .overflow_hidden()
        .bg(linear_gradient(
            145.0,
            linear_color_stop(gpui::Hsla::from(rgba(0x2898FFFF)), 0.0),
            linear_color_stop(gpui::Hsla::from(rgba(0x0B56D6FF)), 1.0),
        ))
        .border_1()
        .border_color(rgba(0x00000018))
        .flex()
        .flex_col()
        .child(mini_bar)
        .child(mini_glass_card);

    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(5.0))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_select(window, cx);
        })
        .child(
            div()
                .w_full()
                .max_w(px(120.0))
                .p(px(2.5))
                .rounded(px(13.0))
                .border_2()
                .border_color(outer_ring)
                .child(wallpaper),
        )
        .child(
            div()
                .text_size(px(11.5))
                .whitespace_nowrap()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if selected {
                    theme.text_primary
                } else {
                    theme.text_secondary
                })
                .child(t(caption)),
        )
}

/// Converts a list of elements into a vertical stack separated by 1px hairlines.
pub fn card_rows(theme: &GlassTheme, rows: Vec<AnyElement>) -> Div {
    let mut card = glass_card(theme);
    for (idx, row) in rows.into_iter().enumerate() {
        if idx > 0 {
            card = card.child(div().px(px(16.0)).child(hairline(theme)));
        }
        card = card.child(row);
    }
    card
}
