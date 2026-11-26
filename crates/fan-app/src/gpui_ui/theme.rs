//! Adaptive Light & Dark Liquid Glass design tokens modeled on macOS Tahoe System Settings.
use crate::presenter::Tone;
use gpui::{hsla, rgba, Hsla, WindowAppearance};

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct GlassTheme {
    pub is_dark: bool,
    /// Translucent sidebar surface layered over `NSGlassEffectView`.
    pub sidebar_bg: Hsla,
    pub sidebar_border: Hsla,
    pub sidebar_active_pill: Hsla,
    pub sidebar_hover_pill: Hsla,
    /// Airy detail pane surface over `NSGlassEffectView`.
    pub detail_bg: Hsla,
    /// Translucent floating popover shell over `NSGlassEffectView`.
    pub popover_bg: Hsla,
    /// Grouped 14px card fill & specular border.
    pub card_bg: Hsla,
    pub card_border: Hsla,
    pub card_sheen: Hsla,
    /// Elevated inset preview well (charts, gauges, curve canvas).
    pub well_bg: Hsla,
    pub well_border: Hsla,
    /// 1px hairline row divider inside cards.
    pub hairline: Hsla,
    /// Floating glass capsule (`< | >` nav pill, chips, buttons).
    pub capsule_bg: Hsla,
    pub capsule_hover: Hsla,
    pub capsule_border: Hsla,
    /// Recessed segmented control track & elevated active pill.
    pub segment_track: Hsla,
    pub segment_active: Hsla,
    pub segment_active_border: Hsla,
    /// Toggle switch track (off) and wide thumb.
    pub toggle_off: Hsla,
    pub toggle_thumb: Hsla,
    /// Slider track.
    pub slider_track: Hsla,
    /// Text hierarchy.
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    /// Semantic colors.
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub good: Hsla,
    pub good_soft: Hsla,
    pub warning: Hsla,
    pub warning_soft: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    /// Soft tinted pill button backgrounds.
    pub danger_pill_bg: Hsla,
    pub danger_pill_hover: Hsla,
    /// Input well background.
    pub input_bg: Hsla,
    pub input_border: Hsla,
    /// Subtle shadow color.
    pub shadow: Hsla,
}

impl GlassTheme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            sidebar_bg: rgba(0xF6F6F85E).into(),
            sidebar_border: rgba(0x00000014).into(),
            sidebar_active_pill: rgba(0x00000014).into(),
            sidebar_hover_pill: rgba(0x0000000A).into(),
            detail_bg: rgba(0xFCFCFDD8).into(),
            popover_bg: rgba(0xF8F8FA6E).into(),
            card_bg: rgba(0xFFFFFF99).into(),
            card_border: rgba(0x00000012).into(),
            card_sheen: rgba(0xFFFFFFC8).into(),
            well_bg: rgba(0xFFFFFFD8).into(),
            well_border: rgba(0x00000014).into(),
            hairline: rgba(0x00000012).into(),
            capsule_bg: rgba(0xFFFFFFCC).into(),
            capsule_hover: rgba(0xFFFFFFEB).into(),
            capsule_border: rgba(0x0000001A).into(),
            segment_track: rgba(0x00000010).into(),
            segment_active: rgba(0xFFFFFFF5).into(),
            segment_active_border: rgba(0x00000014).into(),
            toggle_off: rgba(0x00000026).into(),
            toggle_thumb: rgba(0xFFFFFFFF).into(),
            slider_track: rgba(0x00000024).into(),
            text_primary: rgba(0x161618FF).into(),
            text_secondary: rgba(0x1D1D1FB8).into(),
            text_muted: rgba(0x1D1D1F82).into(),
            accent: rgba(0x0066D6FF).into(),
            accent_soft: rgba(0x0066D61E).into(),
            good: rgba(0x137333FF).into(),
            good_soft: rgba(0x1E9E4922).into(),
            warning: rgba(0xB45309FF).into(),
            warning_soft: rgba(0xD9770622).into(),
            danger: rgba(0xC81E1EFF).into(),
            danger_soft: rgba(0xDC262620).into(),
            danger_pill_bg: rgba(0xDC262620).into(),
            danger_pill_hover: rgba(0xDC262632).into(),
            input_bg: rgba(0xFFFFFFE6).into(),
            input_border: rgba(0x00000022).into(),
            shadow: rgba(0x0000001A).into(),
        }
    }

    pub fn dark() -> Self {
        Self {
            is_dark: true,
            sidebar_bg: rgba(0x1C1C205E).into(),
            sidebar_border: rgba(0xFFFFFF16).into(),
            sidebar_active_pill: rgba(0xFFFFFF20).into(),
            sidebar_hover_pill: rgba(0xFFFFFF10).into(),
            detail_bg: rgba(0x1C1C20CC).into(),
            popover_bg: rgba(0x1E1E2270).into(),
            card_bg: rgba(0xFFFFFF12).into(),
            card_border: rgba(0xFFFFFF1A).into(),
            card_sheen: rgba(0xFFFFFF22).into(),
            well_bg: rgba(0x121216C8).into(),
            well_border: rgba(0xFFFFFF1A).into(),
            hairline: rgba(0xFFFFFF16).into(),
            capsule_bg: rgba(0xFFFFFF1A).into(),
            capsule_hover: rgba(0xFFFFFF28).into(),
            capsule_border: rgba(0xFFFFFF24).into(),
            segment_track: rgba(0xFFFFFF16).into(),
            segment_active: rgba(0xFFFFFF2A).into(),
            segment_active_border: rgba(0xFFFFFF28).into(),
            toggle_off: rgba(0xFFFFFF2A).into(),
            toggle_thumb: rgba(0xFFFFFFFF).into(),
            slider_track: rgba(0xFFFFFF2A).into(),
            text_primary: rgba(0xF5F5F7FF).into(),
            text_secondary: rgba(0xF5F5F7BA).into(),
            text_muted: rgba(0xF5F5F77A).into(),
            accent: rgba(0x006CE0FF).into(),
            accent_soft: rgba(0x0A84FF30).into(),
            good: rgba(0x4ADE80FF).into(),
            good_soft: rgba(0x30D15826).into(),
            warning: rgba(0xFBBF24FF).into(),
            warning_soft: rgba(0xFF9F0A28).into(),
            danger: rgba(0xFF6961FF).into(),
            danger_soft: rgba(0xFF453A28).into(),
            danger_pill_bg: rgba(0xFF453A2C).into(),
            danger_pill_hover: rgba(0xFF453A42).into(),
            input_bg: rgba(0xFFFFFF12).into(),
            input_border: rgba(0xFFFFFF26).into(),
            shadow: rgba(0x00000055).into(),
        }
    }

    pub fn tone_colors(&self, tone: Tone) -> (Hsla, Hsla) {
        match tone {
            Tone::Good => (self.good, self.good_soft),
            Tone::Info => (self.accent, self.accent_soft),
            Tone::Warning => (self.warning, self.warning_soft),
            Tone::Danger => (self.danger, self.danger_soft),
        }
    }

    /// Vibrant gradient stop pair for colorful macOS Settings squircle icons.
    pub fn icon_gradient(tint: IconTint) -> (Hsla, Hsla) {
        match tint {
            IconTint::Blue => (
                hsla(211. / 360., 1.0, 0.56, 1.0),
                hsla(217. / 360., 0.96, 0.44, 1.0),
            ),
            IconTint::Teal => (
                hsla(192. / 360., 0.85, 0.54, 1.0),
                hsla(201. / 360., 0.88, 0.41, 1.0),
            ),
            IconTint::Purple => (
                hsla(252. / 360., 0.88, 0.62, 1.0),
                hsla(262. / 360., 0.82, 0.48, 1.0),
            ),
            IconTint::Orange => (
                hsla(31. / 360., 0.98, 0.56, 1.0),
                hsla(20. / 360., 0.94, 0.46, 1.0),
            ),
            IconTint::Gray => (
                hsla(220. / 360., 0.08, 0.60, 1.0),
                hsla(220. / 360., 0.09, 0.45, 1.0),
            ),
            IconTint::Green => (
                hsla(142. / 360., 0.72, 0.48, 1.0),
                hsla(146. / 360., 0.76, 0.36, 1.0),
            ),
            IconTint::Rose => (
                hsla(352. / 360., 0.92, 0.60, 1.0),
                hsla(340. / 360., 0.88, 0.48, 1.0),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum IconTint {
    Blue,
    Teal,
    Purple,
    Orange,
    Gray,
    Green,
    Rose,
}
