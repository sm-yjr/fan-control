//! GPUI Liquid Glass interactive fan curve editor modal overlay.
use crate::gpui_ui::{
    charts::curve_plot_canvas,
    primitives::{
        glass_card, glass_segmented, glass_toggle, hairline, inset_well, pill_button, t,
        ButtonStyle,
    },
    state::{AppState, CurveDraft},
    theme::GlassTheme,
};
use crate::presenter::{self, CurvePreset};
use gpui::{div, prelude::*, px, rgba, BoxShadow, Entity, FontWeight, MouseButton, Point};

pub fn render_curve_editor_modal(
    theme: &GlassTheme,
    state: &Entity<AppState>,
    draft: &CurveDraft,
    live_input: Option<f64>,
) -> impl IntoElement {
    let matching = presenter::matching_preset(&draft.curve).and_then(|p| {
        CurvePreset::ALL
            .iter()
            .position(|candidate| *candidate == p)
    });

    let state_preset = state.clone();
    let preset_bar = glass_segmented(
        "curve-preset",
        &["安静", "均衡", "性能"],
        matching,
        true,
        false,
        theme,
        move |idx, _, cx| {
            if let Some(&preset) = CurvePreset::ALL.get(idx) {
                state_preset.update(cx, |s, cx| s.curve_apply_preset(preset, cx));
            }
        },
    );

    let state_close_bg = state.clone();
    let state_cancel = state.clone();
    let state_undo = state.clone();
    let state_add = state.clone();
    let state_save = state.clone();
    let state_all = state.clone();

    let mut point_rows = div().flex().flex_col().gap(px(6.0));
    for (idx, pt) in draft.curve.points.iter().enumerate() {
        let selected = draft.selected_point == Some(idx);
        let state_sel = state.clone();
        let state_t_minus = state.clone();
        let state_t_plus = state.clone();
        let state_s_minus = state.clone();
        let state_s_plus = state.clone();
        let state_rm = state.clone();
        let can_remove = draft.curve.points.len() > 2;
        let row_bg = if selected {
            theme.accent_soft
        } else {
            theme.well_bg
        };

        point_rows = point_rows.child(
            div()
                .id(("curve-pt-row", idx))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(12.0))
                .py(px(6.0))
                .rounded(px(9.0))
                .bg(row_bg)
                .border_1()
                .border_color(if selected {
                    theme.accent.opacity(0.4)
                } else {
                    theme.card_border
                })
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    state_sel.update(cx, |s, cx| {
                        if let Some(d) = s.curve_editor.as_mut() {
                            d.selected_point = Some(idx);
                        }
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_secondary)
                        .child(format!("#{}", idx + 1)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(nudge_chip(("pt-t-m", idx), "−", theme, move |_, cx| {
                            state_t_minus
                                .update(cx, |s, cx| s.curve_nudge_point(idx, -2.0, 0.0, cx));
                        }))
                        .child(
                            div()
                                .w(px(52.0))
                                .text_center()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_primary)
                                .child(format!("{:.0}°C", pt.temperature)),
                        )
                        .child(nudge_chip(("pt-t-p", idx), "+", theme, move |_, cx| {
                            state_t_plus.update(cx, |s, cx| s.curve_nudge_point(idx, 2.0, 0.0, cx));
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(nudge_chip(("pt-s-m", idx), "−", theme, move |_, cx| {
                            state_s_minus
                                .update(cx, |s, cx| s.curve_nudge_point(idx, 0.0, -5.0, cx));
                        }))
                        .child(
                            div()
                                .w(px(46.0))
                                .text_center()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.accent)
                                .child(format!("{:.0}%", pt.speed_percent)),
                        )
                        .child(nudge_chip(("pt-s-p", idx), "+", theme, move |_, cx| {
                            state_s_plus.update(cx, |s, cx| s.curve_nudge_point(idx, 0.0, 5.0, cx));
                        })),
                )
                .child(pill_button(
                    ("pt-rm", idx),
                    "删除",
                    ButtonStyle::DangerSoft,
                    can_remove,
                    theme,
                    move |_, cx| {
                        state_rm.update(cx, |s, cx| s.curve_remove_point(idx, cx));
                    },
                )),
        );
    }

    let modal_card = glass_card(theme)
        .w(px(560.0))
        .max_h(px(560.0))
        .bg(theme.detail_bg)
        .border_1()
        .border_color(theme.capsule_border)
        .shadow(vec![BoxShadow {
            color: rgba(0x00000044).into(),
            offset: Point::new(px(0.0), px(12.0)),
            blur_radius: px(32.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .p(px(18.0))
        .gap(px(12.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(t("编辑温度曲线")),
                )
                .child(preset_bar),
        )
        .child(inset_well(theme).p(px(10.0)).child(curve_plot_canvas(
            theme,
            &draft.curve,
            live_input,
            draft.selected_point,
            168.0,
        )))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_secondary)
                        .child(t("控制点")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(pill_button(
                            "curve-add-pt",
                            "添加控制点",
                            ButtonStyle::Secondary,
                            draft.curve.points.len() < 12,
                            theme,
                            move |_, cx| {
                                state_add.update(cx, |s, cx| s.curve_add_point(cx));
                            },
                        ))
                        .child(pill_button(
                            "curve-undo",
                            "撤销",
                            ButtonStyle::Secondary,
                            !draft.undo.is_empty(),
                            theme,
                            move |_, cx| {
                                state_undo.update(cx, |s, cx| s.curve_undo(cx));
                            },
                        )),
                ),
        )
        .child(
            div()
                .id("curve-points-scroll")
                .max_h(px(160.0))
                .overflow_y_scroll()
                .child(point_rows),
        )
        .child(hairline(theme))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(glass_toggle(
                            "curve-all-fans",
                            draft.all_fans,
                            true,
                            theme,
                            move |checked, _, cx| {
                                state_all.update(cx, |s, cx| {
                                    if let Some(d) = s.curve_editor.as_mut() {
                                        d.all_fans = checked;
                                    }
                                    cx.notify();
                                });
                            },
                        ))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.text_primary)
                                .child(t("应用到所有风扇")),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(pill_button(
                            "curve-cancel",
                            "取消",
                            ButtonStyle::Secondary,
                            true,
                            theme,
                            move |_, cx| {
                                state_cancel.update(cx, |s, cx| s.close_curve_editor(cx));
                            },
                        ))
                        .child(pill_button(
                            "curve-save",
                            "应用",
                            ButtonStyle::Primary,
                            true,
                            theme,
                            move |_, cx| {
                                state_save.update(cx, |s, cx| s.curve_save(cx));
                            },
                        )),
                ),
        )
        .when(!draft.error.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.warning)
                    .child(t(&draft.error)),
            )
        });

    div()
        .id("curve-modal-backdrop")
        .absolute()
        .inset_0()
        .bg(rgba(0x0000004D))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            state_close_bg.update(cx, |s, cx| s.close_curve_editor(cx));
        })
        .child(modal_card)
}

fn nudge_chip(
    id: (&'static str, usize),
    glyph: &'static str,
    theme: &GlassTheme,
    on_click: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let hover = theme.capsule_hover;
    div()
        .id(id)
        .w(px(22.0))
        .h(px(22.0))
        .rounded_full()
        .bg(theme.capsule_bg)
        .border_1()
        .border_color(theme.capsule_border)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.0))
        .text_color(theme.text_primary)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            on_click(window, cx);
        })
        .child(glyph)
}
