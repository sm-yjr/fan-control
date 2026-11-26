//! GPUI Liquid Glass floating menu-bar dashboard panel with pinned header/footer,
//! scrollable card body, transparent Root synthesis, curve-mode protection, and Escape dismissal.
use crate::gpui_ui::{
    charts::{rpm_arc_gauge, temperature_trend_chart},
    glass,
    primitives::{
        glass_card, glass_segmented, hairline, pill_button, squircle_badge, status_badge,
        step_slider, t, vector_icon, ButtonStyle,
    },
    state::{cooling_preference_label, AppState, Section},
    theme::{GlassTheme, IconTint},
};
use crate::presenter::Plan;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, FontWeight, MouseButton, Render, Window,
};
use gpui_kit_assets::IconName;
use std::rc::Rc;

pub const POPOVER_WIDTH: f64 = 392.0;
pub const POPOVER_HEIGHT: f64 = 580.0;

type OpenMainCallback = Rc<dyn Fn(Section, &mut gpui::App)>;
type HidePopoverCallback = Rc<dyn Fn(&mut gpui::App)>;
type DeactivateCallback = Rc<dyn Fn(&mut Window)>;

pub struct PopoverView {
    pub state: Entity<AppState>,
    pub on_open_main: OpenMainCallback,
    pub on_hide_popover: HidePopoverCallback,
}

impl PopoverView {
    pub fn new(
        state: Entity<AppState>,
        on_open_main: impl Fn(Section, &mut gpui::App) + 'static,
        on_hide_popover: impl Fn(&mut gpui::App) + 'static,
        on_deactivate: Option<DeactivateCallback>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        cx.observe_window_appearance(window, |_, window, cx| {
            glass::sync_glass_root_theme(window, cx);
        })
        .detach();
        if let Some(deactivate) = on_deactivate {
            cx.observe_window_activation(window, move |_, window, _| {
                if !window.is_window_active() {
                    deactivate(window);
                }
            })
            .detach();
        }
        Self {
            state,
            on_open_main: Rc::new(on_open_main),
            on_hide_popover: Rc::new(on_hide_popover),
        }
    }
}

impl Render for PopoverView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = GlassTheme::for_appearance(window.appearance());
        let st = self.state.read(cx);
        let fresh = st.is_fresh();
        let view = st.presentation();
        let cards = st.fan_cards();
        let bias = st.current_bias();
        let custom_pct = st
            .pending_speed
            .map(|p| p.percent)
            .unwrap_or(view.custom_percent);
        let custom_curves = st.custom_uses_curves();
        let force_fixed = st.popover_fixed_override;
        let saving = st.pending_policy_save.is_some() || st.snapshot.installing;
        let pref_notice = st.preference_notice.clone();
        let pref_is_err = st.preference_notice_is_error;
        let demo = st.demo;

        let temp_digits = view
            .temperature
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "—".into());

        // ── Pinned Top Header ──
        let header = div()
            .flex_shrink_0()
            .px(px(14.0))
            .pt(px(12.0))
            .pb(px(8.0))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .min_w_0()
                    .child(squircle_badge(IconName::Fan, IconTint::Blue, 22.0, true))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_primary)
                            .whitespace_nowrap()
                            .child("Fan Control"),
                    ),
            )
            .child(status_badge(&theme, view.notice.tone, view.notice.title));

        // ── Optional Attention Banner ──
        let notice_banner: Option<AnyElement> =
            if view.notice.action.is_some() || view.notice.tone != crate::presenter::Tone::Good {
                let state_entity = self.state.clone();
                let (fg, bg) = theme.tone_colors(view.notice.tone);
                let mut row = div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .px(px(11.0))
                    .py(px(8.0))
                    .rounded(px(12.0))
                    .bg(bg)
                    .border_1()
                    .border_color(fg.opacity(0.28))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.5))
                            .line_height(px(15.0))
                            .text_color(theme.text_primary)
                            .child(t(view.notice.body)),
                    );
                if let Some(action) = view.notice.action {
                    let action_label = match action {
                        crate::presenter::Action::EnableControl => "启用…",
                        crate::presenter::Action::Reconnect => "重新连接",
                    };
                    row = row.child(pill_button(
                        "popover-notice-action",
                        action_label,
                        ButtonStyle::Primary,
                        true,
                        &theme,
                        move |_, cx| {
                            state_entity.update(cx, |s, cx| s.handle_notice_action(cx));
                        },
                    ));
                }
                Some(row.into_any_element())
            } else {
                None
            };

        // ── Hero Card: Chip Temperature + Fixed 600s Metal Trend ──
        let temp_card = glass_card(&theme).p(px(12.0)).child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(104.0))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_secondary)
                                .child(t("芯片温度")),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_baseline()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(32.0))
                                        .line_height(px(34.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text_primary)
                                        .child(temp_digits),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text_secondary)
                                        .child("°C"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_secondary)
                                .truncate()
                                .child(t(view.trend)),
                        ),
                )
                .child(div().flex_1().min_w_0().child(temperature_trend_chart(
                    &theme,
                    &st.history,
                    view.temperature,
                    fresh,
                    46.0,
                ))),
        );

        // ── Fan Gauges Glass Card ──
        let mut gauges_wrap = div()
            .w_full()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_around()
            .gap(px(10.0));
        if cards.is_empty() {
            gauges_wrap = gauges_wrap.child(
                div()
                    .py(px(12.0))
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child(t("未检测到可控风扇")),
            );
        } else {
            for card in &cards {
                gauges_wrap = gauges_wrap.child(rpm_arc_gauge(&theme, card, true));
            }
        }
        let fans_card = glass_card(&theme)
            .p(px(12.0))
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_secondary)
                    .child(t("风扇转速")),
            )
            .child(gauges_wrap);

        // ── Cooling Plan Control Card ──
        let selected_plan = match view.plan {
            Plan::System => Some(0),
            Plan::Smart => Some(1),
            Plan::Custom => Some(2),
            Plan::Mixed => None,
        };
        let state_plan = self.state.clone();
        let plan_seg = glass_segmented(
            "pop-plan",
            &["系统默认", "智能散热", "自定义"],
            selected_plan,
            view.controls_enabled,
            true,
            &theme,
            move |idx, _, cx| {
                let plan = match idx {
                    0 => Plan::System,
                    1 => Plan::Smart,
                    _ => Plan::Custom,
                };
                state_plan.update(cx, |s, cx| s.apply_plan(plan, custom_pct, cx));
            },
        );

        let mut control_card = glass_card(&theme)
            .p(px(12.0))
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_secondary)
                    .child(t("散热方式")),
            )
            .child(plan_seg);

        // Conditional mode-specific controls
        match view.plan {
            Plan::Smart => {
                let steps: Vec<i32> = (-10..=10).collect();
                let state_bias = self.state.clone();
                let state_reset = self.state.clone();
                control_card = control_card
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_primary)
                                    .whitespace_nowrap()
                                    .child(t("散热偏好")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .when(bias != 0, |this| {
                                        this.child(pill_button(
                                            "pop-pref-reset",
                                            "恢复默认",
                                            ButtonStyle::Link,
                                            !saving,
                                            &theme,
                                            move |_, cx| {
                                                state_reset
                                                    .update(cx, |s, cx| s.save_tuning_bias(0, cx));
                                            },
                                        ))
                                    })
                                    .child(
                                        div()
                                            .px(px(8.0))
                                            .py(px(2.0))
                                            .rounded_full()
                                            .bg(theme.capsule_bg)
                                            .border_1()
                                            .border_color(theme.capsule_border)
                                            .text_size(px(11.0))
                                            .text_color(theme.text_secondary)
                                            .whitespace_nowrap()
                                            .child(cooling_preference_label(bias)),
                                    ),
                            ),
                    )
                    .child(step_slider(
                        "pop-pref",
                        &steps,
                        bias as i32,
                        Some(0),
                        !saving,
                        &theme,
                        move |v, _, cx| {
                            state_bias.update(cx, |s, cx| s.save_tuning_bias(v as i8, cx));
                        },
                    ))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .text_size(px(10.5))
                            .text_color(theme.text_secondary)
                            .child(t("更安静"))
                            .child(t("默认"))
                            .child(t("更凉快")),
                    )
                    .when(!pref_notice.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(if pref_is_err {
                                    theme.warning
                                } else {
                                    theme.text_secondary
                                })
                                .child(t(&pref_notice)),
                        )
                    });
            }
            Plan::Custom => {
                let state_curve = self.state.clone();
                let on_hide = self.on_hide_popover.clone();
                let on_open = self.on_open_main.clone();

                if custom_curves && !force_fixed {
                    // Protect active fan curves from accidental overwrite by a stray slider click.
                    let state_override = self.state.clone();
                    control_card = control_card.child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_primary)
                                    .child(t("温度曲线生效中")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(pill_button(
                                        "pop-switch-fixed",
                                        "改用固定速度",
                                        ButtonStyle::Link,
                                        view.controls_enabled,
                                        &theme,
                                        move |_, cx| {
                                            state_override.update(cx, |s, cx| {
                                                s.popover_fixed_override = true;
                                                cx.notify();
                                            });
                                        },
                                    ))
                                    .child(pill_button(
                                        "pop-edit-curve",
                                        "编辑曲线…",
                                        ButtonStyle::Secondary,
                                        view.controls_enabled,
                                        &theme,
                                        move |_, cx| {
                                            state_curve
                                                .update(cx, |s, cx| s.open_curve_editor(None, cx));
                                            let on_hide = on_hide.clone();
                                            let on_open = on_open.clone();
                                            cx.defer(move |cx| {
                                                on_hide(cx);
                                                on_open(Section::Fans, cx);
                                            });
                                        },
                                    )),
                            ),
                    );
                } else {
                    let pct_steps: Vec<i32> = (0..=20).map(|i| i * 5).collect();
                    let state_speed = self.state.clone();
                    control_card = control_card
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text_primary)
                                        .whitespace_nowrap()
                                        .child(format!(
                                            "{} {:.0}%",
                                            crate::i18n::translate("固定速度"),
                                            custom_pct
                                        )),
                                )
                                .child(pill_button(
                                    "pop-edit-curve",
                                    "编辑曲线…",
                                    ButtonStyle::Secondary,
                                    view.controls_enabled,
                                    &theme,
                                    move |_, cx| {
                                        state_curve
                                            .update(cx, |s, cx| s.open_curve_editor(None, cx));
                                        let on_hide = on_hide.clone();
                                        let on_open = on_open.clone();
                                        cx.defer(move |cx| {
                                            on_hide(cx);
                                            on_open(Section::Fans, cx);
                                        });
                                    },
                                )),
                        )
                        .child(step_slider(
                            "pop-custom-pct",
                            &pct_steps,
                            ((custom_pct / 5.0).round() as i32 * 5).clamp(0, 100),
                            None,
                            view.controls_enabled,
                            &theme,
                            move |v, _, cx| {
                                state_speed.update(cx, |s, cx| s.set_custom_percent(v as f64, cx));
                            },
                        ))
                        .when(custom_curves, |this| {
                            this.child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.warning)
                                    .child(t("拖动滑块后将以固定速度替换当前曲线。")),
                            )
                        });
                }
            }
            Plan::System | Plan::Mixed => {}
        }

        control_card = control_card.child(
            div()
                .text_size(px(11.0))
                .line_height(px(15.0))
                .text_color(theme.text_secondary)
                .child(t(view.plan_description)),
        );

        // ── Pinned Footer Bar ──
        let open_main_btn = {
            let on_hide = self.on_hide_popover.clone();
            let on_open = self.on_open_main.clone();
            pill_button(
                "pop-open-main",
                "打开 Fan Control",
                ButtonStyle::Link,
                true,
                &theme,
                move |_, cx| {
                    let on_hide = on_hide.clone();
                    let on_open = on_open.clone();
                    cx.defer(move |cx| {
                        on_hide(cx);
                        on_open(Section::Overview, cx);
                    });
                },
            )
        };
        let open_settings_btn = {
            let on_hide = self.on_hide_popover.clone();
            let on_open = self.on_open_main.clone();
            let hover = theme.sidebar_hover_pill;
            div()
                .id("pop-open-settings")
                .w(px(28.0))
                .h(px(28.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let on_hide = on_hide.clone();
                    let on_open = on_open.clone();
                    cx.defer(move |cx| {
                        on_hide(cx);
                        on_open(Section::Settings, cx);
                    });
                })
                .child(vector_icon(IconName::Settings, 14.5, theme.text_secondary))
        };
        let quit_btn = {
            let hover = theme.danger_pill_bg;
            let danger = theme.danger;
            div()
                .id("pop-quit")
                .w(px(28.0))
                .h(px(28.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| s.bg(hover).text_color(danger))
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    cx.quit();
                })
                .child(vector_icon(IconName::Power, 14.5, theme.text_secondary))
        };

        let footer = div()
            .flex_shrink_0()
            .w_full()
            .flex()
            .flex_col()
            .child(hairline(&theme))
            .child(
                div()
                    .w_full()
                    .px(px(14.0))
                    .py(px(8.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(open_main_btn)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(open_settings_btn)
                            .child(quit_btn),
                    ),
            )
            .when(demo, |this| {
                this.child(
                    div()
                        .px(px(14.0))
                        .pb(px(6.0))
                        .text_size(px(10.0))
                        .text_color(theme.text_muted)
                        .child(t("界面验证 · 模拟数据")),
                )
            });

        let body = div()
            .id("popover-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .px(px(14.0))
            .pb(px(10.0))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(9.0))
                    .children(notice_banner)
                    .child(temp_card)
                    .child(fans_card)
                    .child(control_card),
            );

        let on_esc = self.on_hide_popover.clone();
        div()
            .size_full()
            .bg(theme.popover_bg)
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_key_down(move |ev, _, cx| {
                if ev.keystroke.key == "escape" {
                    let on_esc = on_esc.clone();
                    cx.defer(move |cx| on_esc(cx));
                }
            })
            .child(header)
            .child(body)
            .child(footer)
    }
}
