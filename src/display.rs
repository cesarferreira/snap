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

use crate::layout::Rect;

pub struct Display {
    /// Full display bounds, CG coordinates.
    pub frame: Rect,
    /// Bounds excluding menu bar and Dock, CG coordinates.
    pub usable: Rect,
}

/// Enumerates every active display. `screens()[0]` is guaranteed by AppKit
/// to be the display holding the menu bar, which is also the reference
/// display for the CG coordinate space's origin.
pub fn all_displays() -> Result<Vec<Display>> {
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
            let usable = cocoa_to_cg(NSScreen::visibleFrame(screen), primary_height);
            displays.push(Display { frame, usable });
        }
        Ok(displays)
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
pub fn target_display_for(window_rect: Rect) -> Result<Display> {
    let displays = all_displays()?;
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
