//! Native macOS Liquid Glass (`NSGlassEffectView` on macOS 26+, `NSVisualEffectView` fallback on
//! macOS 14–15) and lightweight AppKit bridges (status item positioning, accessory policy,
//! native save/alert dialogs, and offscreen window capture) for GPUI windows.
use crate::i18n::{raw_nsstring as raw_text, translated_nsstring as text};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSDictionary, NSPoint, NSRect, NSSize, NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::path::{Path, PathBuf};

/// Visual style requested for a GPUI window's native backdrop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlassStyle {
    /// Standard adaptive Liquid Glass (macOS 26+ `NSGlassEffectViewStyleRegular`).
    Regular,
    /// Clearer translucent Liquid Glass (macOS 26+ `NSGlassEffectViewStyleClear`).
    #[allow(dead_code)]
    Clear,
}

/// Returns `true` when the running macOS exposes Apple's native `NSGlassEffectView` (macOS 26+).
pub fn native_liquid_glass_available() -> bool {
    AnyClass::get(c"NSGlassEffectView").is_some()
}

/// Switches the process to a menu-bar accessory app (no Dock icon) after GPUI finishes launching.
pub fn set_accessory_policy() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

/// Brings our application windows to the foreground (required for accessory apps).
pub fn activate_app() {
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

/// Extracts the raw `(NSView, NSWindow)` pointers behind a GPUI `Window`.
fn raw_ns_handles(window: &gpui::Window) -> Option<(*mut AnyObject, *mut AnyObject)> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let ns_view = appkit.ns_view.as_ptr() as *mut AnyObject;
    if ns_view.is_null() {
        return None;
    }
    let ns_window: *mut AnyObject = unsafe { msg_send![ns_view, window] };
    if ns_window.is_null() {
        None
    } else {
        Some((ns_view, ns_window))
    }
}

fn raw_ns_window(window: &gpui::Window) -> Option<*mut AnyObject> {
    raw_ns_handles(window).map(|(_, w)| w)
}

/// Syncs GPUI Kit's theme with macOS appearance while keeping the Root surface fully
/// transparent so `NSGlassEffectView` / `NSVisualEffectView` Liquid Glass shines through.
pub fn sync_glass_root_theme(window: &mut gpui::Window, cx: &mut gpui::App) {
    gpui_component::Theme::sync_system_appearance(Some(window), cx);
    gpui_component::Theme::global_mut(cx).tokens.background = gpui::transparent_black().into();
}

/// Attaches a real macOS 26+ `NSGlassEffectView` (or `NSVisualEffectView` on macOS 14–15)
/// behind GPUI's transparent Metal hosting view so the window gets true live Liquid Glass
/// refraction and adaptive luminosity.
pub fn attach_liquid_glass(window: &gpui::Window, corner_radius: f64, style: GlassStyle) {
    let Some((ns_view, ns_window)) = raw_ns_handles(window) else {
        return;
    };
    unsafe {
        let content_view: *mut AnyObject = msg_send![ns_window, contentView];
        if content_view.is_null() {
            return;
        }
        // Only mount relative to `ns_view` when it is still a direct subview of `content_view`.
        let superview: *mut AnyObject = msg_send![ns_view, superview];
        let relative_to = if superview == content_view {
            ns_view
        } else {
            std::ptr::null_mut()
        };

        let _: () = msg_send![ns_window, setOpaque: false];
        let _: () = msg_send![ns_window, setHasShadow: true];

        let bounds: NSRect = msg_send![content_view, bounds];
        let mask: usize = 2 | 16; // NSViewWidthSizable | NSViewHeightSizable
        let below: isize = -1; // NSWindowBelow

        if let Some(glass_class) = AnyClass::get(c"NSGlassEffectView") {
            let alloc: *mut AnyObject = msg_send![glass_class, alloc];
            let glass: *mut AnyObject = msg_send![alloc, initWithFrame: bounds];
            if !glass.is_null() {
                let _: () = msg_send![glass, setAutoresizingMask: mask];
                if corner_radius > 0.0 {
                    let _: () = msg_send![glass, setCornerRadius: corner_radius];
                }
                let raw_style: isize = match style {
                    GlassStyle::Regular => 0,
                    GlassStyle::Clear => 1,
                };
                let _: () = msg_send![glass, setStyle: raw_style];
                let _: () = msg_send![
                    content_view,
                    addSubview: glass,
                    positioned: below,
                    relativeTo: relative_to
                ];
                let _: () = msg_send![glass, release];
            }
        } else if let Some(vev_class) = AnyClass::get(c"NSVisualEffectView") {
            let alloc: *mut AnyObject = msg_send![vev_class, alloc];
            let vev: *mut AnyObject = msg_send![alloc, initWithFrame: bounds];
            if !vev.is_null() {
                let _: () = msg_send![vev, setAutoresizingMask: mask];
                // NSVisualEffectMaterial::Popover = 6, Sidebar = 7
                let material: isize = match style {
                    GlassStyle::Clear => 6,
                    GlassStyle::Regular => 7,
                };
                let _: () = msg_send![vev, setMaterial: material];
                // NSVisualEffectBlendingMode::BehindWindow = 0
                let _: () = msg_send![vev, setBlendingMode: 0isize];
                // NSVisualEffectState::Active = 1
                let _: () = msg_send![vev, setState: 1isize];
                if corner_radius > 0.0 {
                    let _: () = msg_send![vev, setWantsLayer: true];
                    let layer: *mut AnyObject = msg_send![vev, layer];
                    if !layer.is_null() {
                        let _: () = msg_send![layer, setCornerRadius: corner_radius];
                        let _: () = msg_send![layer, setMasksToBounds: true];
                    }
                }
                let _: () = msg_send![
                    content_view,
                    addSubview: vev,
                    positioned: below,
                    relativeTo: relative_to
                ];
                let _: () = msg_send![vev, release];
            }
        }
    }
}

/// Configures a GPUI `WindowKind::PopUp` panel as a Liquid Glass floating menu-bar panel
/// without mutating `styleMask` after GPUI initializes `GPUIPanel` / `GPUIView`.
pub fn configure_popover_window(
    window: &gpui::Window,
    anchor_rect: Option<NSRect>,
    width: f64,
    height: f64,
) {
    if let Some(ns_window) = raw_ns_window(window) {
        unsafe {
            // Keep GPUI's initialized `GPUIPanel` styleMask intact so AppKit never rebuilds
            // the contentView hierarchy or breaks GPUIView / CAMetalLayer tracking.
            let _: () = msg_send![ns_window, setOpaque: false];
            let _: () = msg_send![ns_window, setHasShadow: true];
            // NSStatusWindowLevel (25) keeps the panel directly attached to the menu bar.
            let _: () = msg_send![ns_window, setLevel: 25isize];
            // CanJoinAllSpaces (1<<0) | Transient (1<<3) | FullScreenAuxiliary (1<<8)
            let behavior: usize = (1 << 0) | (1 << 3) | (1 << 8);
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
            let _: () = msg_send![ns_window, setMovable: false];
            let _: () = msg_send![ns_window, setReleasedWhenClosed: false];

            if let Some(mtm) = MainThreadMarker::new() {
                position_popover_nswindow(mtm, ns_window, anchor_rect, width, height);
            }
        }
    }
    // Let the native panel mask clip corners cleanly (0.0 avoids mismatched double corners).
    attach_liquid_glass(window, 0.0, GlassStyle::Regular);
}

/// Updates the floating menu bar panel's position and height while keeping its top edge anchored.
pub fn position_popover_window(
    window: &gpui::Window,
    anchor_rect: Option<NSRect>,
    width: f64,
    height: f64,
) {
    let Some(ns_window) = raw_ns_window(window) else {
        return;
    };
    if let Some(mtm) = MainThreadMarker::new() {
        unsafe {
            position_popover_nswindow(mtm, ns_window, anchor_rect, width, height);
        }
    }
}

unsafe fn position_popover_nswindow(
    mtm: MainThreadMarker,
    ns_window: *mut AnyObject,
    anchor_rect: Option<NSRect>,
    width: f64,
    height: f64,
) {
    // Pick the screen that contains the menu bar status item (crucial for multi-display setups).
    let visible = anchor_rect
        .and_then(|anchor| {
            let cx = anchor.origin.x + anchor.size.width / 2.0;
            let cy = anchor.origin.y + anchor.size.height / 2.0;
            NSScreen::screens(mtm).iter().find_map(|s| {
                let f = s.frame();
                let in_x = cx >= f.origin.x && cx <= f.origin.x + f.size.width;
                let in_y = cy >= f.origin.y && cy <= f.origin.y + f.size.height;
                (in_x && in_y).then(|| s.visibleFrame())
            })
        })
        .or_else(|| NSScreen::mainScreen(mtm).map(|s| s.visibleFrame()))
        .unwrap_or_else(|| NSRect::new(NSPoint::new(0., 0.), NSSize::new(1440., 900.)));

    let clamped_h = height.min((visible.size.height - 16.0).max(320.0));

    let (x, y) = if let Some(anchor) = anchor_rect {
        let centered_x = anchor.origin.x + anchor.size.width / 2.0 - width / 2.0;
        let clamped_x = centered_x
            .max(visible.origin.x + 8.0)
            .min(visible.origin.x + visible.size.width - width - 8.0);
        let top_y = (anchor.origin.y - 6.0).min(visible.origin.y + visible.size.height - 4.0);
        let clamped_y = (top_y - clamped_h).max(visible.origin.y + 8.0);
        (clamped_x, clamped_y)
    } else {
        let right_x = (visible.origin.x + visible.size.width - width - 16.0).max(visible.origin.x);
        let top_y =
            (visible.origin.y + visible.size.height - clamped_h - 8.0).max(visible.origin.y);
        (right_x, top_y)
    };

    let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(width, clamped_h));
    let _: () = msg_send![ns_window, setFrame: frame, display: true];
}

/// Returns the on-screen frame of an `NSStatusItem` button in AppKit screen coordinates.
pub fn status_item_screen_rect(item: &NSStatusItem) -> Option<NSRect> {
    let mtm = MainThreadMarker::new()?;
    let button = item.button(mtm)?;
    let window = button.window()?;
    let frame_in_window = button.convertRect_toView(button.bounds(), None);
    Some(window.convertRectToScreen(frame_in_window))
}

/// Returns whether the current global cursor position lies inside the given status item rect.
pub fn cursor_inside_rect(rect: NSRect) -> bool {
    let pt = NSEvent::mouseLocation();
    let pad = 4.0;
    pt.x >= rect.origin.x - pad
        && pt.x <= rect.origin.x + rect.size.width + pad
        && pt.y >= rect.origin.y - pad
        && pt.y <= rect.origin.y + rect.size.height + pad
}

/// Forces a GPUI window's native macOS appearance (for light/dark diagnostic renders).
pub fn set_window_appearance(window: &gpui::Window, dark: bool) {
    let Some(ns_window) = raw_ns_window(window) else {
        return;
    };
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    if let Some(appearance) = NSAppearance::appearanceNamed(name) {
        unsafe {
            let _: () = msg_send![ns_window, setAppearance: &*appearance];
        }
    }
}

/// Hides a GPUI window without destroying it.
pub fn order_out_window(window: &gpui::Window) {
    if let Some(ns_window) = raw_ns_window(window) {
        unsafe {
            let _: () = msg_send![ns_window, orderOut: std::ptr::null::<AnyObject>()];
        }
    }
}

/// Shows and focuses a GPUI window.
pub fn order_front_window(window: &gpui::Window) {
    if let Some(ns_window) = raw_ns_window(window) {
        unsafe {
            let _: () = msg_send![ns_window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
    }
}

/// Returns whether a GPUI window's native `NSWindow` is currently visible on screen.
pub fn is_window_visible(window: &gpui::Window) -> bool {
    let Some(ns_window) = raw_ns_window(window) else {
        return false;
    };
    unsafe { msg_send![ns_window, isVisible] }
}

/// Captures a GPUI window (including Metal GPU layers and `NSGlassEffectView` Liquid Glass)
/// into a PNG file for visual verification. Uses `screencapture -l <windowID>` when desktop
/// capture is available, falling back to AppKit bitmap caching on headless runners.
pub fn capture_window_png(window: &gpui::Window, path: &Path) -> Result<(), String> {
    let ns_window = raw_ns_window(window).ok_or("missing native NSWindow")?;
    let window_num: isize = unsafe { msg_send![ns_window, windowNumber] };
    if window_num > 0 {
        if let Ok(status) = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x", "-o", "-l", &window_num.to_string()])
            .arg(path)
            .status()
        {
            if status.success()
                && path.exists()
                && std::fs::metadata(path).is_ok_and(|m| m.len() > 0)
            {
                return Ok(());
            }
        }
    }
    unsafe {
        let content_view: *mut NSView = msg_send![ns_window, contentView];
        if content_view.is_null() {
            return Err("missing contentView".into());
        }
        let view = &*content_view;
        let bounds = view.bounds();
        let bitmap = view
            .bitmapImageRepForCachingDisplayInRect(bounds)
            .ok_or("failed to allocate bitmap")?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
        let data = bitmap
            .representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
            .ok_or("failed to encode PNG")?;
        std::fs::write(path, data.to_vec()).map_err(|e| format!("write PNG: {e}"))
    }
}

/// Native macOS Save Panel for exporting sanitized diagnostics JSON.
pub fn save_diagnostics_dialog() -> Option<PathBuf> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSSavePanel::savePanel(mtm);
    panel.setNameFieldStringValue(&text("FanControl-diagnostics.json"));
    if panel.runModal() == NSModalResponseOK {
        panel
            .URL()
            .and_then(|url| url.path())
            .map(|p| PathBuf::from(p.to_string()))
    } else {
        None
    }
}

/// Native confirmation dialog before removing the privileged root helper.
pub fn confirm_uninstall_dialog() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&text("移除风扇控制服务？"));
    alert.setInformativeText(&text(
        "应用将先停止自定义控制并交还系统，再请求管理员授权。移除后仍可查看温度，控制设置和诊断日志会保留。",
    ));
    alert.addButtonWithTitle(&text("移除服务"));
    alert.addButtonWithTitle(&text("取消"));
    alert.runModal() == NSAlertFirstButtonReturn
}

/// Native warning alert when fan handback before quitting could not be confirmed.
pub fn show_unconfirmed_handback_alert() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&text("退出前交还未获确认"));
    alert.setInformativeText(&text(
        "控制服务会继续重试。请检查服务状态与风扇实际模式；应用无法确认安全回退已经完成。",
    ));
    alert.addButtonWithTitle(&text("退出"));
    alert.runModal();
}

/// Opens the README help page in the default browser.
pub fn open_help_url() {
    if let Some(url) =
        NSURL::URLWithString(&raw_text("https://github.com/sm-yjr/fan-control#readme"))
    {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// Displays macOS's standard About panel with Fan Control's bundle metadata.
pub fn show_about_panel() {
    if let Some(mtm) = MainThreadMarker::new() {
        activate_app();
        NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
    }
}

/// Saves a string preference in `NSUserDefaults` without touching `config.json`.
pub fn set_user_default(key: &str, value: &str) {
    unsafe {
        objc2_foundation::NSUserDefaults::standardUserDefaults()
            .setObject_forKey(Some(&NSString::from_str(value)), &NSString::from_str(key));
    }
}

/// Reads a bool preference from `NSUserDefaults`.
pub fn get_bool_default(key: &str) -> bool {
    objc2_foundation::NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str(key))
}

/// Writes a bool preference into `NSUserDefaults`.
pub fn set_bool_default(key: &str, value: bool) {
    objc2_foundation::NSUserDefaults::standardUserDefaults()
        .setBool_forKey(value, &NSString::from_str(key));
}

/// Keeps `NSApplication` menu bar shortcuts (⌘Q / ⌘, / ⌘R) alive inside GPUI.
pub fn main_bundle_path() -> String {
    objc2_foundation::NSBundle::mainBundle()
        .bundlePath()
        .to_string()
}

/// Retains an Objective-C object handle safely for status bar target wiring.
#[allow(dead_code)]
pub fn retain_any(obj: &Retained<NSStatusItem>) -> Retained<NSStatusItem> {
    obj.clone()
}
