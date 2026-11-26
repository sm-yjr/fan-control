//! GPUI Liquid Glass interactive fan curve editor modal overlay with occluded backdrop,
//! control-source switching, hysteresis control, draggable Metal canvas, and full
//! core-validated ranges (`0..=100%` demand / `0..=120°C` temperature / `-20%..=100%` speed).
use crate::gpui_ui::{
    charts::curve_plot_canvas,
    primitives::{
        glass_card, glass_segmented, glass_toggle, hairline, inline_picker_trigger, inset_well,
        pill_button, t, ButtonStyle,
    },
    state::{AppState, CurveDraft},
    theme::GlassTheme,
};
use crate::presenter::{self, CurvePreset};
use fan_core::FAN_OFF_PERCENT;
use gpui::{div, prelude::*, px, rgba, BoxShadow, Entity, FontWeight, MouseButton, Point};

pub fn render_curve_editor_modal(
    theme: &GlassTheme,
    state: &Entity<AppState>,
    draft: &CurveDraft,
    live_input: Option<f64>,
    sources: &[(String, String)],
) -> impl IntoElement {
    let matching = presenter::matching_preset(&draft.curve).and_then(|p| {
        CurvePreset::ALL
            .iter()
            .position(|candidate| *candidate == p)
    });

    let unit = draft.x_unit();
    let max_x = draft.max_x();

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

    // Control source selector (cycles through Thermal Demand + CPU/GPU/System sensors).
    let current_source_idx = sources
        .iter()
        .position(|(k, _)| *k == draft.curve.sensor_key)
        .unwrap_or(0);
    let current_source_label = sources
        .get(current_source_idx)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| draft.curve.sensor_key.clone());
    let next_source_key = sources
        .get((current_source_idx + 1) % sources.len().max(1))
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| draft.curve.sensor_key.clone());
    let state_src = state.clone();
    let state_hyst_m = state.clone();
    let state_hyst_p = state.clone();

    let source_and_hysteresis = div()
        .w_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_secondary)
                        .whitespace_nowrap()
                        .child(t("控制源")),
                )
                .child(inline_picker_trigger(
                    "curve-source-cycle",
                    &current_source_label,
                    sources.len() > 1,
                    theme,
                    move |_, cx| {
                        let key = next_source_key.clone();
                        state_src.update(cx, |s, cx| s.curve_set_source(key, cx));
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_secondary)
                        .whitespace_nowrap()
                        .child(t("迟滞")),
                )
                .child(nudge_chip(
                    ("curve-hyst-m", 0),
                    "−",
                    theme,
                    move |_, cx| {
                        state_hyst_m.update(cx, |s, cx| s.curve_nudge_hysteresis(-1.0, cx));
                    },
                ))
                .child(
                    div()
                        .w(px(44.0))
                        .text_center()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(format!("{:.0}{unit}", draft.curve.hysteresis)),
                )
                .child(nudge_chip(("curve-hyst-p", 0), "+", theme, move |_, cx| {
                    state_hyst_p.update(cx, |s, cx| s.curve_nudge_hysteresis(1.0, cx));
                })),
        );

    let state_close_bg = state.clone();
    let state_cancel = state.clone();
    let state_undo = state.clone();
    let state_add = state.clone();
    let state_save = state.clone();
    let state_all = state.clone();
    let state_canvas_sel = state.clone();
    let state_canvas_drag = state.clone();

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
        let speed_label = if pt.speed_percent <= FAN_OFF_PERCENT + 0.01 {
            format!("{} (-20%)", crate::i18n::translate("停转"))
        } else {
            format!("{:.0}%", pt.speed_percent)
        };

        point_rows = point_rows.child(
            div()
                .id(("curve-pt-row", idx))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .px(px(10.0))
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
                    cx.stop_propagation();
                    state_sel.update(cx, |s, cx| {
                        if let Some(d) = s.curve_editor.as_mut() {
                            d.selected_point = Some(idx);
                        }
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .w(px(22.0))
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
                        .gap(px(5.0))
                        .child(nudge_chip(("pt-t-m", idx), "−", theme, move |_, cx| {
                            state_t_minus
                                .update(cx, |s, cx| s.curve_nudge_point(idx, -1.0, 0.0, cx));
                        }))
                        .child(
                            div()
                                .w(px(52.0))
                                .text_center()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_primary)
                                .child(format!("{:.0}{unit}", pt.temperature)),
                        )
                        .child(nudge_chip(("pt-t-p", idx), "+", theme, move |_, cx| {
                            state_t_plus.update(cx, |s, cx| s.curve_nudge_point(idx, 1.0, 0.0, cx));
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .child(nudge_chip(("pt-s-m", idx), "−", theme, move |_, cx| {
                            state_s_minus
                                .update(cx, |s, cx| s.curve_nudge_point(idx, 0.0, -5.0, cx));
                        }))
                        .child(
                            div()
                                .w(px(76.0))
                                .text_center()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.accent)
                                .whitespace_nowrap()
                                .child(speed_label),
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
        .w(px(580.0))
        .max_w(gpui::relative(0.92))
        .max_h(gpui::relative(0.92))
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
        .p(px(16.0))
        .gap(px(10.0))
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_mouse_up(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(15.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(t("编辑温度曲线")),
                )
                .child(preset_bar),
        )
        .child(source_and_hysteresis)
        .child(
            inset_well(theme)
                .flex_shrink_0()
                .p(px(8.0))
                .gap(px(4.0))
                .child(curve_plot_canvas(
                    theme,
                    &draft.curve,
                    live_input,
                    draft.selected_point,
                    156.0,
                    move |idx, _, cx| {
                        state_canvas_sel.update(cx, |s, cx| {
                            if let Some(d) = s.curve_editor.as_mut() {
                                d.selected_point = Some(idx);
                            }
                            cx.notify();
                        });
                    },
                    move |tx, ty, push_undo, _, cx| {
                        state_canvas_drag
                            .update(cx, |s, cx| s.curve_set_selected_at(tx, ty, push_undo, cx));
                    },
                ))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .px(px(6.0))
                        .text_size(px(10.5))
                        .text_color(theme.text_muted)
                        .child(format!("0{unit}"))
                        .child(t("可直接在图上拖动控制点，-20% 表示允许停转"))
                        .child(format!("{max_x:.0}{unit}")),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
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
                .flex_1()
                .min_h_0()
                .max_h(px(150.0))
                .overflow_y_scroll()
                .child(point_rows),
        )
        .child(hairline(theme))
        .child(
            div()
                .flex_shrink_0()
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
                            "应用到所有风扇",
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
                    .flex_shrink_0()
                    .text_size(px(11.5))
                    .text_color(theme.warning)
                    .child(t(&draft.error)),
            )
        });

    // P0 fix: `.occlude()` + stopping mouse/scroll propagation prevents clicks on the modal
    // backdrop from ever passing through to fan controls or "恢复系统自动" underneath.
    div()
        .id("curve-modal-backdrop")
        .occlude()
        .absolute()
        .inset_0()
        .bg(rgba(0x00000054))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            state_close_bg.update(cx, |s, cx| s.close_curve_editor(cx));
        })
        .on_mouse_up(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_scroll(|_, _, cx| {
            cx.stop_propagation();
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
        .flex_shrink_0()
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
