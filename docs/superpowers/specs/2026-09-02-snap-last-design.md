# `snap last` — focus-history daemon design

## Problem

`snap` is a one-shot CLI: each invocation runs, manipulates windows, exits.
It has no persistent process and no visibility into focus changes that
happen between invocations. The request is for `snap last` to jump back to
"the window I was on before the current one" — true OS-wide focus history,
matching Cmd+Tab's "previous" semantics, not just a history of snap's own
actions.

That requires continuously observing focus changes, which requires a
long-lived background process. This is a deliberate exception to snap's
"no daemon" design principle (called out in `undo.rs`'s doc comment and in
the README's tagline) — opt-in, not on by default.

## Architecture

Three new pieces. Every existing command is unaffected; the daemon is only
involved if the user explicitly installs it and only `snap last` reads its
output.

1. **`focus_watch` module** — the daemon's logic.
   - Subscribes to `NSWorkspace`'s app-activation notification
     (`objc2-app-kit`/`objc2-foundation` notification center) to detect app
     switches.
   - On every app switch, tears down the previous `AXObserver` (if any) and
     attaches a fresh one to the new frontmost app's `AXUIElement`, watching
     `kAXFocusedWindowChangedNotification`, to catch in-app window switches
     (e.g. Cmd+`).
   - Both notification paths feed one handler: "focus changed to window W of
     app P," which updates the on-disk history file (see below).
   - Runs the process's `CFRunLoop` indefinitely — this is the long-lived
     binary launchd keeps alive.

2. **`ax.rs` extension** — a small `AXObserver` wrapper (create,
   add-notification, attach-to-runloop, invalidate), in the same style as
   the existing `AXUIElement` wrapper: a thin safe layer over the
   `accessibility-sys` C API. The observer callback is an `extern "C"`
   function that receives a raw-pointer refcon and forwards into the
   `focus_watch` handler.

3. **CLI additions**:
   - `snap daemon install` — writes a launchd plist to
     `~/Library/LaunchAgents/com.cesarferreira.snap.focuswatch.plist`
     (`RunAtLoad`, `KeepAlive`, invoking `snap daemon run`), then loads it.
     Idempotent: running it again reloads rather than erroring.
   - `snap daemon uninstall` — unloads and removes the plist.
   - `snap daemon run` — hidden from `--help`; the actual entry point
     launchd invokes. Not intended for manual use but not blocked either.
   - `snap last` — reads the history file, swaps `current`/`previous`,
     resolves the new `current` to a live window, activates and raises it.
   - `snap doctor` gains a line reporting whether the launch agent is
     installed and loaded (diagnosable state, same spirit as its existing
     Accessibility/config/display checks).

## State: on-disk focus history

New file, same cache directory `undo.rs` already uses:
`~/Library/Caches/snap/focus-history.json`. Hand-rolled line-oriented
(de)serialization, matching `undo.rs`'s existing precedent of not pulling in
`serde` for one small file. Holds exactly two slots:

```json
{"current": {"pid":123,"window":456,"t":1234567890}, "previous": {"pid":789,"window":12,"t":1234567800}}
```

Each slot is `{pid, window_number, recorded_at}` — enough identity to later
resolve back to a live `AXUIElement` (see Resolution below). Missing/corrupt
file is treated as empty history, never an error (same tolerance as
`undo.rs`'s `parse`).

**Daemon write path** (on every detected focus change to window W):
- If `W == current`, no-op. This de-dupes repeated notifications for the
  same window (app activation can fire more than once) and — critically —
  makes the daemon's observation of a `snap last`-triggered activation
  consistent with what `snap last` already wrote (see below), so no
  special-casing is needed between "user-caused" and "snap-caused" switches.
- Otherwise: shift `current` → `previous`, set `current = W`.

**`snap last` read path**:
1. Swap `current` and `previous` in the file.
2. Resolve the new `current` (was `previous`) to a live window.
3. `activate_app(pid)` to bring the owning app forward, then raise the
   specific window (not just "some window of that app").

This gives toggle semantics identical to `undo`: running `snap last` twice
bounces back to where you started. Because the daemon observes every focus
change — including ones caused by `snap` itself (`focus`, `swap`, `stack
next`, `display`) — `snap last`'s own activation is naturally absorbed by
the write path's no-op rule instead of corrupting the two-slot history.

**Resolution (window_number → AXUIElement)**: generalizes the rect-matching
technique `window.rs::visible_windows_on` already uses for on-screen
windows. Query `CGWindowListCopyWindowInfo` for all windows owned by `pid`,
find the entry matching `window_number`, read its bounds, then find the
`AXUIElement` in the app's `kAXWindowsAttribute` list whose rect matches
(`rects_roughly_equal`, already defined in `window.rs`). Unlike
`visible_windows_on`, this isn't scoped to a single display or to
on-screen-only windows — it needs to find the window regardless of which
display/Space it's currently on.

## Error handling

- `snap last` with no history file (daemon never installed/run, or hasn't
  observed a second focus change yet) →
  `error: no focus history yet — run 'snap daemon install'`.
- `previous` window/app no longer exists (closed, quit) →
  `error: previous window is no longer available`. History is left
  untouched in this case — don't burn the only other slot on a dead
  reference by swapping anyway.
- `snap daemon install` when already installed → reloads instead of
  erroring.
- Daemon-internal errors (an `AXObserver` failing to attach to an
  uncooperative/dying app, a transient AX permission hiccup) are logged
  under `SNAP_DEBUG` and skipped — the long-lived process must never crash
  over one misbehaving app.

## Testing

- Pure-logic unit tests for the history file — load/save/round-trip,
  corrupt-file tolerance, the shift-on-change/no-op-on-same-window rule —
  mirroring `undo.rs`'s existing test style. No AX/NSWorkspace involved, so
  these run headlessly like today's tests.
- The `AXObserver` wrapper and the daemon's notification handlers are
  integration-only; no unit tests, matching how `window.rs`'s AX calls are
  already untested and rely on manual verification (can't simulate real
  focus changes in a headless test run).
- Manual verification plan: install the daemon; switch between apps and
  between windows of the same app (Cmd+Tab, clicks, Cmd+`); run `snap last`
  and confirm it returns focus correctly; run it twice to confirm toggle;
  quit an app that's in history and confirm the "no longer available"
  error; confirm `snap doctor` reports daemon install/load state accurately.

## Out of scope

- History deeper than two slots (a real back/forward stack). Toggle-only,
  matching `undo`'s existing UX.
- Tracking focus changes made before the daemon was ever installed.
- Auto-installing the daemon on first `snap last` use — install is an
  explicit, separate step (`snap daemon install`).
