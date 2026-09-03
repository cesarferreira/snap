//! Best-effort persistent error log for CLI failures.

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("snap")
            .join("errors.log"),
    )
}

pub fn record(component: &str, message: &str) {
    let Some(path) = path() else { return };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    record_at(&path, timestamp, component, message);
}

fn record_at(path: &Path, timestamp: u64, component: &str, message: &str) {
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let message = message.lines().collect::<Vec<_>>().join(" | ");
    let _ = writeln!(file, "[{timestamp}] {component}: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "snap-error-log-test-{}-{n}.log",
            std::process::id()
        ))
    }

    #[test]
    fn record_at_appends_without_overwriting_existing_errors() {
        let path = temp_path();

        record_at(&path, 100, "window", "first failure");
        record_at(&path, 101, "cli", "second failure");

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[100] window: first failure\n[101] cli: second failure\n"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_at_keeps_multiline_errors_in_one_log_entry() {
        let path = temp_path();

        record_at(&path, 100, "cli", "first line\nsecond line");

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[100] cli: first line | second line\n"
        );
        let _ = std::fs::remove_file(path);
    }
}
