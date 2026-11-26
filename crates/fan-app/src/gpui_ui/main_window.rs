//! GPUI Liquid Glass main split-view window modeled directly on the macOS Tahoe Rex reference:
//! translucent Liquid Glass sidebar, `< | >` glass navigation capsule, soft grouped cards,
//! Metal gauges & trend charts, and complete Overview / Fans / Smart / Details / Settings views.
use crate::gpui_ui::{
    charts::{rpm_arc_gauge, temperature_trend_chart},
    curve_editor::render_curve_editor_modal,
    glass,
    primitives::{
        card_rows, chevron_row, glass_card, glass_segmented, glass_toggle, hairline, info_footnote,
        inline_picker_trigger, inset_well, menu_bar_preview_tile, nav_capsule, pill_button,
        section_title, settings_row, squircle_badge, status_badge, step_slider, t, vector_icon,
        ButtonStyle,
    },
    state::{cooling_preference_label, AppState, Section},
    theme::{GlassTheme, IconTint},
};
use crate::i18n::Lang;
use crate::presenter::{self, FanChoice, Plan, Tone};
use crate::sensor_list::{GroupFilter, SortOrder};
use crate::status_bar::MenuBarDisplayMode;
use fan_core::SensorGroup;
use gpui::{
    div, prelude::*, px, AnyElement, BoxShadow, Context, Entity, FontWeight, MouseButton, Point,
    Render, Window,
};
use gpui_kit_assets::IconName;

pub const MAIN_WIDTH: f64 = 860.0;
pub const MAIN_HEIGHT: f64 = 640.0;

pub struct MainWindowView {
    pub state: Entity<AppState>,
}

impl MainWindowView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self { state }
    }
}

impl Render for MainWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = GlassTheme::for_appearance(window.appearance());
        let st = self.state.read(cx);
        let section = st.section;
        let can_back = !st.back_stack.is_empty();
        let can_fwd = !st.forward_stack.is_empty();
        let view = st.presentation();
        let curve_modal = st.curve_editor.clone();
        let live_curve_input = curve_modal.as_ref().and_then(|d| {
            if st.is_fresh() {
                st.snapshot
                    .snapshot
                    .input_value(&d.curve.sensor_key, &st.snapshot.thermal)
            } else {
                None
            }
        });

        let sidebar = self.render_sidebar(&theme, &view, section, cx);

        let state_back = self.state.clone();
        let state_fwd = self.state.clone();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(14.0))
            .px(px(24.0))
            .pt(px(14.0))
            .pb(px(10.0))
            .child(nav_capsule(
                &theme,
                can_back,
                can_fwd,
                move |_, cx| {
                    state_back.update(cx, |s, cx| s.go_back(cx));
                },
                move |_, cx| {
                    state_fwd.update(cx, |s, cx| s.go_forward(cx));
                },
            ))
            .child(
                div()
                    .text_size(px(16.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_primary)
                    .child(t(section.title())),
            );

        let page_content: AnyElement = match section {
            Section::Overview => self.render_overview(&theme, cx).into_any_element(),
            Section::Fans => self.render_fans(&theme, cx).into_any_element(),
            Section::Smart => self.render_smart(&theme, cx).into_any_element(),
            Section::Details => self.render_details(&theme, cx).into_any_element(),
            Section::Settings => self.render_settings(&theme, cx).into_any_element(),
        };

        let detail_pane = div()
            .flex_1()
            .h_full()
            .bg(theme.detail_bg)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .id("detail-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(26.0))
                    .pb(px(26.0))
                    .pt(px(4.0))
                    .child(page_content),
            );

        div()
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .child(sidebar)
            .child(div().w(px(1.0)).h_full().bg(theme.sidebar_border))
            .child(detail_pane)
            .when_some(curve_modal, |this, draft| {
                this.child(render_curve_editor_modal(
                    &theme,
                    &self.state,
                    &draft,
                    live_curve_input,
                ))
            })
    }
}

impl MainWindowView {
    fn render_sidebar(
        &self,
        theme: &GlassTheme,
        view: &presenter::Presentation,
        active: Section,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hero_tint = match view.notice.tone {
            Tone::Good => IconTint::Blue,
            Tone::Info => IconTint::Purple,
            Tone::Warning => IconTint::Orange,
            Tone::Danger => IconTint::Rose,
        };
        let state_word = crate::i18n::translate(match view.notice.tone {
            Tone::Good => match view.plan {
                Plan::System => "系统默认",
                Plan::Smart => "智能散热",
                Plan::Custom => "自定义",
                Plan::Mixed => "分别设置",
            },
            Tone::Info => "提示",
            Tone::Warning => "需要注意",
            Tone::Danger => "温度偏高",
        });
        let status_summary = match view.temperature {
            Some(temp) => format!("{state_word} · {temp:.0}°C"),
            None => state_word,
        };

        let hero_selected = active == Section::Overview;
        let state_hero = self.state.clone();
        let hover_pill = theme.sidebar_hover_pill;
        let hero_card = div()
            .id("sidebar-hero")
            .mx(px(10.0))
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(10.0))
            .when(hero_selected, |this| this.bg(theme.sidebar_active_pill))
            .when(!hero_selected, |this| this.hover(move |s| s.bg(hover_pill)))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                state_hero.update(cx, |s, cx| s.navigate(Section::Overview, cx));
            })
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .child(squircle_badge(IconName::Fan, hero_tint, 34.0, true))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_primary)
                            .child("Fan Control"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_secondary)
                            .child(status_summary),
                    ),
            );

        let mut nav_list = div().flex().flex_col().gap(px(2.0)).px(px(10.0));
        for (idx, section) in Section::ALL.into_iter().enumerate() {
            if idx == 4 {
                // Visual section gap before Settings (matches Rex sidebar grouping).
                nav_list = nav_list.child(div().h(px(10.0)));
            }
            let is_active = section == active;
            let state_nav = self.state.clone();
            let hover_bg = theme.sidebar_hover_pill;
            let item = div()
                .id(("sidebar-item", idx))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(9.0))
                .when(is_active, |this| this.bg(theme.sidebar_active_pill))
                .when(!is_active, |this| this.hover(move |s| s.bg(hover_bg)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    state_nav.update(cx, |s, cx| s.navigate(section, cx));
                })
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(squircle_badge(section.icon(), section.tint(), 22.0, false))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(if is_active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(theme.text_primary)
                        .child(t(section.title())),
                );
            nav_list = nav_list.child(item);
        }

        let state_reset = self.state.clone();
        let controls_enabled = view.controls_enabled;
        let reset_hover = theme.sidebar_hover_pill;
        let mut reset_btn = div()
            .id("sidebar-reset")
            .mx(px(12.0))
            .px(px(10.0))
            .py(px(7.0))
            .rounded(px(8.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text_secondary)
            .child(vector_icon(IconName::RotateCcw, 12.5, theme.text_secondary))
            .child(t("恢复系统自动"));

        if controls_enabled {
            reset_btn = reset_btn
                .cursor_pointer()
                .hover(move |s| s.bg(reset_hover))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    state_reset.update(cx, |s, cx| s.send_reset(cx));
                });
        } else {
            reset_btn = reset_btn.opacity(0.45);
        }

        let _ = cx;
        div()
            .w(px(210.0))
            .h_full()
            .flex_shrink_0()
            .bg(theme.sidebar_bg)
            .flex()
            .flex_col()
            .pt(px(42.0))
            .pb(px(14.0))
            .gap(px(12.0))
            .child(hero_card)
            .child(nav_list)
            .child(div().flex_1())
            .child(reset_btn)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Section 1: 概览 (Overview Dashboard)
    // ──────────────────────────────────────────────────────────────────────────
    fn render_overview(&self, theme: &GlassTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let fresh = st.is_fresh();
        let view = st.presentation();
        let cards = st.fan_cards();
        let smart = st.smart_status();
        let custom_pct = st
            .pending_speed
            .map(|(p, _)| p)
            .unwrap_or(view.custom_percent);

        // Hero status card (Rex Server style)
        let state_action = self.state.clone();
        let hero_btn: AnyElement = if let Some(action) = view.notice.action {
            let label = match action {
                presenter::Action::EnableControl => "启用…",
                presenter::Action::Reconnect => "重新连接",
            };
            pill_button(
                "ov-notice-act",
                label,
                ButtonStyle::Primary,
                true,
                theme,
                move |_, cx| {
                    state_action.update(cx, |s, cx| s.handle_notice_action(cx));
                },
            )
            .into_any_element()
        } else {
            let state_rst = self.state.clone();
            pill_button(
                "ov-reset-all",
                "全部交还",
                ButtonStyle::Secondary,
                view.controls_enabled,
                theme,
                move |_, cx| {
                    state_rst.update(cx, |s, cx| s.send_reset(cx));
                },
            )
            .into_any_element()
        };

        let hero_card = glass_card(theme).p(px(16.0)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .flex_1()
                        .child(squircle_badge(
                            IconName::Fan,
                            match view.notice.tone {
                                Tone::Good => IconTint::Blue,
                                Tone::Info => IconTint::Purple,
                                Tone::Warning => IconTint::Orange,
                                Tone::Danger => IconTint::Rose,
                            },
                            38.0,
                            true,
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.text_primary)
                                                .child(t(view.notice.title)),
                                        )
                                        .child(status_badge(
                                            theme,
                                            view.notice.tone,
                                            match view.plan {
                                                Plan::System => "系统默认",
                                                Plan::Smart => "智能散热",
                                                Plan::Custom => "自定义",
                                                Plan::Mixed => "分别设置",
                                            },
                                        )),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(theme.text_secondary)
                                        .child(t(view.notice.body)),
                                ),
                        ),
                )
                .child(hero_btn),
        );

        // Quick Cooling Plan switcher
        let selected_plan = match view.plan {
            Plan::System => Some(0),
            Plan::Smart => Some(1),
            Plan::Custom => Some(2),
            Plan::Mixed => None,
        };
        let state_plan = self.state.clone();
        let plan_card = glass_card(theme)
            .p(px(16.0))
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_primary)
                            .child(t("散热方式")),
                    )
                    .child(glass_segmented(
                        "ov-plan",
                        &["系统默认", "智能散热", "自定义"],
                        selected_plan,
                        view.controls_enabled,
                        false,
                        theme,
                        move |idx, _, cx| {
                            let p = match idx {
                                0 => Plan::System,
                                1 => Plan::Smart,
                                _ => Plan::Custom,
                            };
                            state_plan.update(cx, |s, cx| s.apply_plan(p, custom_pct, cx));
                        },
                    )),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_secondary)
                    .child(t(view.plan_description)),
            );

        // Temperature + 10-Min Trend Card
        let temp_str = view
            .temperature
            .map(|v| format!("{v:.0}°C"))
            .unwrap_or_else(|| "—".into());
        let demand_str = smart
            .demand
            .map(|d| format!("{d:.0}%"))
            .unwrap_or_else(|| "—".into());

        let thermal_card = glass_card(theme).p(px(16.0)).gap(px(12.0)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(18.0))
                .child(
                    div()
                        .w(px(155.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_secondary)
                                .child(t("芯片温度")),
                        )
                        .child(
                            div()
                                .text_size(px(36.0))
                                .line_height(px(38.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text_primary)
                                .child(temp_str),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(8.0))
                                .text_size(px(11.5))
                                .text_color(theme.text_secondary)
                                .child(t(view.trend))
                                .child(format!(
                                    "{} {}",
                                    crate::i18n::translate("热负荷"),
                                    demand_str
                                )),
                        ),
                )
                .child(
                    inset_well(theme)
                        .flex_1()
                        .p(px(12.0))
                        .child(temperature_trend_chart(
                            theme,
                            &st.history,
                            view.temperature,
                            fresh,
                            82.0,
                        )),
                ),
        );

        // Fan Gauges Card
        let mut gauges_row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_around()
            .gap(px(16.0));
        if cards.is_empty() {
            gauges_row = gauges_row.child(
                div()
                    .py(px(16.0))
                    .text_size(px(12.5))
                    .text_color(theme.text_secondary)
                    .child(t("未检测到可控风扇")),
            );
        } else {
            for card in &cards {
                gauges_row = gauges_row.child(rpm_arc_gauge(theme, card, false));
            }
        }
        let fans_card = glass_card(theme).p(px(16.0)).child(gauges_row);

        // Battery / Power telemetry card (if present)
        let power_card: Option<AnyElement> = st.snapshot.battery.as_ref().map(|bat| {
            let (headline, lines) = presenter::battery_lines(bat);
            let sub = lines.join(" · ");
            glass_card(theme)
                .child(settings_row(
                    theme,
                    "电源与电池",
                    Some(&sub),
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(t(&headline)),
                ))
                .into_any_element()
        });

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(hero_card)
            .child(section_title(theme, "控制模式"))
            .child(plan_card)
            .child(section_title(theme, "温度与风扇"))
            .child(thermal_card)
            .child(fans_card)
            .children(power_card)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Section 2: 风扇 (Per-Fan Control Cards)
    // ──────────────────────────────────────────────────────────────────────────
    fn render_fans(&self, theme: &GlassTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let cards = st.fan_cards();
        let editable = !st.demo && st.snapshot.helper_ready && !st.snapshot.installing;

        let mut list = div().flex().flex_col().gap(px(12.0));
        if cards.is_empty() {
            list = list.child(
                glass_card(theme).p(px(24.0)).child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme.text_secondary)
                        .child(t("当前设备未检测到可控风扇。")),
                ),
            );
            return list;
        }

        let choices = [
            FanChoice::System,
            FanChoice::Smart,
            FanChoice::Fixed,
            FanChoice::Curve,
        ];

        for card in cards {
            let fan_id = card.id;
            let selected_idx = choices.iter().position(|c| *c == card.choice);
            let live_pct = st
                .pending_fan_speeds
                .get(&fan_id)
                .map(|(p, _)| *p)
                .unwrap_or(card.fixed_percent);
            let hint = presenter::rpm_hint(live_pct, card.range);

            let state_choice = self.state.clone();
            let mode_seg = glass_segmented(
                match fan_id {
                    0 => "fan-mode-0",
                    1 => "fan-mode-1",
                    2 => "fan-mode-2",
                    _ => "fan-mode-n",
                },
                &["系统自动", "智能散热", "固定速度", "温度曲线"],
                selected_idx,
                editable,
                true,
                theme,
                move |idx, _, cx| {
                    if let Some(&choice) = choices.get(idx) {
                        state_choice
                            .update(cx, |s, cx| s.apply_fan_choice(fan_id, choice, live_pct, cx));
                    }
                },
            );

            let mut fan_panel = glass_card(theme).p(px(16.0)).gap(px(12.0)).child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(18.0))
                    .child(rpm_arc_gauge(theme, &card, true))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(mode_seg)
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .child(t(card.choice.explanation())),
                            )
                            .when(!card.target.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.text_muted)
                                        .child(t(&card.target)),
                                )
                            }),
                    ),
            );

            if card.choice == FanChoice::Fixed {
                let pct_steps: Vec<i32> = (0..=20).map(|i| i * 5).collect();
                let state_slider = self.state.clone();
                fan_panel = fan_panel
                    .child(hairline(theme))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(format!(
                                        "{} {:.0}%",
                                        crate::i18n::translate("目标转速"),
                                        live_pct
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_secondary)
                                    .child(hint),
                            ),
                    )
                    .child(step_slider(
                        match fan_id {
                            0 => "fan-spd-0",
                            1 => "fan-spd-1",
                            _ => "fan-spd-n",
                        },
                        &pct_steps,
                        ((live_pct / 5.0).round() as i32 * 5).clamp(0, 100),
                        None,
                        editable,
                        theme,
                        move |v, _, cx| {
                            state_slider
                                .update(cx, |s, cx| s.set_fan_percent(fan_id, v as f64, cx));
                        },
                    ));
            } else if card.choice == FanChoice::Curve {
                let state_curve = self.state.clone();
                fan_panel = fan_panel.child(hairline(theme)).child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme.text_secondary)
                                .child(t("按自定义温度曲线平滑调节转速。")),
                        )
                        .child(pill_button(
                            ("fan-curve-btn", fan_id as usize),
                            "编辑曲线…",
                            ButtonStyle::Primary,
                            true,
                            theme,
                            move |_, cx| {
                                state_curve
                                    .update(cx, |s, cx| s.open_curve_editor(Some(fan_id), cx));
                            },
                        )),
                );
            }

            list = list.child(fan_panel);
        }

        list
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Section 3: 智能散热 (Smart Cooling & Comfort Calibration)
    // ──────────────────────────────────────────────────────────────────────────
    fn render_smart(&self, theme: &GlassTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let smart = st.smart_status();
        let bias = st.current_bias();
        let saving = st.pending_policy_save.is_some() || st.snapshot.installing;
        let pref_notice = st.preference_notice.clone();
        let pref_err = st.preference_notice_is_error;
        let policy = st.policy.clone();

        let status_card = glass_card(theme).p(px(16.0)).child(
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
                        .gap(px(12.0))
                        .child(squircle_badge(
                            IconName::Sparkles,
                            IconTint::Purple,
                            36.0,
                            true,
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(14.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text_primary)
                                        .child(format!(
                                            "{} · {}",
                                            crate::i18n::translate("热负荷"),
                                            crate::i18n::translate(smart.level)
                                        )),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(theme.text_secondary)
                                        .child(t(smart.reason)),
                                ),
                        ),
                )
                .child(status_badge(
                    theme,
                    if smart.in_use { Tone::Good } else { Tone::Info },
                    if smart.in_use {
                        "已启用"
                    } else {
                        "未启用"
                    },
                )),
        );

        let steps: Vec<i32> = (-10..=10).collect();
        let state_bias = self.state.clone();
        let state_reset = self.state.clone();
        let pref_card = glass_card(theme)
            .p(px(16.0))
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_primary)
                            .child(t("散热偏好")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .px(px(10.0))
                                    .py(px(3.0))
                                    .rounded_full()
                                    .bg(theme.capsule_bg)
                                    .border_1()
                                    .border_color(theme.capsule_border)
                                    .text_size(px(11.5))
                                    .text_color(theme.text_primary)
                                    .child(cooling_preference_label(bias)),
                            )
                            .when(bias != 0, |this| {
                                this.child(pill_button(
                                    "smart-pref-reset",
                                    "恢复默认",
                                    ButtonStyle::Link,
                                    !saving,
                                    theme,
                                    move |_, cx| {
                                        state_reset.update(cx, |s, cx| s.save_tuning_bias(0, cx));
                                    },
                                ))
                            }),
                    ),
            )
            .child(step_slider(
                "smart-pref",
                &steps,
                bias as i32,
                Some(0),
                !saving,
                theme,
                move |v, _, cx| {
                    state_bias.update(cx, |s, cx| s.save_tuning_bias(v as i8, cx));
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .text_size(px(11.5))
                    .text_color(theme.text_secondary)
                    .child(t("更安静"))
                    .child(t("默认"))
                    .child(t("更凉快")),
            )
            .when(!pref_notice.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(if pref_err {
                            theme.warning
                        } else {
                            theme.text_secondary
                        })
                        .child(t(&pref_notice)),
                )
            });

        // Collapsible Comfort Target card
        let state_disc = self.state.clone();
        let state_comfort_toggle = self.state.clone();
        let state_c_minus = self.state.clone();
        let state_c_plus = self.state.clone();
        let state_save_policy = self.state.clone();
        let mut comfort_card = glass_card(theme).child(chevron_row(
            "smart-cal-toggle",
            "键盘体感目标与两点校准",
            Some(if policy.comfort_enabled {
                "已启用"
            } else {
                "默认 38°C"
            }),
            theme,
            move |_, cx| {
                state_disc.update(cx, |s, cx| {
                    s.policy.calibration_open = !s.policy.calibration_open;
                    cx.notify();
                });
            },
        ));

        if policy.calibration_open {
            comfort_card = comfort_card
                .child(div().px(px(16.0)).child(hairline(theme)))
                .child(settings_row(
                    theme,
                    "启用体感温度目标",
                    Some("需同设备外部两点校准且处于有效范围时生效。"),
                    glass_toggle(
                        "comfort-enabled",
                        policy.comfort_enabled,
                        !saving,
                        theme,
                        move |checked, _, cx| {
                            state_comfort_toggle.update(cx, |s, cx| {
                                s.policy.comfort_enabled = checked;
                                cx.notify();
                            });
                        },
                    ),
                ))
                .child(div().px(px(16.0)).child(hairline(theme)))
                .child(settings_row(
                    theme,
                    "体感目标温度",
                    Some("默认 38°C，超过校准上限时自动保持保守散热下界。"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(pill_button(
                            "comfort-minus",
                            "−",
                            ButtonStyle::Secondary,
                            !saving && policy.comfort_target_c > 32.0,
                            theme,
                            move |_, cx| {
                                state_c_minus.update(cx, |s, cx| {
                                    s.policy.comfort_target_c =
                                        (s.policy.comfort_target_c - 1.0).clamp(32.0, 46.0);
                                    cx.notify();
                                });
                            },
                        ))
                        .child(
                            div()
                                .w(px(46.0))
                                .text_center()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_primary)
                                .child(format!("{:.0}°C", policy.comfort_target_c)),
                        )
                        .child(pill_button(
                            "comfort-plus",
                            "+",
                            ButtonStyle::Secondary,
                            !saving && policy.comfort_target_c < 46.0,
                            theme,
                            move |_, cx| {
                                state_c_plus.update(cx, |s, cx| {
                                    s.policy.comfort_target_c =
                                        (s.policy.comfort_target_c + 1.0).clamp(32.0, 46.0);
                                    cx.notify();
                                });
                            },
                        ))
                        .child(pill_button(
                            "comfort-save",
                            "保存",
                            ButtonStyle::Primary,
                            !saving,
                            theme,
                            move |_, cx| {
                                state_save_policy.update(cx, |s, cx| s.save_thermal_policy(cx));
                            },
                        )),
                ))
                .when(!policy.notice.is_empty(), |this| {
                    this.child(
                        div()
                            .px(px(16.0))
                            .pb(px(12.0))
                            .text_size(px(11.5))
                            .text_color(if policy.notice_is_error {
                                theme.warning
                            } else {
                                theme.text_secondary
                            })
                            .child(t(&policy.notice)),
                    )
                });
        }

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(status_card)
            .child(section_title(theme, "散热偏好"))
            .child(pref_card)
            .child(info_footnote(
                theme,
                "调整散热偏好后立即生效并自动保存；持续负载下会提前平缓介入，热量消散后自动交还系统。",
            ))
            .child(section_title(theme, "高级"))
            .child(comfort_card)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Section 4: 温度详情 (Sensor Telemetry Table with Group Filter & Sort)
    // ──────────────────────────────────────────────────────────────────────────
    fn render_details(&self, theme: &GlassTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let fresh = st.is_fresh();
        let group_filter = st.sensor_group;
        let sort_order = st.sensor_sort;

        let state_grp = self.state.clone();
        let group_seg = glass_segmented(
            "sensor-grp",
            &["全部", "处理器", "图形", "机身", "其他"],
            GroupFilter::ALL.iter().position(|g| *g == group_filter),
            true,
            false,
            theme,
            move |idx, _, cx| {
                if let Some(&g) = GroupFilter::ALL.get(idx) {
                    state_grp.update(cx, |s, cx| {
                        s.sensor_group = g;
                        cx.notify();
                    });
                }
            },
        );

        let state_sort = self.state.clone();
        let sort_seg = glass_segmented(
            "sensor-sort",
            &["按分组", "按最热"],
            SortOrder::ALL.iter().position(|s| *s == sort_order),
            true,
            false,
            theme,
            move |idx, _, cx| {
                if let Some(&o) = SortOrder::ALL.get(idx) {
                    state_sort.update(cx, |s, cx| {
                        s.sensor_sort = o;
                        cx.notify();
                    });
                }
            },
        );

        let mut sensors: Vec<_> = st
            .snapshot
            .snapshot
            .sensors
            .iter()
            .filter(|s| match group_filter {
                GroupFilter::All => true,
                GroupFilter::Cpu => s.group == SensorGroup::Cpu,
                GroupFilter::Gpu => s.group == SensorGroup::Gpu,
                GroupFilter::System => s.group == SensorGroup::System,
                GroupFilter::Other => s.group == SensorGroup::Other,
            })
            .cloned()
            .collect();

        if sort_order == SortOrder::HottestFirst {
            sensors.sort_by(|a, b| b.value.unwrap_or(-1.0).total_cmp(&a.value.unwrap_or(-1.0)));
        } else {
            let rank = |g: SensorGroup| match g {
                SensorGroup::Cpu => 0u8,
                SensorGroup::Gpu => 1,
                SensorGroup::System => 2,
                SensorGroup::Other => 3,
            };
            sensors.sort_by(|a, b| rank(a.group).cmp(&rank(b.group)).then(a.name.cmp(&b.name)));
        }

        let mut rows: Vec<AnyElement> = Vec::new();
        for sensor in sensors.into_iter().take(180) {
            let val_opt = fresh.then_some(sensor.value).flatten();
            let val_text = val_opt
                .map(|v| format!("{v:.1}°C"))
                .unwrap_or_else(|| "—".into());
            let tone = val_opt
                .map(presenter::temperature_level)
                .unwrap_or(Tone::Info);
            let (fg, _) = theme.tone_colors(tone);
            let ratio = val_opt
                .map(|v| ((v - 20.0) / 85.0).clamp(0.0, 1.0) as f32)
                .unwrap_or(0.0);

            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(9.0))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(sensor.name),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.text_muted)
                                    .child(sensor.key),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .w(px(100.0))
                                    .h(px(5.0))
                                    .rounded_full()
                                    .bg(theme.slider_track)
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(ratio))
                                            .rounded_full()
                                            .bg(fg),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(54.0))
                                    .text_right()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if val_opt.is_some() {
                                        theme.text_primary
                                    } else {
                                        theme.text_muted
                                    })
                                    .child(val_text),
                            ),
                    )
                    .into_any_element(),
            );
        }

        if rows.is_empty() {
            rows.push(
                div()
                    .p(px(20.0))
                    .text_size(px(12.5))
                    .text_color(theme.text_secondary)
                    .child(t("没有匹配的传感器读数。"))
                    .into_any_element(),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(group_seg)
                    .child(sort_seg),
            )
            .child(card_rows(theme, rows))
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Section 5: 设置 (Rex-inspired macOS Tahoe Liquid Glass Settings!)
    // ──────────────────────────────────────────────────────────────────────────
    fn render_settings(&self, theme: &GlassTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let demo = st.demo;
        let active_mode = st.menu_bar_mode;
        let login_on = st.login_enabled;
        let msg = st.settings_message.clone();
        let msg_is_err = st.settings_message_is_error;
        let lang_open = st.language_menu_open;
        let installing = st.snapshot.installing;
        let ready = st.snapshot.helper_ready;
        let cfg_notice = st.snapshot.configuration_notice.clone();
        let can_update = st.can_check_updates();
        let version = crate::app_version::current();

        // ── Section 1: 外观与通用 ──
        let modes: [(MenuBarDisplayMode, &'static str, &'static str); 4] = [
            (MenuBarDisplayMode::IconOnly, "", "仅图标"),
            (MenuBarDisplayMode::Temperature, "54°", "芯片温度"),
            (MenuBarDisplayMode::FanSpeed, "2.4k", "最高转速"),
            (
                MenuBarDisplayMode::TemperatureAndFan,
                "54°·2.4k",
                "温度与转速",
            ),
        ];
        let mut tiles_row = div()
            .flex()
            .flex_row()
            .items_start()
            .justify_between()
            .gap(px(10.0))
            .w_full();
        for (idx, (mode, sample, caption)) in modes.into_iter().enumerate() {
            let state_mode = self.state.clone();
            tiles_row = tiles_row.child(menu_bar_preview_tile(
                ("mb-tile", idx),
                sample,
                caption,
                mode == active_mode,
                theme,
                move |_, cx| {
                    state_mode.update(cx, |s, cx| s.set_menu_bar_mode(mode, cx));
                },
            ));
        }
        let picker_block = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .px(px(16.0))
            .py(px(14.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child(t("菜单栏显示")),
            )
            .child(tiles_row);

        let state_login = self.state.clone();
        let login_row = settings_row(
            theme,
            "登录时启动",
            None,
            glass_toggle(
                "settings-login",
                login_on,
                !demo,
                theme,
                move |wanted, _, cx| {
                    state_login.update(cx, |s, cx| s.toggle_login(wanted, cx));
                },
            ),
        );

        let current_lang = crate::i18n::override_language();
        let lang_label = match current_lang {
            Lang::System => "跟随系统",
            Lang::Chinese => "简体中文",
            Lang::English => "English",
        };
        let state_lang_toggle = self.state.clone();
        let mut lang_section = div().relative().child(settings_row(
            theme,
            "界面语言",
            None,
            inline_picker_trigger(
                "settings-lang-trigger",
                lang_label,
                !demo,
                theme,
                move |_, cx| {
                    state_lang_toggle.update(cx, |s, cx| {
                        s.language_menu_open = !s.language_menu_open;
                        cx.notify();
                    });
                },
            ),
        ));
        if lang_open {
            let mut menu = div()
                .mx(px(16.0))
                .mb(px(10.0))
                .p(px(4.0))
                .rounded(px(10.0))
                .bg(theme.well_bg)
                .border_1()
                .border_color(theme.capsule_border)
                .shadow(vec![BoxShadow {
                    color: theme.shadow,
                    offset: Point::new(px(0.0), px(4.0)),
                    blur_radius: px(12.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .flex()
                .flex_row()
                .gap(px(4.0));
            for (idx, (lang, title)) in [
                (Lang::System, "跟随系统"),
                (Lang::Chinese, "简体中文"),
                (Lang::English, "English"),
            ]
            .into_iter()
            .enumerate()
            {
                let active = lang == current_lang;
                let state_pick = self.state.clone();
                let hover = theme.sidebar_hover_pill;
                menu = menu.child(
                    div()
                        .id(("lang-opt", idx))
                        .flex_1()
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .when(active, |this| {
                            this.bg(theme.accent).text_color(gpui::rgba(0xFFFFFFFF))
                        })
                        .when(!active, |this| {
                            this.text_color(theme.text_primary)
                                .hover(move |s| s.bg(hover))
                        })
                        .cursor_pointer()
                        .text_center()
                        .text_size(px(12.0))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            state_pick.update(cx, |s, cx| s.set_language(lang, cx));
                        })
                        .child(t(title)),
                );
            }
            lang_section = lang_section.child(menu);
        }

        let general_card = card_rows(
            theme,
            vec![
                picker_block.into_any_element(),
                login_row.into_any_element(),
                lang_section.into_any_element(),
            ],
        );

        // ── Section 2: 风扇控制服务 (Rex Server Card style!) ──
        let (svc_tone, svc_title) = if demo {
            (Tone::Info, "演示模式")
        } else if installing {
            (Tone::Warning, "等待管理员授权")
        } else if ready {
            (Tone::Good, "已启用")
        } else {
            (Tone::Info, "未启用")
        };
        let state_install = self.state.clone();
        let state_retry = self.state.clone();
        let state_reset = self.state.clone();
        let state_uninstall = self.state.clone();

        let service_status_row = settings_row(
            theme,
            "控制服务状态",
            None,
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(status_badge(theme, svc_tone, svc_title))
                .when(!demo && !ready && !installing, |this| {
                    this.child(pill_button(
                        "settings-install",
                        "启用…",
                        ButtonStyle::Primary,
                        true,
                        theme,
                        move |_, cx| {
                            state_install.update(cx, |s, cx| s.send_install(cx));
                        },
                    ))
                })
                .child(pill_button(
                    "settings-retry",
                    "重新检测",
                    ButtonStyle::Link,
                    true,
                    theme,
                    move |_, cx| {
                        state_retry.update(cx, |s, cx| s.send_retry(cx));
                    },
                )),
        );

        let reset_row = settings_row(
            theme,
            "立即交还系统",
            None,
            pill_button(
                "settings-reset",
                "全部交还",
                ButtonStyle::Secondary,
                true,
                theme,
                move |_, cx| {
                    state_reset.update(cx, |s, cx| s.send_reset(cx));
                },
            ),
        );

        let uninstall_row = settings_row(
            theme,
            "移除控制服务",
            None,
            pill_button(
                "settings-uninstall",
                "移除…",
                ButtonStyle::DangerSoft,
                !demo,
                theme,
                move |_, cx| {
                    state_uninstall.update(cx, |s, cx| s.request_uninstall(cx));
                },
            ),
        );

        let service_card = card_rows(
            theme,
            vec![
                service_status_row.into_any_element(),
                reset_row.into_any_element(),
                uninstall_row.into_any_element(),
            ],
        );

        // ── Section 3: 支持与关于 ──
        let state_update = self.state.clone();
        let state_export = self.state.clone();
        let version_str = format!("{} ({})", version.version, version.build);

        let update_row = settings_row(
            theme,
            "软件更新",
            None,
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(theme.text_secondary)
                        .child(version_str),
                )
                .child(pill_button(
                    "settings-update",
                    "检查更新…",
                    ButtonStyle::Secondary,
                    can_update,
                    theme,
                    move |_, cx| {
                        state_update.read(cx).check_updates();
                    },
                )),
        );
        let export_row = chevron_row(
            "settings-export",
            "诊断信息",
            Some("导出…"),
            theme,
            move |_, cx| {
                state_export.update(cx, |s, _| s.export_diagnostics());
            },
        );
        let help_row = chevron_row("settings-help", "使用帮助", None, theme, |_, _| {
            glass::open_help_url();
        });
        let about_row = chevron_row(
            "settings-about",
            "关于 Fan Control",
            None,
            theme,
            |_, _| {
                glass::show_about_panel();
            },
        );

        let support_card = card_rows(
            theme,
            vec![
                update_row.into_any_element(),
                export_row.into_any_element(),
                help_row.into_any_element(),
                about_row.into_any_element(),
            ],
        );

        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(section_title(theme, "外观与通用"))
            .child(general_card)
            .child(info_footnote(
                theme,
                "开机后自动在菜单栏运行；切换界面语言需重新打开应用生效。",
            ))
            .when(!msg.is_empty(), |this| {
                this.child(
                    div()
                        .px(px(6.0))
                        .text_size(px(11.5))
                        .text_color(if msg_is_err {
                            theme.warning
                        } else {
                            theme.text_secondary
                        })
                        .child(t(&msg)),
                )
            })
            .child(div().h(px(6.0)))
            .child(section_title(theme, "风扇控制服务"))
            .child(service_card)
            .child(info_footnote(
                theme,
                "应用退出、失联或电脑睡眠时会自动交还 macOS；移除服务后仍可查看温度，你的设置会保留。",
            ))
            .when(!cfg_notice.is_empty(), |this| {
                this.child(
                    div()
                        .px(px(6.0))
                        .text_size(px(11.5))
                        .text_color(theme.warning)
                        .child(t(&cfg_notice)),
                )
            })
            .child(div().h(px(6.0)))
            .child(section_title(theme, "支持与关于"))
            .child(support_card)
    }
}
