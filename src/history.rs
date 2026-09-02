//! On-disk focus-history cache for `snap last` (design doc
//! docs/superpowers/specs/2026-09-02-snap-last-design.md). Same cache
//! directory and hand-rolled-JSON style as `undo.rs`'s cache — a flat
//! two-slot record (`current`, `previous`) rather than a general history
//! stack, since `snap last` only ever toggles between the two most recent
//! windows.
//!
//! The focus-watch daemon (`focus_watch.rs`) is the only writer, on every
//! real focus change; `snap last` is the only reader, and it writes back
//! the swapped result so a second `snap last` toggles back — exactly like
//! `undo.rs`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowRef {
    pub pid: i32,
    pub window_number: i64,
    pub recorded_at: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct History {
    current: Option<WindowRef>,
    previous: Option<WindowRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastError {
    /// No `previous` entry recorded yet (daemon never installed/run, or
    /// has only observed one focus change so far).
    NoHistory,
    /// A `previous` entry exists, but `resolve` couldn't turn it into a
    /// live window (app quit, window closed).
    Unavailable,
}

pub fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("snap")
            .join("focus-history.json"),
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Records `(pid, window_number)` as the newly focused window. Called by
/// the daemon on every detected focus change. No-ops if it's already the
/// recorded `current` window — de-dupes repeat notifications, and makes a
/// `snap last`-triggered activation self-consistent rather than corrupting
/// the two-slot history (see `toggle`).
pub fn record(pid: i32, window_number: i64) {
    let Some(path) = history_path() else { return };
    record_at(&path, pid, window_number);
}

fn record_at(path: &Path, pid: i32, window_number: i64) {
    let mut history = load(path);
    let already_current =
        matches!(history.current, Some(c) if c.pid == pid && c.window_number == window_number);
    if already_current {
        return;
    }
    history.previous = history.current;
    history.current = Some(WindowRef {
        pid,
        window_number,
        recorded_at: now(),
    });
    save(path, &history);
}

/// Toggles `current`/`previous` and, if a `previous` entry exists, calls
/// `resolve` with it. Commits the swap only if `resolve` succeeds — an
/// unresolvable `previous` (dead window) leaves the on-disk history
/// untouched rather than burning the only other slot on a dead reference.
pub fn toggle<T>(resolve: impl FnOnce(WindowRef) -> Option<T>) -> Result<T, LastError> {
    let path = history_path().ok_or(LastError::NoHistory)?;
    toggle_at(&path, resolve)
}

fn toggle_at<T>(path: &Path, resolve: impl FnOnce(WindowRef) -> Option<T>) -> Result<T, LastError> {
    let history = load(path);
    let previous = history.previous.ok_or(LastError::NoHistory)?;
    let current = history.current.ok_or(LastError::NoHistory)?;
    let resolved = resolve(previous).ok_or(LastError::Unavailable)?;

    let mut history = history;
    history.previous = Some(current);
    history.current = Some(previous);
    save(path, &history);
    Ok(resolved)
}

fn load(path: &Path) -> History {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse(&contents),
        Err(_) => History::default(),
    }
}

fn save(path: &Path, history: &History) {
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let _ = std::fs::write(path, serialize(history));
}

fn serialize(history: &History) -> String {
    format!(
        "{{\n  \"current\": {},\n  \"previous\": {}\n}}\n",
        serialize_slot(history.current),
        serialize_slot(history.previous),
    )
}

fn serialize_slot(slot: Option<WindowRef>) -> String {
    match slot {
        None => "null".to_string(),
        Some(w) => format!(
            "{{\"pid\":{},\"window\":{},\"t\":{}}}",
            w.pid, w.window_number, w.recorded_at
        ),
    }
}

/// Deliberately not a general JSON parser — a tolerant, line-oriented scan
/// tailored to exactly what [`serialize`] writes, matching `undo.rs`'s
/// `parse`. Any malformed content yields empty history rather than an
/// error.
fn parse(contents: &str) -> History {
    let mut history = History::default();
    for line in contents.lines() {
        let line = line.trim().trim_end_matches(',');
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        let slot = parse_slot(value.trim());
        match key {
            "current" => history.current = slot,
            "previous" => history.previous = slot,
            _ => {}
        }
    }
    history
}

fn parse_slot(value: &str) -> Option<WindowRef> {
    if value.starts_with("null") {
        return None;
    }
    let inner = value.trim_start_matches('{').trim_end_matches('}');
    let (mut pid, mut window, mut t) = (None, None, None);
    for field in inner.split(',') {
        let Some((k, v)) = field.split_once(':') else {
            continue;
        };
        match k.trim().trim_matches('"') {
            "pid" => pid = v.trim().parse::<i32>().ok(),
            "window" => window = v.trim().parse::<i64>().ok(),
            "t" => t = v.trim().parse::<u64>().ok(),
            _ => {}
        }
    }
    match (pid, window, t) {
        (Some(pid), Some(window_number), Some(recorded_at)) => Some(WindowRef {
            pid,
            window_number,
            recorded_at,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("snap-history-test-{}-{n}.json", std::process::id()))
    }

    #[test]
    fn round_trips_through_serialize_and_parse() {
        let history = History {
            current: Some(WindowRef {
                pid: 1,
                window_number: 2,
                recorded_at: 100,
            }),
            previous: Some(WindowRef {
                pid: 3,
                window_number: 4,
                recorded_at: 90,
            }),
        };
        let text = serialize(&history);
        let parsed = parse(&text);
        assert_eq!(parsed.current, history.current);
        assert_eq!(parsed.previous, history.previous);
    }

    #[test]
    fn missing_file_is_treated_as_empty_history() {
        let path = temp_path();
        let history = load(&path);
        assert_eq!(history.current, None);
        assert_eq!(history.previous, None);
    }

    #[test]
    fn parse_ignores_malformed_content() {
        let history = parse("not json at all");
        assert_eq!(history.current, None);
        assert_eq!(history.previous, None);
    }

    #[test]
    fn record_shifts_current_into_previous_on_new_window() {
        let path = temp_path();
        record_at(&path, 1, 100);
        record_at(&path, 2, 200);

        let history = load(&path);
        assert_eq!(
            history.current.map(|w| (w.pid, w.window_number)),
            Some((2, 200))
        );
        assert_eq!(
            history.previous.map(|w| (w.pid, w.window_number)),
            Some((1, 100))
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn record_is_a_no_op_when_window_is_already_current() {
        let path = temp_path();
        record_at(&path, 1, 100);
        record_at(&path, 2, 200);
        record_at(&path, 2, 200); // repeat notification for the same window

        let history = load(&path);
        assert_eq!(
            history.previous.map(|w| (w.pid, w.window_number)),
            Some((1, 100))
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn toggle_swaps_current_and_previous_and_resolves() {
        let path = temp_path();
        record_at(&path, 1, 100);
        record_at(&path, 2, 200);

        let resolved = toggle_at(&path, |w| Some((w.pid, w.window_number))).unwrap();
        assert_eq!(resolved, (1, 100));

        let history = load(&path);
        assert_eq!(
            history.current.map(|w| (w.pid, w.window_number)),
            Some((1, 100))
        );
        assert_eq!(
            history.previous.map(|w| (w.pid, w.window_number)),
            Some((2, 200))
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn toggle_twice_returns_to_the_original_current() {
        let path = temp_path();
        record_at(&path, 1, 100);
        record_at(&path, 2, 200);

        toggle_at(&path, |w| Some((w.pid, w.window_number))).unwrap();
        let second = toggle_at(&path, |w| Some((w.pid, w.window_number))).unwrap();
        assert_eq!(second, (2, 200));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn toggle_with_no_previous_entry_returns_no_history() {
        let path = temp_path();
        record_at(&path, 1, 100); // only one recorded window ever

        let result = toggle_at(&path, |w| Some((w.pid, w.window_number)));
        assert_eq!(result, Err(LastError::NoHistory));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn toggle_leaves_history_untouched_when_resolve_fails() {
        let path = temp_path();
        record_at(&path, 1, 100);
        record_at(&path, 2, 200);

        let result = toggle_at(&path, |_| None::<()>);
        assert_eq!(result, Err(LastError::Unavailable));

        let history = load(&path);
        assert_eq!(
            history.current.map(|w| (w.pid, w.window_number)),
            Some((2, 200))
        );
        assert_eq!(
            history.previous.map(|w| (w.pid, w.window_number)),
            Some((1, 100))
        );

        let _ = std::fs::remove_file(&path);
    }
}
