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
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidActivateApplicationNotification};
use objc2_foundation::NSNotification;

use crate::ax::{AXObserver, AXUIElement};
use crate::history;
use crate::window;

/// Runs forever. Called only from `snap daemon run`, which launchd invokes.
pub fn run() -> Result<()> {
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

    CFRunLoop::run_current();
    unreachable!("CFRunLoop::run_current() blocks forever")
}

/// Drops the previous app's `AXObserver` (releasing it), records whichever
/// window is frontmost right now, then attaches a fresh `AXObserver` to the
/// new frontmost app so in-app window switches keep getting recorded until
/// the next app switch.
fn attach_and_record(observer_slot: &Rc<RefCell<Option<AXObserver>>>) {
    *observer_slot.borrow_mut() = None;
    record_current_focus();

    let Some(pid) = window::frontmost_app_pid() else {
        return;
    };
    if let Ok(observer) = attach_focused_window_observer(pid) {
        *observer_slot.borrow_mut() = Some(observer);
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
    let Some(pid) = window::frontmost_app_pid() else {
        return;
    };
    let Ok(focused) = window::Window::focused() else {
        return;
    };
    let Ok(rect) = focused.rect() else {
        return;
    };
    let Some(window_number) = window::window_number_for(pid, rect) else {
        return;
    };
    history::record(pid, window_number);
}
