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

/// Every active display in a stable order: left-to-right, then
/// top-to-bottom, tie-broken by the original `NSScreen.screens()` array
/// index (index 0 is guaranteed to be the menu-bar display). Used by
/// `snap display next/previous/N` so the ordering is documented and doesn't
/// silently change between two invocations.
pub fn ordered_displays(stage_manager_width: f64) -> Result<Vec<Display>> {
    Ok(order_displays(all_displays(stage_manager_width)?))
}

fn order_displays(displays: Vec<Display>) -> Vec<Display> {
    let mut indexed: Vec<(usize, Display)> = displays.into_iter().enumerate().collect();
    indexed.sort_by(|(ia, a), (ib, b)| {
        a.frame
            .x
            .partial_cmp(&b.frame.x)
            .unwrap()
            .then(a.frame.y.partial_cmp(&b.frame.y).unwrap())
            .then(ia.cmp(ib))
    });
    indexed.into_iter().map(|(_, d)| d).collect()
}

/// Index into `displays` (as returned by [`ordered_displays`]) of the one
/// containing the largest portion of `window_rect`. Ties keep the
/// lower/earlier index. Falls back to `0` if `window_rect` overlaps nothing
/// (e.g. it's positioned fully off-screen) — `displays` is never empty in
/// practice since `all_displays` errors instead of returning zero.
pub fn display_index_containing(displays: &[Display], window_rect: Rect) -> usize {
    let mut best_index = 0;
    let mut best_overlap = -1.0;
    for (i, d) in displays.iter().enumerate() {
        let overlap = overlap_area(d.frame, window_rect);
        if overlap > best_overlap {
            best_overlap = overlap;
            best_index = i;
        }
    }
    best_index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(x: f64, y: f64, w: f64, h: f64) -> Display {
        Display {
            frame: Rect::new(x, y, w, h),
            usable: Rect::new(x, y, w, h),
        }
    }

    #[test]
    fn order_displays_sorts_left_to_right() {
        let left = display(0.0, 0.0, 1000.0, 800.0);
        let right = display(1000.0, 0.0, 1000.0, 800.0);
        let ordered = order_displays(vec![right, left]);
        assert_eq!(ordered[0].frame.x, 0.0);
        assert_eq!(ordered[1].frame.x, 1000.0);
    }

    #[test]
    fn order_displays_handles_negative_origin() {
        let a = display(-1920.0, 0.0, 1920.0, 1080.0);
        let b = display(0.0, 0.0, 1728.0, 1117.0);
        let ordered = order_displays(vec![b, a]);
        assert_eq!(ordered[0].frame.x, -1920.0);
        assert_eq!(ordered[1].frame.x, 0.0);
    }

    #[test]
    fn order_displays_breaks_ties_by_original_index() {
        let a = display(0.0, 0.0, 1000.0, 800.0);
        let b = display(0.0, 0.0, 1000.0, 800.0);
        let ordered = order_displays(vec![a, b]);
        // Same frame: original order (index 0 first) is preserved.
        assert_eq!(ordered.len(), 2);
    }

    #[test]
    fn display_index_containing_picks_largest_overlap() {
        let displays = vec![
            display(0.0, 0.0, 1000.0, 800.0),
            display(1000.0, 0.0, 1000.0, 800.0),
        ];
        let window = Rect::new(1200.0, 0.0, 400.0, 400.0);
        assert_eq!(display_index_containing(&displays, window), 1);
    }

    #[test]
    fn display_index_containing_falls_back_to_zero_when_off_screen() {
        let displays = vec![
            display(0.0, 0.0, 1000.0, 800.0),
            display(1000.0, 0.0, 1000.0, 800.0),
        ];
        let window = Rect::new(-5000.0, -5000.0, 100.0, 100.0);
        assert_eq!(display_index_containing(&displays, window), 0);
    }
}
