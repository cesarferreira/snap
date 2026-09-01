mod accessibility;
mod cli;
mod config;
mod display;
mod layout;
mod tile;
mod window;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};
use layout::{
    DisplayTarget, Rect, SUPPORTED_PERCENTS, center_rect, detect_centered_percent,
    detect_directional_percent, directional_rect, full_rect, is_supported_percent,
    map_rect_between_displays, next_cycle_percent, padded, resolve_display_index, sized_rect,
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
    let config = config::load();

    if !accessibility::is_trusted() {
        accessibility::prompt_for_trust();
        return Err(accessibility_unavailable());
    }

    match action {
        Action::Tile { gap } => run_tile(gap.unwrap_or(config.padding), config.stage_manager_width),
        Action::Reposition(compute) => {
            run_reposition(compute, config.padding, config.stage_manager_width)
        }
        Action::Display(target) => {
            run_display_move(target, config.padding, config.stage_manager_width)
        }
    }
}

/// `compute(usable, current_window_rect) -> new_rect`. Most operations only
/// need `usable`; `center` also needs the window's current size.
type ComputeRect = Box<dyn Fn(Rect, Rect) -> Rect>;

enum Action {
    Reposition(ComputeRect),
    Tile { gap: Option<f64> },
    Display(DisplayTarget),
}

fn resolve_action(cli: &Cli) -> anyhow::Result<Action> {
    if let Some(command) = &cli.command {
        if let Some((position, size)) = command.as_position_and_size() {
            return match size {
                Some(size) => {
                    validate_size(size)?;
                    Ok(Action::Reposition(Box::new(move |usable, _window| {
                        directional_rect(usable, position, size)
                    })))
                }
                // No SIZE given — cycle 25/50/75% based on the window's current geometry.
                None => Ok(Action::Reposition(Box::new(move |usable, window| {
                    let current = detect_directional_percent(usable, position, window);
                    directional_rect(usable, position, next_cycle_percent(current))
                }))),
            };
        }
        return match command {
            Command::Full => Ok(Action::Reposition(Box::new(|usable, _window| {
                full_rect(usable)
            }))),
            Command::Center => Ok(Action::Reposition(Box::new(|usable, window| {
                center_rect(usable, window.width, window.height)
            }))),
            Command::Tile { gap } => {
                if let Some(gap) = gap {
                    validate_gap(*gap)?;
                }
                Ok(Action::Tile { gap: *gap })
            }
            Command::Display { target } => Ok(Action::Display(*target)),
            _ => unreachable!("directional commands handled above"),
        };
    }

    if let Some(size) = cli.size {
        validate_size(size)?;
        return Ok(Action::Reposition(Box::new(move |usable, _window| {
            sized_rect(usable, size)
        })));
    }

    // Bare `snap` with no size — cycle 25/50/75% based on the window's current geometry.
    Ok(Action::Reposition(Box::new(|usable, window| {
        let current = detect_centered_percent(usable, window);
        sized_rect(usable, next_cycle_percent(current))
    })))
}

fn validate_gap(gap: f64) -> anyhow::Result<()> {
    if gap.is_finite() && gap >= 0.0 {
        Ok(())
    } else {
        Err(invalid_args(format!(
            "error: invalid gap '{gap}'\n\ngap must be a non-negative number"
        )))
    }
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

fn run_reposition(
    compute: ComputeRect,
    padding: f64,
    stage_manager_width: f64,
) -> anyhow::Result<()> {
    let target = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let window_rect = target.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(window_rect, stage_manager_width).map_err(runtime_failure)?;

    let usable = padded(target_display.usable, padding);
    let rect = compute(usable, window_rect);
    if std::env::var_os("SNAP_DEBUG").is_some() {
        eprintln!(
            "[snap debug] display.usable={:?} padded_usable={usable:?} requested={rect:?}",
            target_display.usable
        );
    }
    target.set_rect(rect).map_err(runtime_failure)
}

fn run_display_move(
    target: DisplayTarget,
    padding: f64,
    stage_manager_width: f64,
) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let window_rect = focused.rect().map_err(runtime_failure)?;

    let displays = display::ordered_displays(stage_manager_width).map_err(runtime_failure)?;
    if displays.len() == 1 && matches!(target, DisplayTarget::Next | DisplayTarget::Previous) {
        return Err(ExitError("error: only one display".into(), EXIT_RUNTIME_FAILURE).into());
    }

    let current_index = display::display_index_containing(&displays, window_rect);
    let dest_index =
        resolve_display_index(current_index, displays.len(), target).ok_or_else(|| {
            invalid_args(format!(
                "error: invalid display target\n\ndisplays currently attached: {}",
                displays.len()
            ))
        })?;

    let from_usable = padded(displays[current_index].usable, padding);
    let to_usable = padded(displays[dest_index].usable, padding);
    let new_rect = map_rect_between_displays(window_rect, from_usable, to_usable);
    focused.set_rect(new_rect).map_err(runtime_failure)
}

fn run_tile(gap: f64, stage_manager_width: f64) -> anyhow::Result<()> {
    let debug = std::env::var_os("SNAP_DEBUG").is_some();

    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let focused_rect = focused.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(focused_rect, stage_manager_width).map_err(runtime_failure)?;

    let mut candidates =
        window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    candidates.retain(|c| !rects_roughly_equal(c.rect, focused_rect));

    let mut ordered = vec![window::TileCandidate {
        window: focused,
        rect: focused_rect,
    }];
    ordered.extend(candidates);

    // The same padding value governs both the outer margin (window-to-screen-edge)
    // and the inter-tile gap, matching how `snap left/right/top/bottom/full/center`
    // apply it as a uniform screen-edge inset.
    let usable = padded(target_display.usable, gap);

    if debug {
        eprintln!("[snap debug] usable={usable:?} (padding={gap})");
        eprintln!("[snap debug] {} window(s) to tile", ordered.len());
    }

    let rects = tile::tile_rects(usable, ordered.len(), gap);
    for (candidate, rect) in ordered.into_iter().zip(rects) {
        // An individual unmanageable window is skipped, not fatal (PRD §23).
        let result = candidate.window.set_rect(rect);
        if debug {
            let after = candidate.window.rect();
            eprintln!("[snap debug] requested={rect:?} set_rect={result:?} actual_after={after:?}");
        }
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
