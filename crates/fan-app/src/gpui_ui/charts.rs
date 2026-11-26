//! GPU-accelerated Metal vector visualizations rendered via GPUI `canvas` and `PathBuilder`:
//! 180° RPM arc gauge, 10-minute temperature trend area chart, and interactive fan curve canvas.
use crate::gpui_ui::primitives::t;
use crate::gpui_ui::theme::GlassTheme;
use crate::presenter::{FanCard, Tone};
use crate::trend::History;
use fan_core::Curve;
use gpui::{
    canvas, div, linear_color_stop, linear_gradient, point, prelude::*, px, rgba, Bounds,
    FontWeight, PathBuilder, Pixels, Point, Window,
};
use std::f32::consts::PI;

/// 180° semicircular Metal-rendered RPM arc gauge with glowing tip indicator and crisp digits.
pub fn rpm_arc_gauge(theme: &GlassTheme, card: &FanCard, compact: bool) -> impl IntoElement {
    let ratio = card
        .percent
        .map(|p| (p / 100.0).clamp(0.0, 1.0) as f32)
        .unwrap_or(0.0);
    let has_ratio = card.percent.is_some();
    let gauge_tone = match card.percent {
        Some(p) if p >= 85.0 => Tone::Danger,
        Some(p) if p >= 65.0 => Tone::Warning,
        _ => Tone::Info,
    };
    let (arc_color, _) = theme.tone_colors(gauge_tone);
    let track_color = theme.slider_track;
    let stroke_w = if compact { 8.0 } else { 10.0 };
    let gauge_w = if compact { 146.0 } else { 184.0 };
    let gauge_h = if compact { 76.0 } else { 100.0 };

    let rpm_text = card
        .measured_rpm
        .map(format_rpm)
        .unwrap_or_else(|| "—".into());

    let arc_canvas = canvas(
        move |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _cx| {
            let cx = f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0;
            let cy = f32::from(bounds.origin.y) + f32::from(bounds.size.height) - 8.0;
            let r = (f32::from(bounds.size.width) / 2.0 - stroke_w - 4.0)
                .min(f32::from(bounds.size.height) - stroke_w - 6.0)
                .max(24.0);

            // Semicircle track (180° left to 0° right, y grows downward in GPUI).
            if let Some(path) = build_arc_path(cx, cy, r, PI, 0.0, stroke_w, 40) {
                window.paint_path(path, track_color);
            }

            if has_ratio && ratio > 0.005 {
                let end_angle = PI * (1.0 - ratio);
                if let Some(path) = build_arc_path(cx, cy, r, PI, end_angle, stroke_w, 40) {
                    window.paint_path(path, arc_color);
                }
                // Glowing white knob at the active arc tip.
                let tip_x = cx + r * end_angle.cos();
                let tip_y = cy - r * end_angle.sin();
                if let Some(halo) = build_circle_path(tip_x, tip_y, stroke_w * 0.62) {
                    window.paint_path(halo, arc_color);
                }
                if let Some(dot) = build_circle_path(tip_x, tip_y, stroke_w * 0.34) {
                    window.paint_path(dot, rgba(0xFFFFFFFF));
                }
            }
        },
    )
    .w(px(gauge_w))
    .h(px(gauge_h));

    div()
        .w(px(gauge_w))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.0))
        .child(
            div()
                .w_full()
                .text_center()
                .text_size(px(if compact { 11.5 } else { 13.0 }))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text_primary)
                .truncate()
                .child(card.name.clone()),
        )
        .child(
            div()
                .relative()
                .w(px(gauge_w))
                .h(px(gauge_h))
                .child(arc_canvas)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(4.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(if compact { 20.0 } else { 24.0 }))
                                .line_height(px(if compact { 22.0 } else { 26.0 }))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text_primary)
                                .child(rpm_text),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .line_height(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_muted)
                                .child("RPM"),
                        ),
                ),
        )
        .child(
            div()
                .w_full()
                .text_center()
                .text_size(px(11.0))
                .text_color(theme.text_secondary)
                .truncate()
                .child(t(&card.detail)),
        )
}

/// 10-minute temperature trend chart with translucent gradient area fill and live glow endpoint.
pub fn temperature_trend_chart(
    theme: &GlassTheme,
    history: &History,
    latest_temp: Option<f64>,
    fresh: bool,
    height: f32,
) -> impl IntoElement {
    let temps: Vec<Option<f64>> = history.samples().map(|s| s.temperature).collect();
    let valid: Vec<f64> = temps.iter().flatten().copied().collect();
    let (min_t, max_t) = if valid.is_empty() {
        (40.0, 80.0)
    } else {
        let lo = valid.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = valid.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let span = (hi - lo).max(10.0);
        let pad = span * 0.2;
        (
            ((lo - pad) / 5.0).floor() * 5.0,
            ((hi + pad) / 5.0).ceil() * 5.0,
        )
    };
    let line_color = if latest_temp.is_some_and(|t| t >= 90.0) {
        theme.danger
    } else if latest_temp.is_some_and(|t| t >= 75.0) {
        theme.warning
    } else {
        theme.accent
    };
    let grid_color = theme.hairline;
    let fill_top = line_color.opacity(0.28);
    let fill_bottom = line_color.opacity(0.02);

    let plot = canvas(
        move |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _cx| {
            let x0 = f32::from(bounds.origin.x) + 4.0;
            let y0 = f32::from(bounds.origin.y) + 6.0;
            let w = (f32::from(bounds.size.width) - 8.0).max(12.0);
            let h = (f32::from(bounds.size.height) - 12.0).max(12.0);

            // Subtle horizontal reference grid lines.
            for frac in [0.0f32, 0.5, 1.0] {
                let y = y0 + h * frac;
                let mut g = PathBuilder::stroke(px(1.0));
                g.move_to(point(px(x0), px(y)));
                g.line_to(point(px(x0 + w), px(y)));
                if let Ok(path) = g.build() {
                    window.paint_path(path, grid_color);
                }
            }

            if temps.len() < 2 {
                if fresh {
                    if let Some(v) = latest_temp {
                        let span_t = (max_t - min_t).max(1.0);
                        let norm = ((v - min_t) / span_t).clamp(0.0, 1.0) as f32;
                        let py = y0 + h * (1.0 - norm);
                        let mut line = PathBuilder::stroke(px(2.0));
                        line.move_to(point(px(x0), px(py)));
                        line.line_to(point(px(x0 + w), px(py)));
                        if let Ok(path) = line.build() {
                            window.paint_path(path, line_color.opacity(0.65));
                        }
                        if let Some(halo) = build_circle_path(x0 + w, py, 6.0) {
                            window.paint_path(halo, line_color.opacity(0.28));
                        }
                        if let Some(dot) = build_circle_path(x0 + w, py, 3.2) {
                            window.paint_path(dot, line_color);
                        }
                    }
                }
                return;
            }
            let span_t = (max_t - min_t).max(1.0);
            let step_x = w / (temps.len() - 1) as f32;

            // Break into contiguous runs whenever readings are missing.
            let mut run: Vec<Point<Pixels>> = Vec::new();
            let mut last_pt: Option<Point<Pixels>> = None;

            let flush_run = |pts: &[Point<Pixels>], window: &mut Window| {
                if pts.len() < 2 {
                    return;
                }
                let mut area = PathBuilder::fill();
                area.move_to(point(pts[0].x, px(y0 + h)));
                for p in pts {
                    area.line_to(*p);
                }
                area.line_to(point(pts[pts.len() - 1].x, px(y0 + h)));
                area.close();
                if let Ok(path) = area.build() {
                    window.paint_path(
                        path,
                        linear_gradient(
                            180.0,
                            linear_color_stop(fill_top, 0.0),
                            linear_color_stop(fill_bottom, 1.0),
                        ),
                    );
                }

                let mut stroke = PathBuilder::stroke(px(2.2));
                stroke.move_to(pts[0]);
                for p in &pts[1..] {
                    stroke.line_to(*p);
                }
                if let Ok(path) = stroke.build() {
                    window.paint_path(path, line_color);
                }
            };

            for (i, temp) in temps.iter().enumerate() {
                if let Some(v) = temp {
                    let norm = ((*v - min_t) / span_t).clamp(0.0, 1.0) as f32;
                    let pt = point(px(x0 + i as f32 * step_x), px(y0 + h * (1.0 - norm)));
                    run.push(pt);
                    last_pt = Some(pt);
                } else {
                    flush_run(&run, window);
                    run.clear();
                }
            }
            flush_run(&run, window);

            if fresh {
                if let Some(pt) = last_pt {
                    if let Some(halo) = build_circle_path(f32::from(pt.x), f32::from(pt.y), 6.0) {
                        window.paint_path(halo, line_color.opacity(0.28));
                    }
                    if let Some(dot) = build_circle_path(f32::from(pt.x), f32::from(pt.y), 3.2) {
                        window.paint_path(dot, line_color);
                    }
                }
            }
        },
    )
    .w_full()
    .h(px(height));

    let now_label = if fresh {
        latest_temp
            .map(|v| format!("{v:.0}°C"))
            .unwrap_or_else(|| "—".into())
    } else {
        "—".into()
    };

    div()
        .flex()
        .flex_col()
        .w_full()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_secondary)
                        .child(t("温度趋势")),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_secondary)
                        .child(format!("{max_t:.0}° / {min_t:.0}°")),
                ),
        )
        .child(plot)
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_size(px(10.5))
                .text_color(theme.text_muted)
                .child(t("10 分钟前"))
                .child(format!("{} · {now_label}", crate::i18n::translate("现在"))),
        )
}

/// Interactive Metal fan curve plot: grid, live sensor line, gradient fill, and control points.
pub fn curve_plot_canvas(
    theme: &GlassTheme,
    curve: &Curve,
    live_value: Option<f64>,
    selected_idx: Option<usize>,
    height: f32,
) -> impl IntoElement {
    let mut points = curve.points.clone();
    points.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
    let live_interp = live_value.map(|v| (v, curve.interpolate(v)));
    let min_x = 0.0f64;
    let max_x = 100.0f64;
    let accent = theme.accent;
    let grid_color = theme.hairline;
    let live_color = theme.warning;
    let fill_top = accent.opacity(0.24);
    let fill_bottom = accent.opacity(0.02);

    canvas(
        move |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _cx| {
            let x0 = f32::from(bounds.origin.x) + 10.0;
            let y0 = f32::from(bounds.origin.y) + 10.0;
            let w = (f32::from(bounds.size.width) - 20.0).max(20.0);
            let h = (f32::from(bounds.size.height) - 20.0).max(20.0);

            // Grid lines (25% steps).
            for step in 0..=4 {
                let f = step as f32 / 4.0;
                let mut hline = PathBuilder::stroke(px(1.0));
                hline.move_to(point(px(x0), px(y0 + h * f)));
                hline.line_to(point(px(x0 + w), px(y0 + h * f)));
                if let Ok(path) = hline.build() {
                    window.paint_path(path, grid_color);
                }
                let mut vline = PathBuilder::stroke(px(1.0));
                vline.move_to(point(px(x0 + w * f), px(y0)));
                vline.line_to(point(px(x0 + w * f), px(y0 + h)));
                if let Ok(path) = vline.build() {
                    window.paint_path(path, grid_color);
                }
            }

            if points.is_empty() {
                return;
            }
            let map_pt = |temp: f64, pct: f64| {
                let nx = ((temp - min_x) / (max_x - min_x)).clamp(0.0, 1.0) as f32;
                let ny = (pct / 100.0).clamp(0.0, 1.0) as f32;
                point(px(x0 + w * nx), px(y0 + h * (1.0 - ny)))
            };

            let mapped: Vec<Point<Pixels>> = points
                .iter()
                .map(|p| map_pt(p.temperature, p.speed_percent))
                .collect();

            // Extend flat ends to 0 and 100 so curve reflects actual interpolation.
            let left_ext = point(px(x0), mapped[0].y);
            let right_ext = point(px(x0 + w), mapped[mapped.len() - 1].y);

            let mut area = PathBuilder::fill();
            area.move_to(point(px(x0), px(y0 + h)));
            area.line_to(left_ext);
            for p in &mapped {
                area.line_to(*p);
            }
            area.line_to(right_ext);
            area.line_to(point(px(x0 + w), px(y0 + h)));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(
                    path,
                    linear_gradient(
                        180.0,
                        linear_color_stop(fill_top, 0.0),
                        linear_color_stop(fill_bottom, 1.0),
                    ),
                );
            }

            let mut stroke = PathBuilder::stroke(px(2.4));
            stroke.move_to(left_ext);
            for p in &mapped {
                stroke.line_to(*p);
            }
            stroke.line_to(right_ext);
            if let Ok(path) = stroke.build() {
                window.paint_path(path, accent);
            }

            // Live input vertical indicator + intersection dot.
            if let Some((v, interp)) = live_interp {
                let nx = ((v - min_x) / (max_x - min_x)).clamp(0.0, 1.0) as f32;
                let lx = x0 + w * nx;
                let mut ind = PathBuilder::stroke(px(1.5));
                ind.move_to(point(px(lx), px(y0)));
                ind.line_to(point(px(lx), px(y0 + h)));
                if let Ok(path) = ind.build() {
                    window.paint_path(path, live_color.opacity(0.65));
                }
                let cross = map_pt(v, interp);
                if let Some(c) = build_circle_path(f32::from(cross.x), f32::from(cross.y), 4.5) {
                    window.paint_path(c, live_color);
                }
            }

            // Control point handles.
            for (idx, p) in mapped.iter().enumerate() {
                let active = selected_idx == Some(idx);
                let px_x = f32::from(p.x);
                let px_y = f32::from(p.y);
                if active {
                    if let Some(halo) = build_circle_path(px_x, px_y, 9.5) {
                        window.paint_path(halo, accent.opacity(0.28));
                    }
                }
                if let Some(outer) = build_circle_path(px_x, px_y, if active { 6.2 } else { 5.0 }) {
                    window.paint_path(outer, accent);
                }
                if let Some(inner) = build_circle_path(px_x, px_y, if active { 3.2 } else { 2.4 }) {
                    window.paint_path(inner, rgba(0xFFFFFFFF));
                }
            }
        },
    )
    .w_full()
    .h(px(height))
}

fn build_arc_path(
    cx: f32,
    cy: f32,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    width: f32,
    segments: usize,
) -> Option<gpui::Path> {
    let mut b = PathBuilder::stroke(px(width));
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = start_angle + (end_angle - start_angle) * t;
        let x = cx + radius * angle.cos();
        let y = cy - radius * angle.sin();
        if i == 0 {
            b.move_to(point(px(x), px(y)));
        } else {
            b.line_to(point(px(x), px(y)));
        }
    }
    b.build().ok()
}

fn build_circle_path(cx: f32, cy: f32, radius: f32) -> Option<gpui::Path> {
    let mut b = PathBuilder::fill();
    let segs = 20;
    for i in 0..segs {
        let a = (i as f32 / segs as f32) * 2.0 * PI;
        let pt = point(px(cx + radius * a.cos()), px(cy + radius * a.sin()));
        if i == 0 {
            b.move_to(pt);
        } else {
            b.line_to(pt);
        }
    }
    b.close();
    b.build().ok()
}

fn format_rpm(rpm: f64) -> String {
    let rounded = rpm.round().max(0.0) as u64;
    if rounded >= 1000 {
        format!("{},{:03}", rounded / 1000, rounded % 1000)
    } else {
        rounded.to_string()
    }
}
