//! Long-lived focus-tracking daemon for `snap last` (design doc
//! docs/superpowers/specs/2026-09-02-snap-last-design.md). Only runs when
//! explicitly installed via `snap daemon install` — every other snap
//! command remains a one-shot process with no background component.
//!
//! Detection is event-driven: an `NSWorkspace` notification for app
//! switches, plus a single `AXObserver` attached to whichever app is
//! currently frontmost (swapped on every app switch) for in-app window
//! switches. Both paths funnel into `record_current_focus`, which reads
//! whatever's frontmost right now and writes it to `history.rs`'s on-disk
//! state, rather than trying to interpret each notification's payload —
//! so the two paths can never disagree about "what's focused now."

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;

use accessibility_sys::{
    AXObserverRef, AXUIElementRef, kAXFocusedWindowChangedNotification, pid_t,
};
use anyhow::{Result, anyhow};
use block2::RcBlock;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
use core_foundation::string::{CFString, CFStringRef};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSWorkspace, NSWorkspaceDidActivateApplicationNotification};
use objc2_foundation::NSNotification;

use crate::ax::{AXObserver, AXUIElement};
use crate::error_log;
use crate::history;
use crate::window;

/// Runs forever. Called only from `snap daemon run`, which launchd invokes.
pub fn run() -> Result<()> {
    // `NSWorkspace` notifications (app activation) are only delivered to
    // processes connected to the window server as a proper application —
    // a bare CFRunLoop with no `NSApplication` never receives them. Without
    // this, the AXObserver path (attached below, and re-attached on every
    // notification) silently never gets re-targeted to a newly frontmost
    // app: it stays attached to whatever app was frontmost at daemon
    // startup, so only in-app window switches within that one app keep
    // getting recorded.
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("focus_watch::run must be called from the main thread"))?;
    NSApplication::sharedApplication(mtm);

    let observer_slot: Rc<RefCell<Option<AXObserver>>> = Rc::new(RefCell::new(None));

    attach_and_record(&observer_slot);

    let slot_for_block = Rc::clone(&observer_slot);
    let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
        attach_and_record(&slot_for_block);
    });

    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let name = unsafe { NSWorkspaceDidActivateApplicationNotification };
    // SAFETY: `name` is a valid NSNotificationName; `block` outlives every
    // future notification because `run` never returns while the daemon is
    // alive (`CFRunLoop::run_current` blocks forever below).
    let _token = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
    };

    if std::env::var_os("SNAP_DEBUG").is_some() {
        eprintln!("[snap debug] daemon: entering CFRunLoop::run_current()");
    }
    CFRunLoop::run_current();
    unreachable!("CFRunLoop::run_current() blocks forever")
}

/// Drops the previous app's `AXObserver` (releasing it), records whichever
/// window is frontmost right now, then attaches a fresh `AXObserver` to the
/// new frontmost app so in-app window switches keep getting recorded until
/// the next app switch.
fn attach_and_record(observer_slot: &Rc<RefCell<Option<AXObserver>>>) {
    let debug = std::env::var_os("SNAP_DEBUG").is_some();
    if debug {
        eprintln!("[snap debug] daemon: attach_and_record fired");
    }
    *observer_slot.borrow_mut() = None;
    record_current_focus();

    let Some(pid) = window::frontmost_app_pid() else {
        error_log::record("daemon", "attach_and_record: no frontmost app pid");
        if debug {
            eprintln!("[snap debug] daemon: no frontmost app pid");
        }
        return;
    };
    match attach_focused_window_observer(pid) {
        Ok(observer) => *observer_slot.borrow_mut() = Some(observer),
        Err(e) => {
            error_log::record(
                "daemon",
                &format!("attach_focused_window_observer failed for pid {pid}: {e}"),
            );
            if debug {
                eprintln!("[snap debug] daemon: attach_focused_window_observer failed: {e}");
            }
        }
    }
}

fn attach_focused_window_observer(pid: pid_t) -> Result<AXObserver> {
    let app = AXUIElement::application(pid);
    let observer = AXObserver::new(pid, on_focused_window_changed)
        .map_err(|_| anyhow!("failed to create AXObserver for pid {pid}"))?;
    observer
        .add_notification(
            &app,
            &CFString::from_static_string(kAXFocusedWindowChangedNotification),
            std::ptr::null_mut(),
        )
        .map_err(|_| anyhow!("failed to watch focused-window changes for pid {pid}"))?;
    CFRunLoop::get_current().add_source(&observer.run_loop_source(), unsafe {
        kCFRunLoopDefaultMode
    });
    Ok(observer)
}

unsafe extern "C" fn on_focused_window_changed(
    _observer: AXObserverRef,
    _element: AXUIElementRef,
    _notification: CFStringRef,
    _refcon: *mut c_void,
) {
    record_current_focus();
}

/// Reads whichever window is frontmost right now and records it. Shared by
/// both notification paths (app switch and in-app window switch).
fn record_current_focus() {
    let debug = std::env::var_os("SNAP_DEBUG").is_some();
    let Some(pid) = window::frontmost_app_pid() else {
        error_log::record("daemon", "record_current_focus: no frontmost app pid");
        if debug {
            eprintln!("[snap debug] daemon: record_current_focus: no frontmost app pid");
        }
        return;
    };
    let focused = match window::Window::focused() {
        Ok(focused) => focused,
        Err(error) => {
            error_log::record(
                "daemon",
                &format!("record_current_focus: pid {pid}: no focused window: {error}"),
            );
            if debug {
                eprintln!(
                    "[snap debug] daemon: record_current_focus: pid {pid}: no focused window"
                );
            }
            return;
        }
    };
    let rect = match focused.rect() {
        Ok(rect) => rect,
        Err(error) => {
            error_log::record(
                "daemon",
                &format!("record_current_focus: pid {pid}: focused rect failed: {error}"),
            );
            if debug {
                eprintln!(
                    "[snap debug] daemon: record_current_focus: pid {pid}: focused.rect() failed"
                );
            }
            return;
        }
    };
    let Some(window_number) = window_number_for_with_retry(pid, rect) else {
        error_log::record(
            "daemon",
            &format!("record_current_focus: pid {pid}: no CGWindowList match for rect {rect:?}"),
        );
        if debug {
            eprintln!(
                "[snap debug] daemon: record_current_focus: pid {pid}: no CGWindowList match for rect {rect:?}"
            );
        }
        return;
    };
    if debug {
        eprintln!(
            "[snap debug] daemon: record_current_focus: recording pid {pid} window {window_number}"
        );
    }
    history::record(pid, window_number);
}

/// `CGWindowList`'s on-screen snapshot can briefly lag behind AX's
/// already-updated focused-window rect right at the moment an app becomes
/// frontmost — the very first lookup right after a notification can miss
/// even though the window is genuinely on-screen, with a lookup moments
/// later succeeding. Bounded to ~600ms: long enough to absorb that lag for
/// most apps without meaningfully delaying a background recording nothing
/// else is waiting on, but short enough not to badly stall the daemon's
/// single run loop thread if the user switches again immediately — a
/// lookup that's still failing when the budget runs out is usually
/// corrected moments later anyway by the newly-attached AXObserver's own
/// notification for whatever's now actually focused.
const WINDOW_LOOKUP_RETRIES: u32 = 10;
const WINDOW_LOOKUP_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(60);

fn window_number_for_with_retry(pid: pid_t, rect: crate::layout::Rect) -> Option<i64> {
    for attempt in 0..WINDOW_LOOKUP_RETRIES {
        if let Some(number) = window::window_number_for(pid, rect) {
            return Some(number);
        }
        if attempt + 1 < WINDOW_LOOKUP_RETRIES {
            std::thread::sleep(WINDOW_LOOKUP_RETRY_DELAY);
        }
    }
    None
}
