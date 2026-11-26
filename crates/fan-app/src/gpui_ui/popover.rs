//! GPUI Liquid Glass floating menu-bar dashboard panel.
use crate::gpui_ui::{
    charts::{rpm_arc_gauge, temperature_trend_chart},
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

pub const POPOVER_WIDTH: f64 = 400.0;
pub const POPOVER_HEIGHT: f64 = 560.0;

type OpenMainCallback = Box<dyn Fn(Section, &mut gpui::App)>;
type HidePopoverCallback = Box<dyn Fn(&mut gpui::App)>;

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
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self {
            state,
            on_open_main: Box::new(on_open_main),
            on_hide_popover: Box::new(on_hide_popover),
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
            .map(|(p, _)| p)
            .unwrap_or(view.custom_percent);
        let saving = st.pending_policy_save.is_some() || st.snapshot.installing;
        let pref_notice = st.preference_notice.clone();
        let pref_is_err = st.preference_notice_is_error;
        let demo = st.demo;

        let temp_digits = view
            .temperature
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "—".into());

        // ── Top Header ──
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(9.0))
                    .child(squircle_badge(IconName::Fan, IconTint::Blue, 24.0, true))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_primary)
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
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .py(px(9.0))
                    .rounded(px(12.0))
                    .bg(bg)
                    .border_1()
                    .border_color(fg.opacity(0.28))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
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

        // ── Hero Card: Chip Temperature + 10-Min Metal Trend ──
        let temp_card = glass_card(&theme).p(px(14.0)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(14.0))
                .child(
                    div()
                        .w(px(116.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(11.5))
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
                                        .text_size(px(38.0))
                                        .line_height(px(40.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text_primary)
                                        .child(temp_digits),
                                )
                                .child(
                                    div()
                                        .text_size(px(16.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text_secondary)
                                        .child("°C"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(theme.text_secondary)
                                .child(t(view.trend)),
                        ),
                )
                .child(div().flex_1().child(temperature_trend_chart(
                    &theme,
                    &st.history,
                    view.temperature,
                    fresh,
                    58.0,
                ))),
        );

        // ── Fan Gauges Glass Card ──
        let mut gauges_wrap = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_around()
            .gap(px(10.0));
        if cards.is_empty() {
            gauges_wrap = gauges_wrap.child(
                div()
                    .py(px(14.0))
                    .text_size(px(12.5))
                    .text_color(theme.text_secondary)
                    .child(t("未检测到可控风扇")),
            );
        } else {
            for card in &cards {
                gauges_wrap = gauges_wrap.child(rpm_arc_gauge(&theme, card, true));
            }
        }
        let fans_card = glass_card(&theme)
            .p(px(14.0))
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(12.0))
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
            .p(px(14.0))
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(12.0))
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
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_primary)
                                    .child(t("散热偏好")),
                            )
                            .child(
                                div()
                                    .px(px(9.0))
                                    .py(px(2.5))
                                    .rounded_full()
                                    .bg(theme.capsule_bg)
                                    .border_1()
                                    .border_color(theme.capsule_border)
                                    .text_size(px(11.0))
                                    .text_color(theme.text_secondary)
                                    .child(format!(
                                        "{}：{}",
                                        crate::i18n::translate("当前选择"),
                                        cooling_preference_label(bias)
                                    )),
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
                            .flex()
                            .flex_row()
                            .justify_between()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child(t("更安静"))
                            .child(t("默认"))
                            .child(t("更凉快")),
                    )
                    .when(bias != 0 || !pref_notice.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(if bias != 0 {
                                    pill_button(
                                        "pop-pref-reset",
                                        "恢复默认",
                                        ButtonStyle::Link,
                                        !saving,
                                        &theme,
                                        move |_, cx| {
                                            state_reset
                                                .update(cx, |s, cx| s.save_tuning_bias(0, cx));
                                        },
                                    )
                                    .into_any_element()
                                } else {
                                    div().into_any_element()
                                })
                                .when(!pref_notice.is_empty(), |row| {
                                    row.child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(if pref_is_err {
                                                theme.warning
                                            } else {
                                                theme.text_secondary
                                            })
                                            .child(t(&pref_notice)),
                                    )
                                }),
                        )
                    });
            }
            Plan::Custom => {
                let pct_steps: Vec<i32> = (0..=20).map(|i| i * 5).collect();
                let state_speed = self.state.clone();
                let state_curve = self.state.clone();
                let view_handle = cx.entity().clone();
                control_card = control_card
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_primary)
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
                                    state_curve.update(cx, |s, cx| s.open_curve_editor(None, cx));
                                    view_handle.update(cx, |this, cx| {
                                        (this.on_hide_popover)(cx);
                                        (this.on_open_main)(Section::Fans, cx);
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
                    ));
            }
            Plan::System | Plan::Mixed => {}
        }

        control_card = control_card.child(
            div()
                .text_size(px(11.5))
                .text_color(theme.text_secondary)
                .child(t(view.plan_description)),
        );

        // ── Footer Bar ──
        let open_main_btn = {
            let view_handle = cx.entity().clone();
            pill_button(
                "pop-open-main",
                "打开 Fan Control",
                ButtonStyle::Link,
                true,
                &theme,
                move |_, cx| {
                    view_handle.update(cx, |this, cx| {
                        (this.on_hide_popover)(cx);
                        (this.on_open_main)(Section::Overview, cx);
                    });
                },
            )
        };
        let open_settings_btn = {
            let view_handle = cx.entity().clone();
            let hover = theme.sidebar_hover_pill;
            div()
                .id("pop-open-settings")
                .w(px(28.0))
                .h(px(28.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .text_color(theme.text_secondary)
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    view_handle.update(cx, |this, cx| {
                        (this.on_hide_popover)(cx);
                        (this.on_open_main)(Section::Settings, cx);
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
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.quit();
                })
                .child(vector_icon(IconName::Power, 14.5, theme.text_secondary))
        };

        let footer = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .pt(px(2.0))
            .child(open_main_btn)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(open_settings_btn)
                    .child(quit_btn),
            );

        div()
            .size_full()
            .rounded(px(18.0))
            .bg(theme.popover_bg)
            .border_1()
            .border_color(theme.card_sheen)
            .overflow_hidden()
            .flex()
            .flex_col()
            .p(px(14.0))
            .gap(px(10.0))
            .child(header)
            .children(notice_banner)
            .child(temp_card)
            .child(fans_card)
            .child(control_card)
            .child(hairline(&theme))
            .child(footer)
            .when(demo, |this| {
                this.child(
                    div()
                        .text_size(px(10.5))
                        .text_color(theme.text_muted)
                        .child(t("界面验证 · 模拟数据")),
                )
            })
    }
}
