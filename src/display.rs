//! Display discovery and usable-bounds calculation (PRD §6, §17, §24).
//!
//! `NSScreen` reports geometry in Cocoa's coordinate space: origin at the
//! bottom-left of the primary screen, y increasing upward. The Accessibility
//! APIs (`AXPosition`/`AXSize`) use the Quartz/CG space: origin at the
//! top-left of the primary screen, y increasing downward. Every rect
//! returned from here is already converted into that CG space so the rest
//! of the app only ever deals with one coordinate system.
//!
//! Uses the `cocoa` crate (deprecated upstream in favor of `objc2-app-kit`)
//! since `accessibility` already pulls it in transitively.
#![allow(deprecated)]

use anyhow::{Result, bail};
use cocoa::appkit::NSScreen;
use cocoa::base::{id, nil};
use cocoa::foundation::NSArray;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::base::Boolean;
use core_foundation_sys::preferences::CFPreferencesGetAppBooleanValue;
use core_foundation_sys::string::CFStringRef;

use crate::layout::Rect;

pub struct Display {
    /// Full display bounds, CG coordinates.
    pub frame: Rect,
    /// Bounds excluding menu bar, Dock, and (when enabled) the Stage Manager
    /// strip, CG coordinates.
    pub usable: Rect,
}

/// Enumerates every active display. `screens()[0]` is guaranteed by AppKit
/// to be the display holding the menu bar, which is also the reference
/// display for the CG coordinate space's origin.
///
/// `NSScreen.visibleFrame` excludes the menu bar and Dock but NOT the Stage
/// Manager strip — Apple doesn't treat it as reserved screen real estate, so
/// windows we resize can end up covering it. When Stage Manager is globally
/// enabled, `stage_manager_width` is reserved on the left edge of every
/// display's usable area to keep it clear.
pub fn all_displays(stage_manager_width: f64) -> Result<Vec<Display>> {
    let reserve = if stage_manager_width > 0.0 && stage_manager_enabled() {
        stage_manager_width
    } else {
        0.0
    };

    unsafe {
        let screens = NSScreen::screens(nil);
        let count = screens.count();
        if count == 0 {
            bail!("no displays found");
        }

        let primary_height = NSScreen::frame(screens.objectAtIndex(0)).size.height;

        let mut displays = Vec::with_capacity(count as usize);
        for i in 0..count {
            let screen: id = screens.objectAtIndex(i);
            let frame = cocoa_to_cg(NSScreen::frame(screen), primary_height);
            let mut usable = cocoa_to_cg(NSScreen::visibleFrame(screen), primary_height);
            usable.x += reserve;
            usable.width -= reserve;
            displays.push(Display { frame, usable });
        }
        Ok(displays)
    }
}

/// Reads `GloballyEnabled` from the `com.apple.WindowManager` preference
/// domain — the same flag System Settings → Desktop & Dock → Stage Manager
/// toggles (verified via `defaults read com.apple.WindowManager`).
fn stage_manager_enabled() -> bool {
    unsafe {
        let key = CFString::from_static_string("GloballyEnabled");
        let app_id = CFString::from_static_string("com.apple.WindowManager");
        let mut key_exists: Boolean = 0;
        let value = CFPreferencesGetAppBooleanValue(
            key.as_concrete_TypeRef() as CFStringRef,
            app_id.as_concrete_TypeRef() as CFStringRef,
            &mut key_exists,
        );
        key_exists != 0 && value != 0
    }
}

fn cocoa_to_cg(r: cocoa::foundation::NSRect, primary_height: f64) -> Rect {
    Rect::new(
        r.origin.x,
        primary_height - (r.origin.y + r.size.height),
        r.size.width,
        r.size.height,
    )
}

fn overlap_area(a: Rect, b: Rect) -> f64 {
    let x_overlap = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let y_overlap = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    x_overlap.max(0.0) * y_overlap.max(0.0)
}

/// The display containing the largest portion of `window_rect` (PRD §7).
/// Falls back to the primary display if the window doesn't overlap any
/// display (e.g. it's positioned fully off-screen).
pub fn target_display_for(window_rect: Rect, stage_manager_width: f64) -> Result<Display> {
    let displays = all_displays(stage_manager_width)?;
    let best_index = (0..displays.len())
        .max_by(|&a, &b| {
            overlap_area(displays[a].frame, window_rect)
                .partial_cmp(&overlap_area(displays[b].frame, window_rect))
                .unwrap()
                .then(b.cmp(&a))
        })
        .expect("all_displays returns at least one display or errors");
    Ok(displays.into_iter().nth(best_index).unwrap())
}
