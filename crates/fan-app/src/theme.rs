//! Unified semantic design tokens for Fan Control's native AppKit interface.
//! All colors resolve dynamically through AppKit semantic colors so Light Mode,
//! Dark Mode, Accent Color, and Increase Contrast work automatically.
use crate::presenter::Tone;
use objc2::rc::Retained;
use objc2_app_kit::*;
use objc2_foundation::NSString;

pub mod spacing {
    pub const XS: f64 = 4.0;
    pub const SM: f64 = 8.0;
    pub const MD: f64 = 12.0;
    pub const LG: f64 = 16.0;
    pub const XL: f64 = 20.0;
    pub const XXL: f64 = 24.0;
}

pub mod radius {
    pub const CARD: f64 = 12.0;
    pub const WELL: f64 = 10.0;
    pub const PILL: f64 = 999.0;
}

pub mod typography {
    pub const DISPLAY_HERO: f64 = 48.0;
    pub const DISPLAY_HERO_COMPACT: f64 = 42.0;
    pub const RPM_DIGITS: f64 = 28.0;
    pub const RPM_DIGITS_COMPACT: f64 = 24.0;
    pub const METRIC_LARGE: f64 = 24.0;
    pub const METRIC_MEDIUM: f64 = 20.0;
    pub const TITLE: f64 = 18.0;
    pub const SECTION: f64 = 15.0;
    pub const HEADLINE: f64 = 14.0;
    pub const BODY: f64 = 13.0;
    pub const CAPTION: f64 = 12.0;
    pub const SMALL: f64 = 11.0;
    pub const MICRO: f64 = 10.0;
}

pub mod layout {
    pub const PANEL_WIDTH: f64 = 400.0;
    pub const WINDOW_DEFAULT_WIDTH: f64 = 840.0;
    pub const WINDOW_DEFAULT_HEIGHT: f64 = 600.0;
    pub const WINDOW_MIN_WIDTH: f64 = 700.0;
    pub const WINDOW_MIN_HEIGHT: f64 = 500.0;
    pub const SIDEBAR_WIDTH: f64 = 196.0;
    pub const GAUGE_TRACK: f64 = 10.0;
    pub const HAIRLINE: f64 = 0.5;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceStyle {
    /// Layered card surface with subtle hairline border.
    Card,
    /// Recessed well inside a card or page (charts, inline summaries).
    Well,
    /// Neutral pill badge.
    NeutralPill,
    /// Semantic status pill with tinted fill and subtle border.
    TonePill(Tone),
    /// Semantic attention banner with tinted fill and border.
    Banner(Tone),
}

impl SurfaceStyle {
    pub fn corner_radius(self) -> f64 {
        match self {
            Self::Card | Self::Banner(_) => radius::CARD,
            Self::Well => radius::WELL,
            Self::NeutralPill | Self::TonePill(_) => radius::PILL,
        }
    }

    /// Resolves dynamic AppKit colors inside `drawRect:` so appearance switches
    /// (Light/Dark, Increase Contrast, Accent Color) repaint accurately.
    pub fn colors(self) -> (Option<Retained<NSColor>>, Option<Retained<NSColor>>) {
        match self {
            Self::Card => (
                Some(NSColor::quaternarySystemFillColor()),
                Some(NSColor::separatorColor().colorWithAlphaComponent(0.5)),
            ),
            Self::Well => (
                Some(NSColor::quinaryLabelColor()),
                Some(NSColor::separatorColor().colorWithAlphaComponent(0.35)),
            ),
            Self::NeutralPill => (
                Some(NSColor::quaternarySystemFillColor()),
                Some(NSColor::separatorColor().colorWithAlphaComponent(0.4)),
            ),
            Self::TonePill(tone) => {
                let tint = tone_color(tone);
                (
                    Some(tint.colorWithAlphaComponent(0.14)),
                    Some(tint.colorWithAlphaComponent(0.28)),
                )
            }
            Self::Banner(tone) => {
                let tint = tone_color(tone);
                (
                    Some(tint.colorWithAlphaComponent(0.12)),
                    Some(tint.colorWithAlphaComponent(0.32)),
                )
            }
        }
    }
}

pub fn tone_color(tone: Tone) -> Retained<NSColor> {
    match tone {
        Tone::Good => NSColor::systemGreenColor(),
        Tone::Info => NSColor::controlAccentColor(),
        Tone::Warning => NSColor::systemOrangeColor(),
        Tone::Danger => NSColor::systemRedColor(),
    }
}

pub fn tone_symbol(tone: Tone) -> &'static str {
    match tone {
        Tone::Good => "checkmark.circle.fill",
        Tone::Info => "info.circle.fill",
        Tone::Warning => "exclamationmark.triangle.fill",
        Tone::Danger => "flame.fill",
    }
}

/// Precision instrument numerals: SF Pro Rounded with monospaced digits,
/// matching the WidgetKit design and preventing horizontal jitter during updates.
pub fn rounded_digits(size: f64, weight: NSFontWeight) -> Retained<NSFont> {
    let base = NSFont::monospacedDigitSystemFontOfSize_weight(size, weight);
    let descriptor = base.fontDescriptor();
    let design = unsafe { NSFontDescriptorSystemDesignRounded };
    descriptor
        .fontDescriptorWithDesign(design)
        .and_then(|rounded| NSFont::fontWithDescriptor_size(&rounded, size))
        .unwrap_or(base)
}

pub fn symbol_image(
    name: &str,
    accessibility: Option<&str>,
    size: f64,
) -> Option<Retained<NSImage>> {
    let description = accessibility.map(|text| NSString::from_str(&crate::i18n::translate(text)));
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        description.as_deref(),
    )?;
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(size, unsafe {
        NSFontWeightMedium
    });
    image.imageWithSymbolConfiguration(&config).or(Some(image))
}

#[allow(dead_code)]
pub fn reduce_motion_enabled() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_radii_match_semantic_hierarchy() {
        assert!(SurfaceStyle::Card.corner_radius() > SurfaceStyle::Well.corner_radius());
        assert_eq!(
            SurfaceStyle::TonePill(Tone::Good).corner_radius(),
            radius::PILL
        );
    }

    #[test]
    fn every_tone_has_a_distinct_symbol() {
        let symbols = [
            tone_symbol(Tone::Good),
            tone_symbol(Tone::Info),
            tone_symbol(Tone::Warning),
            tone_symbol(Tone::Danger),
        ];
        for i in 0..symbols.len() {
            for j in (i + 1)..symbols.len() {
                assert_ne!(symbols[i], symbols[j]);
            }
        }
    }
}
