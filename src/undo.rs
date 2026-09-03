//! On-disk last-rect cache for `snap undo` (PRD issue #9) — deliberately
//! the only persistent layout state snap keeps. A flat map of
//! `window_number -> {previous rect, timestamp}`, no daemon, no layout
//! tree. Hand-rolled, line-oriented (de)serialization rather than pulling
//! in `serde`/`serde_json`, matching this repo's `config.rs` precedent of
//! not adding a parsing dependency for one small file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    rect: Rect,
    recorded_at: u64,
}

/// Entries older than this are treated as stale and ignored/pruned, per the
/// issue's "prune entries older than ~24h" guidance.
const MAX_AGE_SECS: u64 = 24 * 60 * 60;

pub fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("snap")
            .join("last-frames.json"),
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Records `rect` as the pre-mutation frame for `window_number`. Called
/// after a successful mutation only (never for failed commands, `list`, or
/// `doctor`). Best-effort: an unwritable cache never fails the mutation
/// that triggered it.
pub fn record(window_number: i64, rect: Rect) {
    let Some(path) = cache_path() else { return };
    record_at(&path, window_number, rect);
}

fn record_at(path: &Path, window_number: i64, rect: Rect) {
    let mut entries = load(path);
    prune(&mut entries);
    entries.insert(
        window_number,
        Entry {
            rect,
            recorded_at: now(),
        },
    );
    save(path, &entries);
}

/// Reads the frame that undo would restore without changing the cache.
/// Call [`record`] with the current frame only after the move completes so
/// a cancelled animation does not claim that the toggle was applied.
pub fn previous(window_number: i64) -> Option<Rect> {
    let path = cache_path()?;
    previous_at(&path, window_number)
}

fn previous_at(path: &Path, window_number: i64) -> Option<Rect> {
    let mut entries = load(path);
    prune(&mut entries);
    entries.get(&window_number).map(|entry| entry.rect)
}

fn prune(entries: &mut HashMap<i64, Entry>) {
    let cutoff = now().saturating_sub(MAX_AGE_SECS);
    entries.retain(|_, e| e.recorded_at >= cutoff);
}

fn load(path: &Path) -> HashMap<i64, Entry> {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse(&contents),
        Err(_) => HashMap::new(),
    }
}

fn save(path: &Path, entries: &HashMap<i64, Entry>) {
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let _ = std::fs::write(path, serialize(entries));
}

fn serialize(entries: &HashMap<i64, Entry>) -> String {
    let mut keys: Vec<_> = entries.keys().copied().collect();
    keys.sort_unstable();

    let mut out = String::from("{\n");
    for (i, id) in keys.iter().enumerate() {
        let e = &entries[id];
        out.push_str(&format!(
            "  \"{id}\": {{\"x\":{},\"y\":{},\"width\":{},\"height\":{},\"t\":{}}}",
            e.rect.x, e.rect.y, e.rect.width, e.rect.height, e.recorded_at
        ));
        if i + 1 < keys.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

/// Deliberately not a general JSON parser — a tolerant, line-oriented scan
/// tailored to exactly what [`serialize`] writes. Any malformed content
/// (corruption, a manual edit, a future format change) yields an empty map
/// rather than an error, per "undo may fail, a mutation never does."
fn parse(contents: &str) -> HashMap<i64, Entry> {
    let mut entries = HashMap::new();
    for line in contents.lines() {
        let line = line.trim().trim_end_matches(',');
        let Some((key_part, rest)) = line.split_once(':') else {
            continue;
        };
        let Ok(window_number) = key_part.trim().trim_matches('"').parse::<i64>() else {
            continue;
        };
        let rest = rest.trim().trim_start_matches('{').trim_end_matches('}');

        let (mut x, mut y, mut width, mut height, mut t) = (None, None, None, None, None);
        for field in rest.split(',') {
            let Some((k, v)) = field.split_once(':') else {
                continue;
            };
            match k.trim().trim_matches('"') {
                "x" => x = v.trim().parse::<f64>().ok(),
                "y" => y = v.trim().parse::<f64>().ok(),
                "width" => width = v.trim().parse::<f64>().ok(),
                "height" => height = v.trim().parse::<f64>().ok(),
                "t" => t = v.trim().parse::<u64>().ok(),
                _ => {}
            }
        }
        if let (Some(x), Some(y), Some(width), Some(height), Some(t)) = (x, y, width, height, t) {
            entries.insert(
                window_number,
                Entry {
                    rect: Rect::new(x, y, width, height),
                    recorded_at: t,
                },
            );
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("snap-undo-test-{}-{n}.json", std::process::id()))
    }

    #[test]
    fn round_trips_through_serialize_and_parse() {
        let mut entries = HashMap::new();
        entries.insert(
            42,
            Entry {
                rect: Rect::new(1.0, 2.0, 3.0, 4.0),
                recorded_at: 1000,
            },
        );
        let text = serialize(&entries);
        let parsed = parse(&text);
        assert_eq!(parsed.get(&42), entries.get(&42));
    }

    #[test]
    fn parse_ignores_malformed_content() {
        assert_eq!(parse("not json at all").len(), 0);
        assert_eq!(parse("").len(), 0);
    }

    #[test]
    fn record_then_read_restores_previous_rect() {
        let path = temp_path();
        let original = Rect::new(0.0, 0.0, 800.0, 600.0);
        record_at(&path, 7, original);

        let restored = previous_at(&path, 7);
        assert_eq!(restored, Some(original));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn second_undo_toggles_back() {
        let path = temp_path();
        let original = Rect::new(10.0, 10.0, 500.0, 500.0);
        let snapped = Rect::new(0.0, 0.0, 300.0, 300.0);
        record_at(&path, 9, original);

        let first_undo = previous_at(&path, 9).unwrap();
        assert_eq!(first_undo, original);
        record_at(&path, 9, snapped);

        // Applying `first_undo` and undoing again returns to `snapped`.
        let second_undo = previous_at(&path, 9).unwrap();
        assert_eq!(second_undo, snapped);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reading_previous_frame_does_not_commit_the_toggle() {
        let path = temp_path();
        let previous = Rect::new(1.0, 2.0, 300.0, 400.0);
        record_at(&path, 42, previous);

        assert_eq!(previous_at(&path, 42), Some(previous));
        assert_eq!(previous_at(&path, 42), Some(previous));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_window_number_returns_none() {
        let path = temp_path();
        assert_eq!(previous_at(&path, 999), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stale_entries_are_pruned_and_treated_as_nothing_to_undo() {
        let path = temp_path();
        let mut entries = HashMap::new();
        entries.insert(
            5,
            Entry {
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                recorded_at: 0, // far in the past
            },
        );
        save(&path, &entries);

        assert_eq!(previous_at(&path, 5), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_treated_as_empty_not_an_error() {
        let path = temp_path();
        assert_eq!(load(&path).len(), 0);
    }
}
