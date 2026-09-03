use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use objc2_app_kit::NSWorkspace;

use crate::layout::Rect;
use crate::window::{RectChange, Window};

const FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy)]
pub struct Generation {
    started_at: u128,
    pid: u32,
}

impl Generation {
    pub fn now() -> Self {
        Self {
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0),
            pid: std::process::id(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Settings {
    duration: Duration,
    generation: Generation,
}

pub struct Transition<'a> {
    window: &'a Window,
    from: Rect,
    to: Rect,
    change: RectChange,
}

impl<'a> Transition<'a> {
    pub fn new(window: &'a Window, from: Rect, to: Rect) -> Result<Self> {
        let change = window.prepare_rect(to)?;
        Ok(Self {
            window,
            from,
            to,
            change,
        })
    }
}

pub enum Outcome {
    Applied,
    Failed(anyhow::Error),
    Cancelled,
}

struct CancellationToken {
    path: PathBuf,
    generation: String,
    lock_file: File,
}

impl CancellationToken {
    fn begin(generation: Generation) -> std::io::Result<Self> {
        let path = generation_path()?;
        let generation = format_generation(generation);
        Self::begin_at(path, &generation)
    }

    fn begin_at(path: PathBuf, generation: &str) -> std::io::Result<Self> {
        let token = Self::open_at(path, generation)?;
        token.with_lock(|| {
            let current = std::fs::read_to_string(&token.path).unwrap_or_default();
            if current.as_str() < generation {
                std::fs::write(&token.path, generation)?;
            }
            Ok(())
        })?;
        Ok(token)
    }

    fn open_at(path: PathBuf, generation: &str) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path(&path))?;
        Ok(Self {
            path,
            generation: generation.to_owned(),
            lock_file,
        })
    }

    #[cfg(test)]
    fn is_current(&self) -> bool {
        self.with_lock(|| Ok(self.is_current_unlocked()))
            .unwrap_or(false)
    }

    fn apply_if_current<T>(&self, apply: impl FnOnce() -> T) -> std::io::Result<Option<T>> {
        self.with_lock(|| Ok(self.is_current_unlocked().then(apply)))
    }

    fn is_current_unlocked(&self) -> bool {
        std::fs::read_to_string(&self.path).is_ok_and(|value| value == self.generation)
    }

    fn with_lock<T>(&self, operation: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
        if unsafe { libc::flock(self.lock_file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let result = operation();
        let unlock_result =
            if unsafe { libc::flock(self.lock_file.as_raw_fd(), libc::LOCK_UN) } != 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            };
        match (result, unlock_result) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }
}

fn format_generation(generation: Generation) -> String {
    format!("{:039}-{:010}", generation.started_at, generation.pid)
}

fn lock_path(generation_path: &Path) -> PathBuf {
    generation_path.with_extension("lock")
}

fn effective_duration_ms(configured: u64, animations: bool, reduce_motion: bool) -> u64 {
    if animations && !reduce_motion {
        configured
    } else {
        0
    }
}

pub fn configured(configured_ms: u64, animations: bool, generation: Generation) -> Settings {
    let reduce_motion = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
    Settings {
        duration: Duration::from_millis(effective_duration_ms(
            configured_ms,
            animations,
            reduce_motion,
        )),
        generation,
    }
}

/// Serializes a state read with animation frames and their completion
/// metadata, preventing callers from observing a half-published final state.
pub fn coordinate<T>(operation: impl FnOnce() -> T) -> Result<T> {
    Ok(coordinate_at(generation_path()?, operation)?)
}

/// Runs a side effect only if this invocation still owns the current
/// generation, holding the coordinator lock for the full operation.
pub fn if_current<T>(settings: Settings, operation: impl FnOnce() -> T) -> Result<Option<T>> {
    let token =
        CancellationToken::open_at(generation_path()?, &format_generation(settings.generation))?;
    Ok(token.apply_if_current(operation)?)
}

fn coordinate_at<T>(path: PathBuf, operation: impl FnOnce() -> T) -> std::io::Result<T> {
    CancellationToken::open_at(path, "")?.with_lock(|| Ok(operation()))
}

/// Advances all transitions on the same time-based easing curve. Individual
/// AX failures stop only that window; a newer Snap process cancels unfinished
/// transitions so rapidly repeated shortcuts do not fight over geometry.
/// `complete` runs under the same interprocess lock as the exact final frame,
/// allowing callers to publish matching undo metadata atomically.
pub fn run(
    transitions: &[Transition<'_>],
    settings: Settings,
    start: impl FnOnce(),
    complete: impl FnOnce(&[bool]) -> Result<()>,
) -> Result<Vec<Outcome>> {
    let duration = if transitions
        .iter()
        .all(|transition| transition.change.is_empty())
    {
        Duration::ZERO
    } else {
        settings.duration
    };
    let token = CancellationToken::begin(settings.generation)?;
    let started = Instant::now();
    let mut failures: Vec<Option<anyhow::Error>> = std::iter::repeat_with(|| None)
        .take(transitions.len())
        .collect();
    let mut start = Some(start);
    let mut complete = Some(complete);

    loop {
        let frame_started = Instant::now();
        let progress = if duration.is_zero() {
            1.0
        } else {
            (started.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0)
        };

        let applied = token.apply_if_current(|| -> Result<()> {
            if let Some(start) = start.take() {
                start();
            }
            for (index, transition) in transitions.iter().enumerate() {
                if failures[index].is_some() {
                    continue;
                }
                let rect = interpolate_rect(transition.from, transition.to, progress);
                if let Err(error) = transition.window.apply_rect(rect, transition.change) {
                    failures[index] = Some(error);
                }
            }
            if progress >= 1.0 {
                let applied: Vec<bool> = failures.iter().map(Option::is_none).collect();
                if let Some(complete) = complete.take() {
                    complete(&applied)?;
                }
            }
            Ok(())
        })?;
        match applied {
            None => {
                return Ok(failures
                    .into_iter()
                    .map(|failure| failure.map_or(Outcome::Cancelled, Outcome::Failed))
                    .collect());
            }
            Some(result) => result?,
        }

        if progress >= 1.0 {
            break;
        }

        if let Some(remaining) = FRAME_INTERVAL.checked_sub(frame_started.elapsed()) {
            std::thread::sleep(remaining);
        }
    }

    Ok(failures
        .into_iter()
        .map(|failure| failure.map_or(Outcome::Applied, Outcome::Failed))
        .collect())
}

fn generation_path() -> std::io::Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "HOME is not set; cannot coordinate window animations",
        )
    })?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Caches")
        .join("snap")
        .join("animation-generation"))
}

fn interpolate_rect(start: Rect, end: Rect, progress: f64) -> Rect {
    let progress = progress.clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - progress).powi(3);
    let interpolate = |from: f64, to: f64| from + (to - from) * eased;
    Rect::new(
        interpolate(start.x, end.x),
        interpolate(start.y, end.y),
        interpolate(start.width, end.width),
        interpolate(start.height, end.height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Rect;
    use std::path::PathBuf;

    #[test]
    fn interpolation_uses_cubic_ease_out() {
        let start = Rect::new(0.0, 0.0, 100.0, 100.0);
        let end = Rect::new(100.0, 200.0, 200.0, 300.0);

        assert_eq!(
            interpolate_rect(start, end, 0.5),
            Rect::new(87.5, 175.0, 187.5, 275.0)
        );
    }

    #[test]
    fn interpolation_clamps_progress_to_the_transition_endpoints() {
        let start = Rect::new(10.0, 20.0, 300.0, 400.0);
        let end = Rect::new(50.0, 60.0, 700.0, 800.0);

        assert_eq!(interpolate_rect(start, end, -1.0), start);
        assert_eq!(interpolate_rect(start, end, 2.0), end);
    }

    #[test]
    fn a_new_animation_cancels_the_previous_generation() {
        let directory = std::env::temp_dir().join(format!(
            "snap-animation-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let path = directory.join("generation");

        let first = CancellationToken::begin_at(PathBuf::from(&path), "first").unwrap();
        assert!(first.is_current());

        let second = CancellationToken::begin_at(PathBuf::from(&path), "second").unwrap();
        assert!(!first.is_current());
        assert!(second.is_current());

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn an_older_animation_cannot_supersede_a_newer_generation() {
        let directory =
            std::env::temp_dir().join(format!("snap-animation-order-test-{}", std::process::id()));
        let path = directory.join("generation");

        let newer = CancellationToken::begin_at(PathBuf::from(&path), "200").unwrap();
        let older = CancellationToken::begin_at(PathBuf::from(&path), "100").unwrap();

        assert!(newer.is_current());
        assert!(!older.is_current());

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn generation_check_and_frame_application_are_atomic() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, mpsc};

        let directory =
            std::env::temp_dir().join(format!("snap-animation-lock-test-{}", std::process::id()));
        let path = directory.join("generation");
        let frame = Arc::new(AtomicUsize::new(0));
        let older = CancellationToken::begin_at(path.clone(), "100").unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let old_frame = Arc::clone(&frame);

        let old_thread = std::thread::spawn(move || {
            older
                .apply_if_current(|| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    old_frame.store(1, Ordering::SeqCst);
                })
                .unwrap();
        });

        entered_rx.recv().unwrap();
        let new_frame = Arc::clone(&frame);
        let new_thread = std::thread::spawn(move || {
            let newer = CancellationToken::begin_at(path, "200").unwrap();
            newer
                .apply_if_current(|| new_frame.store(2, Ordering::SeqCst))
                .unwrap();
        });
        release_tx.send(()).unwrap();
        old_thread.join().unwrap();
        new_thread.join().unwrap();

        assert_eq!(frame.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn older_animation_cannot_commit_metadata_after_newer_animation() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let directory =
            std::env::temp_dir().join(format!("snap-animation-commit-test-{}", std::process::id()));
        let path = directory.join("generation");
        let metadata = AtomicUsize::new(0);
        let _older = CancellationToken::begin_at(path.clone(), "100").unwrap();
        let _newer = CancellationToken::begin_at(path.clone(), "200").unwrap();

        CancellationToken::open_at(path.clone(), "200")
            .unwrap()
            .apply_if_current(|| metadata.store(2, Ordering::SeqCst))
            .unwrap();
        CancellationToken::open_at(path, "100")
            .unwrap()
            .apply_if_current(|| metadata.store(1, Ordering::SeqCst))
            .unwrap();

        assert_eq!(metadata.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn final_frame_and_metadata_are_not_observable_half_committed() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, mpsc};

        let directory = std::env::temp_dir().join(format!(
            "snap-animation-final-commit-test-{}",
            std::process::id()
        ));
        let path = directory.join("generation");
        let frame = Arc::new(AtomicUsize::new(0));
        let metadata = Arc::new(AtomicUsize::new(0));
        let writer = CancellationToken::begin_at(path.clone(), "100").unwrap();
        let (frame_tx, frame_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let written_frame = Arc::clone(&frame);
        let written_metadata = Arc::clone(&metadata);

        let writer_thread = std::thread::spawn(move || {
            writer
                .apply_if_current(|| {
                    written_frame.store(1, Ordering::SeqCst);
                    frame_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    written_metadata.store(1, Ordering::SeqCst);
                })
                .unwrap();
        });

        frame_rx.recv().unwrap();
        let observed_frame = Arc::clone(&frame);
        let observed_metadata = Arc::clone(&metadata);
        let reader_thread = std::thread::spawn(move || {
            coordinate_at(path, || {
                (
                    observed_frame.load(Ordering::SeqCst),
                    observed_metadata.load(Ordering::SeqCst),
                )
            })
            .unwrap()
        });
        release_tx.send(()).unwrap();

        writer_thread.join().unwrap();
        assert_eq!(reader_thread.join().unwrap(), (1, 1));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn reduce_motion_disables_the_configured_duration() {
        assert_eq!(effective_duration_ms(180, true, false), 180);
        assert_eq!(effective_duration_ms(180, true, true), 0);
    }

    #[test]
    fn disabled_animations_use_an_instant_transition() {
        assert_eq!(effective_duration_ms(180, false, false), 0);
    }
}
