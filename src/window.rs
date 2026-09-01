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
use cocoa::base::{BOOL, YES, id};
use cocoa::foundation::NSArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use core_graphics::geometry::{CGPoint, CGSize};
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
pub fn visible_windows_on(display_frame: Rect) -> Result<Vec<TileCandidate>> {
    let mut candidates = Vec::new();

    for pid in running_regular_app_pids() {
        let app = AXUIElement::application(pid);
        let Ok(windows) = app.windows() else { continue };

        for element in windows.iter() {
            if !is_tileable(&element) {
                continue;
            }
            let window = Window {
                element: element.clone(),
            };
            let Ok(rect) = window.rect() else { continue };
            if overlap_area(rect, display_frame) <= 0.0 {
                continue;
            }
            candidates.push(TileCandidate { window, rect });
        }
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

/// NSApplicationActivationPolicyRegular apps only — excludes menu-bar-only
/// utilities, background agents, and hidden applications (PRD §13).
fn running_regular_app_pids() -> Vec<pid_t> {
    const ACTIVATION_POLICY_REGULAR: i64 = 0;

    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: id = msg_send![workspace, runningApplications];
        let count = apps.count();

        let mut pids = Vec::new();
        for i in 0..count {
            let app: id = apps.objectAtIndex(i);
            let policy: i64 = msg_send![app, activationPolicy];
            if policy != ACTIVATION_POLICY_REGULAR {
                continue;
            }
            let hidden: BOOL = msg_send![app, isHidden];
            if hidden == YES {
                continue;
            }
            let pid: pid_t = msg_send![app, processIdentifier];
            pids.push(pid);
        }
        pids
    }
}
