//! Window discovery and manipulation via the Accessibility APIs (PRD §7,
//! §13, §23).
#![allow(deprecated)] // `cocoa` is deprecated upstream in favor of objc2-app-kit.
#![allow(unexpected_cfgs)] // `objc`'s msg_send!/class! macros check a `cargo-clippy` cfg we don't set.

use std::ffi::c_void;

use accessibility::{AXAttribute, AXUIElement, AXUIElementAttributes};
use accessibility_sys::{
    AXValueCreate, AXValueGetValue, AXValueRef, AXValueType, kAXValueTypeCGPoint,
    kAXValueTypeCGSize, pid_t,
};
use anyhow::{Result, anyhow};
use cocoa::base::id;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::CFString;
use core_graphics::geometry::{CGPoint, CGSize};
use core_graphics::window::{
    copy_window_info, kCGWindowBounds, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly, kCGWindowOwnerPID,
};
use objc::{class, msg_send, sel, sel_impl};

use crate::layout::Rect;

pub struct Window {
    element: AXUIElement,
}

fn position_attr() -> AXAttribute<CFType> {
    AXAttribute::new(&CFString::from_static_string("AXPosition"))
}

fn size_attr() -> AXAttribute<CFType> {
    AXAttribute::new(&CFString::from_static_string("AXSize"))
}

impl Window {
    /// The currently focused application's focused/main window (PRD §7).
    ///
    /// Deliberately does not query the system-wide element's
    /// `AXFocusedApplication` attribute: that call reliably returns
    /// `kAXErrorCannotComplete` for plain (non-bundled) CLI processes even
    /// when Accessibility is trusted. Asking `NSWorkspace` for the
    /// frontmost app's pid and then querying that app's `AXUIElement`
    /// avoids the flaky call entirely.
    pub fn focused() -> Result<Window> {
        let debug = std::env::var_os("SNAP_DEBUG").is_some();

        let pid = frontmost_app_pid().ok_or_else(|| {
            if debug {
                eprintln!("[snap debug] NSWorkspace reported no frontmost application");
            }
            anyhow!("no focused window")
        })?;

        let app = AXUIElement::application(pid);
        let window = app
            .focused_window()
            .or_else(|e1| {
                app.main_window().map_err(|e2| {
                    if debug {
                        eprintln!(
                            "[snap debug] pid {pid}: focused_window failed: {e1:?}; main_window failed: {e2:?}"
                        );
                    }
                    e2
                })
            })
            .map_err(|_| anyhow!("no focused window"))?;

        Ok(Window { element: window })
    }

    pub fn rect(&self) -> Result<Rect> {
        let position: CGPoint = self.get_ax_value(&position_attr(), kAXValueTypeCGPoint)?;
        let size: CGSize = self.get_ax_value(&size_attr(), kAXValueTypeCGSize)?;
        Ok(Rect::new(position.x, position.y, size.width, size.height))
    }

    /// Requests the given geometry. Applications may adjust the exact
    /// values (minimum/maximum window sizes); that is not itself an error
    /// (PRD §23).
    pub fn set_rect(&self, rect: Rect) -> Result<()> {
        self.set_ax_value(
            &position_attr(),
            kAXValueTypeCGPoint,
            &CGPoint::new(rect.x, rect.y),
        )?;
        self.set_ax_value(
            &size_attr(),
            kAXValueTypeCGSize,
            &CGSize::new(rect.width, rect.height),
        )?;
        Ok(())
    }

    fn get_ax_value<T: Copy + Default>(
        &self,
        attr: &AXAttribute<CFType>,
        value_type: AXValueType,
    ) -> Result<T> {
        let value = self
            .element
            .attribute(attr)
            .map_err(|_| anyhow!("window cannot be resized"))?;
        let value_ref = value.as_concrete_TypeRef() as *mut c_void as AXValueRef;
        let mut out = T::default();
        let ok =
            unsafe { AXValueGetValue(value_ref, value_type, &mut out as *mut T as *mut c_void) };
        if ok {
            Ok(out)
        } else {
            Err(anyhow!("window cannot be resized"))
        }
    }

    fn set_ax_value<T>(
        &self,
        attr: &AXAttribute<CFType>,
        value_type: AXValueType,
        value: &T,
    ) -> Result<()> {
        let ax_value = unsafe { AXValueCreate(value_type, value as *const T as *const c_void) };
        if ax_value.is_null() {
            return Err(anyhow!("window cannot be resized"));
        }
        let wrapped =
            unsafe { CFType::wrap_under_create_rule(ax_value as core_foundation::base::CFTypeRef) };
        self.element
            .set_attribute(attr, wrapped)
            .map_err(|_| anyhow!("window cannot be resized"))
    }
}

/// A visible, tileable window on the given display, ordered top-to-bottom
/// then left-to-right (PRD §15), excluding the ones listed in PRD §13.
pub struct TileCandidate {
    pub window: Window,
    pub rect: Rect,
}

/// Enumerates normal, visible, resizable application windows overlapping
/// `display_frame`. Skips minimized/hidden/utility windows and anything
/// Accessibility can't manipulate; unmanageable windows are simply skipped
/// rather than failing the whole operation (PRD §13, §23).
///
/// Sources candidates from `CGWindowListCopyWindowInfo` with
/// `kCGWindowListOptionOnScreenOnly` rather than walking each app's full
/// `AXUIElement` window list: the AX list includes windows regardless of
/// Stage Manager grouping, Spaces, or minimization (they still report a
/// stale on-screen-range position), which silently pulled in windows from
/// other Stage Manager stages. The on-screen window list is exactly what's
/// currently compositing to the display, so it respects all of that.
pub fn visible_windows_on(display_frame: Rect) -> Result<Vec<TileCandidate>> {
    let debug = std::env::var_os("SNAP_DEBUG").is_some();
    let infos = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        0,
    )
    .ok_or_else(|| anyhow!("failed to enumerate windows"))?;

    let mut candidates = Vec::new();
    for ptr in infos.get_all_values() {
        let dict: CFDictionary =
            unsafe { CFDictionary::wrap_under_get_rule(ptr as CFDictionaryRef) };

        // Layer 0 is a normal window; menu bar items, the Dock, etc. use
        // other layers and aren't tileable application windows.
        if dict_i64(&dict, unsafe { kCGWindowLayer } as *const c_void) != Some(0) {
            continue;
        }
        let Some(pid) = dict_i64(&dict, unsafe { kCGWindowOwnerPID } as *const c_void) else {
            continue;
        };
        let Some(cg_bounds) = dict_bounds(&dict) else {
            continue;
        };
        if overlap_area(cg_bounds, display_frame) <= 0.0 {
            continue;
        }

        let app = AXUIElement::application(pid as pid_t);
        let Ok(windows) = app.windows() else { continue };
        let Some(element) = windows.iter().find(|w| {
            let window = Window {
                element: (**w).clone(),
            };
            matches!(window.rect(), Ok(r) if rects_roughly_equal(r, cg_bounds))
        }) else {
            if debug {
                eprintln!(
                    "[snap debug] on-screen window at {cg_bounds:?} (pid {pid}) has no matching AXUIElement, skipping"
                );
            }
            continue;
        };

        if !is_tileable(&element) {
            continue;
        }
        candidates.push(TileCandidate {
            window: Window {
                element: (*element).clone(),
            },
            rect: cg_bounds,
        });
    }

    candidates.sort_by(|a, b| {
        a.rect
            .y
            .partial_cmp(&b.rect.y)
            .unwrap()
            .then(a.rect.x.partial_cmp(&b.rect.x).unwrap())
    });
    Ok(candidates)
}

fn rects_roughly_equal(a: Rect, b: Rect) -> bool {
    const EPS: f64 = 2.0;
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.width - b.width).abs() < EPS
        && (a.height - b.height).abs() < EPS
}

fn dict_i64(dict: &CFDictionary, key: *const c_void) -> Option<i64> {
    let value_ptr = dict.find(key)?;
    let number = unsafe { CFNumber::wrap_under_get_rule(*value_ptr as CFNumberRef) };
    number.to_i64()
}

fn dict_bounds(dict: &CFDictionary) -> Option<Rect> {
    let bounds_ptr = dict.find(unsafe { kCGWindowBounds } as *const c_void)?;
    let bounds_dict: CFDictionary =
        unsafe { CFDictionary::wrap_under_get_rule(*bounds_ptr as CFDictionaryRef) };
    let x = dict_f64_by_str(&bounds_dict, "X")?;
    let y = dict_f64_by_str(&bounds_dict, "Y")?;
    let width = dict_f64_by_str(&bounds_dict, "Width")?;
    let height = dict_f64_by_str(&bounds_dict, "Height")?;
    Some(Rect::new(x, y, width, height))
}

fn dict_f64_by_str(dict: &CFDictionary, key: &str) -> Option<f64> {
    let key = CFString::new(key);
    let value_ptr = dict.find(key.as_concrete_TypeRef() as *const c_void)?;
    let number = unsafe { CFNumber::wrap_under_get_rule(*value_ptr as CFNumberRef) };
    number.to_f64()
}

fn is_tileable(element: &AXUIElement) -> bool {
    if matches!(element.minimized(), Ok(m) if m == CFBoolean::true_value()) {
        return false;
    }
    match element.subrole() {
        Ok(subrole) => subrole == "AXStandardWindow",
        Err(_) => false,
    }
}

fn overlap_area(a: Rect, b: Rect) -> f64 {
    let x_overlap = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let y_overlap = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    x_overlap.max(0.0) * y_overlap.max(0.0)
}

fn frontmost_app_pid() -> Option<pid_t> {
    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: id = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: pid_t = msg_send![app, processIdentifier];
        Some(pid)
    }
}
