mod accessibility;
mod cli;
mod display;
mod layout;
mod tile;
mod window;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};
use layout::{
    Rect, SUPPORTED_PERCENTS, center_rect, directional_rect, full_rect, is_supported_percent,
    sized_rect,
};

const EXIT_SUCCESS: u8 = 0;
const EXIT_RUNTIME_FAILURE: u8 = 1;
const EXIT_INVALID_ARGS: u8 = 2;
const EXIT_ACCESSIBILITY_UNAVAILABLE: u8 = 3;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::from(EXIT_SUCCESS),
        Err(err) => {
            if let Some(exit_err) = err.downcast_ref::<ExitError>() {
                if !exit_err.0.to_string().is_empty() {
                    eprintln!("{}", exit_err.0);
                }
                return ExitCode::from(exit_err.1);
            }
            eprintln!("error: {err}");
            ExitCode::from(EXIT_RUNTIME_FAILURE)
        }
    }
}

/// Carries an already-formatted message plus the exit code it should map to,
/// so `main` doesn't need to pattern-match error strings.
struct ExitError(String, u8);

impl std::fmt::Debug for ExitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::fmt::Display for ExitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ExitError {}

fn invalid_args(msg: impl Into<String>) -> anyhow::Error {
    ExitError(msg.into(), EXIT_INVALID_ARGS).into()
}

fn accessibility_unavailable() -> anyhow::Error {
    ExitError(
        accessibility::PERMISSION_MESSAGE.to_string(),
        EXIT_ACCESSIBILITY_UNAVAILABLE,
    )
    .into()
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let action = resolve_action(&cli)?;

    if !accessibility::is_trusted() {
        accessibility::prompt_for_trust();
        return Err(accessibility_unavailable());
    }

    match action {
        Action::Tile { gap } => run_tile(gap),
        Action::Reposition(compute) => run_reposition(compute),
    }
}

/// `compute(usable, current_window_rect) -> new_rect`. Most operations only
/// need `usable`; `center` also needs the window's current size.
type ComputeRect = Box<dyn Fn(Rect, Rect) -> Rect>;

enum Action {
    Reposition(ComputeRect),
    Tile { gap: f64 },
}

fn resolve_action(cli: &Cli) -> anyhow::Result<Action> {
    if let Some(command) = &cli.command {
        if let Some((position, size)) = command.as_position_and_size() {
            validate_size(size)?;
            return Ok(Action::Reposition(Box::new(move |usable, _window| {
                directional_rect(usable, position, size)
            })));
        }
        return match command {
            Command::Full => Ok(Action::Reposition(Box::new(|usable, _window| {
                full_rect(usable)
            }))),
            Command::Center => Ok(Action::Reposition(Box::new(|usable, window| {
                center_rect(usable, window.width, window.height)
            }))),
            Command::Tile { gap } => Ok(Action::Tile { gap: *gap }),
            _ => unreachable!("directional commands handled above"),
        };
    }

    if let Some(size) = cli.size {
        validate_size(size)?;
        return Ok(Action::Reposition(Box::new(move |usable, _window| {
            sized_rect(usable, size)
        })));
    }

    Err(invalid_args(
        "error: no command given\n\nrun `snap --help` for usage",
    ))
}

fn validate_size(size: u32) -> anyhow::Result<()> {
    if is_supported_percent(size) {
        Ok(())
    } else {
        let supported = SUPPORTED_PERCENTS.map(|p| p.to_string()).join(", ");
        Err(invalid_args(format!(
            "error: unsupported size '{size}'\n\nsupported sizes: {supported}"
        )))
    }
}

fn run_reposition(compute: ComputeRect) -> anyhow::Result<()> {
    let target = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let window_rect = target.rect().map_err(runtime_failure)?;
    let target_display = display::target_display_for(window_rect).map_err(runtime_failure)?;

    let rect = compute(target_display.usable, window_rect);
    target.set_rect(rect).map_err(runtime_failure)
}

fn run_tile(gap: f64) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let focused_rect = focused.rect().map_err(runtime_failure)?;
    let target_display = display::target_display_for(focused_rect).map_err(runtime_failure)?;

    let mut candidates =
        window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    candidates.retain(|c| !rects_roughly_equal(c.rect, focused_rect));

    let mut ordered = vec![window::TileCandidate {
        window: focused,
        rect: focused_rect,
    }];
    ordered.extend(candidates);

    let rects = tile::tile_rects(target_display.usable, ordered.len(), gap);
    for (candidate, rect) in ordered.into_iter().zip(rects) {
        // An individual unmanageable window is skipped, not fatal (PRD §23).
        let _ = candidate.window.set_rect(rect);
    }
    Ok(())
}

fn rects_roughly_equal(a: Rect, b: Rect) -> bool {
    const EPS: f64 = 1.0;
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.width - b.width).abs() < EPS
        && (a.height - b.height).abs() < EPS
}

fn runtime_failure(err: anyhow::Error) -> anyhow::Error {
    ExitError(format!("error: {err}"), EXIT_RUNTIME_FAILURE).into()
}
