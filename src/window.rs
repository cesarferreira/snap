//! Window discovery and manipulation via the Accessibility APIs (PRD §7,
//! §13, §23).

use std::ffi::c_void;

use accessibility_sys::{
    AXValueCreate, AXValueGetValue, AXValueRef, AXValueType, kAXFocusedWindowAttribute,
    kAXMainWindowAttribute, kAXMinimizedAttribute, kAXPositionAttribute, kAXRaiseAction,
    kAXSizeAttribute, kAXStandardWindowSubrole, kAXSubroleAttribute, kAXValueTypeCGPoint,
    kAXValueTypeCGSize, kAXWindowsAttribute, pid_t,
};
use anyhow::{Result, anyhow};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGSize};
use core_graphics::window::{
    copy_window_info, kCGWindowBounds, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly, kCGWindowName, kCGWindowNumber, kCGWindowOwnerName,
    kCGWindowOwnerPID,
};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

use crate::ax::AXUIElement;
use crate::layout::Rect;

pub struct Window {
    element: AXUIElement,
}

#[derive(Clone, Copy)]
pub(crate) struct RectChange {
    position: bool,
    size: bool,
}

impl RectChange {
    pub(crate) fn is_empty(self) -> bool {
        !self.position && !self.size
    }
}

fn position_attr() -> CFString {
    CFString::from_static_string(kAXPositionAttribute)
}

fn size_attr() -> CFString {
    CFString::from_static_string(kAXSizeAttribute)
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
            .attribute::<AXUIElement>(&CFString::from_static_string(kAXFocusedWindowAttribute))
            .or_else(|e1| {
                app.attribute::<AXUIElement>(&CFString::from_static_string(
                    kAXMainWindowAttribute,
                ))
                .inspect_err(|e2| {
                    if debug {
                        eprintln!(
                            "[snap debug] pid {pid}: focused_window failed: {e1:?}; main_window failed: {e2:?}"
                        );
                    }
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
    /// (PRD §23). But if the window genuinely can't be moved/resized to
    /// where it needs to go, this makes no change at all rather than
    /// applying half of it — e.g. a fixed-size window like Calculator's
    /// would otherwise get moved into position and then fail to resize,
    /// leaving it in a half-applied, worse-looking state than before.
    pub fn set_rect(&self, rect: Rect) -> Result<()> {
        let change = self.prepare_rect(rect)?;
        self.apply_rect(rect, change)
    }

    /// Validates a requested frame once before an animation begins. Repeating
    /// these AX queries for every rendered frame is both wasteful and prone to
    /// stalling applications that service Accessibility messages slowly.
    pub(crate) fn prepare_rect(&self, rect: Rect) -> Result<RectChange> {
        let debug = std::env::var_os("SNAP_DEBUG").is_some();
        let current = self.rect()?;
        let needs_position = !points_equal(current.x, rect.x) || !points_equal(current.y, rect.y);
        let needs_size =
            !points_equal(current.width, rect.width) || !points_equal(current.height, rect.height);

        let position_settable = self.element.is_settable(&position_attr()).unwrap_or(false);
        let size_settable = self.element.is_settable(&size_attr()).unwrap_or(false);
        if debug {
            eprintln!(
                "[snap debug] needs_position={needs_position} (settable={position_settable}) needs_size={needs_size} (settable={size_settable})"
            );
        }
        if (needs_position && !position_settable) || (needs_size && !size_settable) {
            return Err(anyhow!("window cannot be resized"));
        }

        Ok(RectChange {
            position: needs_position,
            size: needs_size,
        })
    }

    /// Applies a frame after [`Self::prepare_rect`] has established which AX
    /// attributes are safe to write.
    pub(crate) fn apply_rect(&self, rect: Rect, change: RectChange) -> Result<()> {
        if change.position {
            self.set_ax_value(
                &position_attr(),
                kAXValueTypeCGPoint,
                &CGPoint::new(rect.x, rect.y),
            )?;
        }
        if change.size {
            self.set_ax_value(
                &size_attr(),
                kAXValueTypeCGSize,
                &CGSize::new(rect.width, rect.height),
            )?;
        }
        Ok(())
    }

    /// AX-raises this window within its application's z-order. Combine with
    /// [`activate_app`] to also bring the owning application frontmost, so
    /// the window actually becomes key (PRD: spatial focus, accordion).
    pub fn raise(&self) -> Result<()> {
        self.element
            .perform_action(&CFString::from_static_string(kAXRaiseAction))
            .map_err(|_| anyhow!("cannot raise window"))
    }

    fn get_ax_value<T: Copy + Default>(
        &self,
        attr: &CFString,
        value_type: AXValueType,
    ) -> Result<T> {
        let value = self
            .element
            .attribute_value(attr)
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

    fn set_ax_value<T>(&self, attr: &CFString, value_type: AXValueType, value: &T) -> Result<()> {
        let ax_value = unsafe { AXValueCreate(value_type, value as *const T as *const c_void) };
        if ax_value.is_null() {
            return Err(anyhow!("window cannot be resized"));
        }
        let wrapped =
            unsafe { CFType::wrap_under_create_rule(ax_value as core_foundation::base::CFTypeRef) };
        self.element
            .set_attribute(attr, &wrapped)
            .map_err(|_| anyhow!("window cannot be resized"))
    }
}

/// A visible, tileable window on the given display, ordered top-to-bottom
/// then left-to-right (PRD §15), excluding the ones listed in PRD §13.
pub struct TileCandidate {
    pub window: Window,
    pub rect: Rect,
    /// Owning application's pid.
    pub pid: pid_t,
    /// `kCGWindowOwnerName` — the owning application's name, as CGWindowList
    /// reports it (matches what Activity Monitor shows).
    pub app_name: String,
    /// `kCGWindowName` — the window's title. Often empty without Screen
    /// Recording permission (macOS withholds it since 10.15); best-effort.
    pub title: Option<String>,
    /// `kCGWindowNumber` — stable for the life of the window (until closed),
    /// unlike the rect-matching used to find its `AXUIElement`. The
    /// identity later features (e.g. undo) should key on, per its own issue.
    pub window_number: i64,
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

    // Cached per pid (avoids re-querying the same app's AX window list once
    // per on-screen window it owns) and paired with a used-index set: two
    // on-screen windows of the same app can land within `rects_roughly_equal`
    // of each other (e.g. `snap stack`'s cascade, where windows sit only a
    // small step apart), and without excluding already-claimed AX windows
    // here, both CGWindow entries would match the *same* AXUIElement —
    // silently collapsing two distinct windows into one candidate that's
    // moved twice instead of two windows moved once each.
    let mut ax_windows_by_pid: std::collections::HashMap<pid_t, Vec<AXUIElement>> =
        std::collections::HashMap::new();
    let mut used_by_pid: std::collections::HashMap<pid_t, std::collections::HashSet<usize>> =
        std::collections::HashMap::new();

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
        let pid = pid as pid_t;
        let Some(cg_bounds) = dict_bounds(&dict) else {
            continue;
        };
        if overlap_area(cg_bounds, display_frame) <= 0.0 {
            continue;
        }

        let ax_windows = ax_windows_by_pid.entry(pid).or_insert_with(|| {
            AXUIElement::application(pid)
                .attribute::<CFArray<AXUIElement>>(&CFString::from_static_string(
                    kAXWindowsAttribute,
                ))
                .map(|arr| arr.iter().map(|w| (*w).clone()).collect())
                .unwrap_or_default()
        });
        let used = used_by_pid.entry(pid).or_default();
        let Some(match_idx) = ax_windows.iter().enumerate().find_map(|(i, w)| {
            if used.contains(&i) {
                return None;
            }
            let window = Window { element: w.clone() };
            matches!(window.rect(), Ok(r) if rects_roughly_equal(r, cg_bounds)).then_some(i)
        }) else {
            if debug {
                eprintln!(
                    "[snap debug] on-screen window at {cg_bounds:?} (pid {pid}) has no matching AXUIElement, skipping"
                );
            }
            continue;
        };
        used.insert(match_idx);
        let element = ax_windows[match_idx].clone();

        if !is_tileable(&element) {
            continue;
        }
        let app_name =
            dict_string(&dict, unsafe { kCGWindowOwnerName } as *const c_void).unwrap_or_default();
        let title =
            dict_string(&dict, unsafe { kCGWindowName } as *const c_void).filter(|t| !t.is_empty());
        let window_number =
            dict_i64(&dict, unsafe { kCGWindowNumber } as *const c_void).unwrap_or(-1);
        candidates.push(TileCandidate {
            window: Window { element },
            rect: cg_bounds,
            pid,
            app_name,
            title,
            window_number,
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

fn dict_string(dict: &CFDictionary, key: *const c_void) -> Option<String> {
    let value_ptr = dict.find(key)?;
    let string = unsafe { CFString::wrap_under_get_rule(*value_ptr as CFStringRef) };
    Some(string.to_string())
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
    let minimized =
        element.attribute::<CFBoolean>(&CFString::from_static_string(kAXMinimizedAttribute));
    if matches!(minimized, Ok(m) if m == CFBoolean::true_value()) {
        return false;
    }
    match element.attribute::<CFString>(&CFString::from_static_string(kAXSubroleAttribute)) {
        Ok(subrole) => subrole == kAXStandardWindowSubrole,
        Err(_) => false,
    }
}

fn overlap_area(a: Rect, b: Rect) -> f64 {
    let x_overlap = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let y_overlap = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    x_overlap.max(0.0) * y_overlap.max(0.0)
}

fn points_equal(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0
}

pub fn frontmost_app_pid() -> Option<pid_t> {
    Some(
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()?
            .processIdentifier(),
    )
}

/// Brings the application owning `pid` frontmost, without moving the mouse
/// or touching Spaces. Combined with [`Window::raise`], this activates the
/// target window's application after a focus or stack operation.
pub fn activate_app(pid: pid_t) {
    // `ActivateIgnoringOtherApps` is a no-op from macOS 14 on, but it makes
    // activation stick on 12/13 when another app holds activation.
    #[allow(deprecated)]
    let options = NSApplicationActivationOptions::ActivateIgnoringOtherApps;
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(options);
    }
}
