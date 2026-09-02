mod accessibility;
mod accordion;
mod ax;
mod cli;
mod config;
mod display;
mod focus_watch;
mod history;
mod launchd;
mod layout;
mod spatial;
mod tile;
mod undo;
mod window;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command, DaemonCommand, ListScope, StackAction};
use layout::{
    DisplayTarget, MAX_PERCENT, MIN_PERCENT, Rect, almost_rect, center_rect,
    detect_centered_percent, detect_directional_percent, detect_third, directional_rect, full_rect,
    grow_rect, is_supported_percent, map_rect_between_displays, next_cycle_percent, next_third,
    padded, resolve_display_index, shrink_rect, sized_rect, third_rect,
};
use spatial::{Direction, neighbor_in_direction};
use tile::TileLayout;

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
    let config = config::load();
    let action = resolve_action(&cli, &config)?;

    // `doctor` reports Accessibility status rather than requiring it —
    // unlike every other command, it must still produce output (and exit
    // 0) when snap isn't trusted yet.
    if let Action::Doctor = action {
        return run_doctor(&config, config.stage_manager_width);
    }

    // `daemon install`/`uninstall` never touch AX; `daemon run` checks
    // Accessibility itself (see `run_daemon`) since it's the one variant
    // that actually calls into AX.
    if let Action::Daemon(daemon_action) = action {
        return run_daemon(daemon_action);
    }

    let app = cli.app.as_deref();

    if !accessibility::is_trusted() {
        accessibility::prompt_for_trust();
        return Err(accessibility_unavailable());
    }

    match action {
        Action::Tile { gap, layout } => run_tile(
            gap.unwrap_or(config.padding),
            config.stage_manager_width,
            layout,
        ),
        Action::Reposition(compute) => {
            run_reposition(compute, config.padding, config.stage_manager_width, app)
        }
        Action::Display(target) => {
            run_display_move(target, config.padding, config.stage_manager_width, app)
        }
        Action::List(scope) => run_list(scope, config.stage_manager_width),
        Action::Focus(direction) => run_focus(direction, config.stage_manager_width),
        Action::Swap(direction) => run_swap(direction, config.stage_manager_width),
        Action::Stack(action) => run_stack(
            action,
            config.padding,
            config.stage_manager_width,
            config.accordion_padding,
        ),
        Action::Undo => run_undo(config.stage_manager_width),
        Action::Last => run_last(),
        Action::Daemon(_) => unreachable!("handled above before the accessibility gate"),
        Action::Doctor => unreachable!("handled above before the accessibility gate"),
    }
}

/// `compute(usable, current_window_rect) -> new_rect`. Most operations only
/// need `usable`; `center` also needs the window's current size.
type ComputeRect = Box<dyn Fn(Rect, Rect) -> Rect>;

enum Action {
    Reposition(ComputeRect),
    Tile {
        gap: Option<f64>,
        layout: TileLayout,
    },
    Display(DisplayTarget),
    List(ListScope),
    Focus(Direction),
    Swap(Direction),
    Stack(Option<StackAction>),
    Undo,
    Last,
    Daemon(DaemonCommand),
    Doctor,
}

fn resolve_action(cli: &Cli, config: &config::Config) -> anyhow::Result<Action> {
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
            Command::Grow => Ok(Action::Reposition(Box::new(|usable, window| {
                grow_rect(usable, window)
            }))),
            Command::Shrink => Ok(Action::Reposition(Box::new(|usable, window| {
                shrink_rect(usable, window)
            }))),
            Command::Almost => {
                let almost_padding = config.almost_padding;
                Ok(Action::Reposition(Box::new(move |usable, _window| {
                    almost_rect(usable, almost_padding)
                })))
            }
            Command::Tile { gap, layout } => {
                if cli.app.is_some() {
                    return Err(invalid_args(
                        "error: --app is not supported with tile\n\ntile is a display operation; use --app with size/side/corner/full/center commands instead",
                    ));
                }
                if let Some(gap) = gap {
                    validate_gap(*gap)?;
                }
                Ok(Action::Tile {
                    gap: *gap,
                    layout: layout.unwrap_or_default(),
                })
            }
            Command::Display { target } => Ok(Action::Display(*target)),
            Command::List { display } => Ok(Action::List(*display)),
            Command::Focus { direction } => Ok(Action::Focus(*direction)),
            Command::Swap { direction } => Ok(Action::Swap(*direction)),
            Command::Stack { action } => Ok(Action::Stack(*action)),
            Command::Undo => Ok(Action::Undo),
            Command::Last => Ok(Action::Last),
            Command::Daemon { action } => Ok(Action::Daemon(*action)),
            Command::Doctor => Ok(Action::Doctor),
            Command::Third { position } => match position {
                Some(third) => {
                    let third = *third;
                    Ok(Action::Reposition(Box::new(move |usable, _window| {
                        third_rect(usable, third)
                    })))
                }
                None => Ok(Action::Reposition(Box::new(|usable, window| {
                    let current = detect_third(usable, window);
                    third_rect(usable, next_third(current))
                }))),
            },
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
        Err(invalid_args(format!(
            "error: unsupported size '{size}'\n\nsize must be an integer percent from {MIN_PERCENT} to {MAX_PERCENT}"
        )))
    }
}

/// Resolves the window a mutate command should act on: the focused window
/// by default, or the window matching `--app NAME` when given.
/// Resolves the target window plus its `kCGWindowNumber`, when it can be
/// determined, for the caller to hand to [`undo::record`] after a
/// successful mutation. `None` (rather than failing the command) when the
/// window can't be matched back to a CGWindowList entry — undo just won't
/// work for that one mutation.
fn resolve_target(
    app: Option<&str>,
    stage_manager_width: f64,
) -> anyhow::Result<(window::Window, Rect, Option<i64>)> {
    match app {
        None => {
            let window = window::Window::focused()
                .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
            let rect = window.rect().map_err(runtime_failure)?;
            let window_number = display::target_display_for(rect, stage_manager_width)
                .ok()
                .and_then(|d| window::visible_windows_on(d.frame).ok())
                .and_then(|candidates| {
                    candidates
                        .iter()
                        .find(|c| rects_roughly_equal(c.rect, rect))
                        .map(|c| c.window_number)
                });
            Ok((window, rect, window_number))
        }
        Some(name) => {
            let candidate = find_app_window(name, stage_manager_width)?;
            let rect = candidate.rect;
            Ok((candidate.window, rect, Some(candidate.window_number)))
        }
    }
}

/// `--app NAME` resolution (PRD issue #4): exact, case-insensitive match on
/// `kCGWindowOwnerName` across every attached display. If the app is
/// frontmost, its currently focused window wins; otherwise its largest
/// window (ties broken by title) does. Two distinct running processes
/// sharing the same displayed app name are reported as ambiguous rather
/// than picked between arbitrarily.
fn find_app_window(name: &str, stage_manager_width: f64) -> anyhow::Result<window::TileCandidate> {
    let displays = display::ordered_displays(stage_manager_width).map_err(runtime_failure)?;
    let focused_pid = window::frontmost_app_pid();
    let focused_rect = window::Window::focused().ok().and_then(|w| w.rect().ok());

    let mut seen = std::collections::HashSet::new();
    let mut matches: Vec<window::TileCandidate> = Vec::new();
    for d in &displays {
        let candidates = window::visible_windows_on(d.frame).map_err(runtime_failure)?;
        for c in candidates {
            if c.app_name.eq_ignore_ascii_case(name) && seen.insert(c.window_number) {
                matches.push(c);
            }
        }
    }

    if matches.is_empty() {
        return Err(ExitError(
            format!("error: no window for app '{name}'"),
            EXIT_RUNTIME_FAILURE,
        )
        .into());
    }

    let distinct_pids: std::collections::HashSet<_> = matches.iter().map(|c| c.pid).collect();
    if distinct_pids.len() > 1 {
        let mut pids: Vec<_> = distinct_pids.into_iter().collect();
        pids.sort_unstable();
        let candidates_desc = pids
            .iter()
            .map(|pid| format!("  pid {pid}"))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(ExitError(
            format!(
                "error: ambiguous app name '{name}' matches multiple running processes:\n{candidates_desc}"
            ),
            EXIT_RUNTIME_FAILURE,
        )
        .into());
    }

    if Some(matches[0].pid) == focused_pid {
        if let Some(idx) =
            focused_rect.and_then(|fr| matches.iter().position(|c| rects_roughly_equal(fr, c.rect)))
        {
            return Ok(matches.swap_remove(idx));
        }
    }

    // Not frontmost (or its focused window wasn't in the candidate set):
    // largest window wins, ties broken by title for determinism.
    matches.sort_by(|a, b| {
        (b.rect.width * b.rect.height)
            .partial_cmp(&(a.rect.width * a.rect.height))
            .unwrap()
            .then(
                a.title
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.title.as_deref().unwrap_or("")),
            )
    });
    Ok(matches.remove(0))
}

fn run_reposition(
    compute: ComputeRect,
    padding: f64,
    stage_manager_width: f64,
    app: Option<&str>,
) -> anyhow::Result<()> {
    let (target, window_rect, window_number) = resolve_target(app, stage_manager_width)?;
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
    target.set_rect(rect).map_err(runtime_failure)?;
    if let Some(window_number) = window_number {
        undo::record(window_number, window_rect);
    }
    Ok(())
}

fn run_display_move(
    target: DisplayTarget,
    padding: f64,
    stage_manager_width: f64,
    app: Option<&str>,
) -> anyhow::Result<()> {
    let (focused, window_rect, window_number) = resolve_target(app, stage_manager_width)?;

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
    focused.set_rect(new_rect).map_err(runtime_failure)?;
    if let Some(window_number) = window_number {
        undo::record(window_number, window_rect);
    }
    Ok(())
}

/// `snap undo` — restores the focused window to its previously recorded
/// frame, then swaps the cache entry so a second `undo` toggles back.
/// `snap doctor` — read-only diagnostic report (PRD issue #10). Unlike
/// every other command it does not require Accessibility trust to run: it
/// reports trust status as one line among several, exiting 0 as long as it
/// could produce a report at all.
fn run_doctor(config: &config::Config, stage_manager_width: f64) -> anyhow::Result<()> {
    println!("snap {}", env!("CARGO_PKG_VERSION"));
    if let Ok(path) = std::env::current_exe() {
        println!("binary: {}", path.display());
    }
    println!();

    if accessibility::is_trusted() {
        println!("Accessibility: trusted");
    } else {
        println!("Accessibility: not trusted");
        for line in accessibility::PERMISSION_MESSAGE.lines() {
            println!("  {line}");
        }
    }
    println!();

    match config::config_path() {
        Some(path) if path.exists() => println!("Config: {}", path.display()),
        Some(path) => println!("Config: {} (not found, using defaults)", path.display()),
        None => println!("Config: $HOME not set, using defaults"),
    }
    println!("  padding = {}", config.padding);
    println!("  stage_manager_width = {}", config.stage_manager_width);
    println!("  almost_padding = {}", config.almost_padding);
    println!("  accordion_padding = {}", config.accordion_padding);

    println!();
    match launchd::status() {
        (true, true) => println!("Focus-history daemon: installed and running"),
        (true, false) => println!(
            "Focus-history daemon: installed but not running (try 'snap daemon install' again)"
        ),
        (false, _) => println!(
            "Focus-history daemon: not installed (run 'snap daemon install' to enable 'snap last')"
        ),
    }
    println!();

    let stage_manager_on = display::stage_manager_enabled();
    if stage_manager_on && config.stage_manager_width > 0.0 {
        println!(
            "Stage Manager: on (inset {} applied)",
            config.stage_manager_width
        );
    } else if stage_manager_on {
        println!("Stage Manager: on (inset ignored — stage_manager_width = 0)");
    } else {
        println!("Stage Manager: off (inset ignored)");
    }
    println!();

    let displays = display::ordered_displays(stage_manager_width).unwrap_or_default();
    let focused_rect = window::Window::focused().ok().and_then(|w| w.rect().ok());
    let current_index = focused_rect.map(|r| display::display_index_containing(&displays, r));

    println!("Displays (left-to-right, then top-to-bottom):");
    if displays.is_empty() {
        println!("  (none found)");
    }
    for (i, d) in displays.iter().enumerate() {
        let marker = if current_index == Some(i) {
            "  [current]"
        } else {
            ""
        };
        println!(
            "  {}. {}x{} usable {}x{} origin ({}, {}){marker}",
            i + 1,
            d.frame.width,
            d.frame.height,
            d.usable.width,
            d.usable.height,
            d.frame.x,
            d.frame.y,
        );
    }
    println!();

    match focused_rect {
        None => println!("Focused: no focused window"),
        Some(rect) => {
            let display_index = current_index.map(|i| i + 1);
            let label = display_index
                .and_then(|i| {
                    let d = displays.get(i - 1)?;
                    let candidates = window::visible_windows_on(d.frame).ok()?;
                    candidates
                        .into_iter()
                        .find(|c| rects_roughly_equal(c.rect, rect))
                })
                .map(|c| {
                    let title = c.title.as_deref().unwrap_or("");
                    format!("{} — \"{title}\"", c.app_name)
                })
                .unwrap_or_else(|| "unknown".to_string());
            println!(
                "Focused: {label}  frame ({}, {}, {}, {})  display {}",
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                display_index
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "?".into())
            );
        }
    }

    Ok(())
}

fn run_undo(stage_manager_width: f64) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let current_rect = focused.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(current_rect, stage_manager_width).map_err(runtime_failure)?;

    let candidates = window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    let window_number = candidates
        .iter()
        .find(|c| rects_roughly_equal(c.rect, current_rect))
        .map(|c| c.window_number)
        .ok_or_else(|| ExitError("error: nothing to undo".into(), EXIT_RUNTIME_FAILURE))?;

    let previous = undo::take_and_swap(window_number, current_rect)
        .ok_or_else(|| ExitError("error: nothing to undo".into(), EXIT_RUNTIME_FAILURE))?;
    focused.set_rect(previous).map_err(runtime_failure)
}

/// `snap last` — toggles focus to whichever window was focused immediately
/// before the current one, per the focus-watch daemon's history.
fn run_last() -> anyhow::Result<()> {
    // Check the daemon itself before trusting anything in the history file:
    // a `previous` entry left over from a daemon that's no longer running
    // is stale by definition, and "previous window is no longer available"
    // would otherwise be a misleading way to say "there's no daemon
    // keeping this up to date" — the fix here is 'snap daemon install',
    // not "try switching windows again."
    let (installed, loaded) = launchd::status();
    if !loaded {
        let message = if installed {
            "error: focus-history daemon is installed but not running — run 'snap daemon install' again"
        } else {
            "error: focus-history daemon isn't installed — run 'snap daemon install'"
        };
        return Err(ExitError(message.into(), EXIT_RUNTIME_FAILURE).into());
    }

    match history::toggle(|target| {
        let window = window::find_window(target.pid, target.window_number).ok()?;
        window.raise().ok()?;
        window::activate_app(target.pid);
        Some(())
    }) {
        Ok(()) => Ok(()),
        Err(history::LastError::NoHistory) => Err(ExitError(
            "error: no focus history yet — switch to a couple of windows, then try again".into(),
            EXIT_RUNTIME_FAILURE,
        )
        .into()),
        Err(history::LastError::Unavailable) => Err(ExitError(
            "error: previous window is no longer available".into(),
            EXIT_RUNTIME_FAILURE,
        )
        .into()),
    }
}

/// `snap daemon install|uninstall|run`.
fn run_daemon(action: DaemonCommand) -> anyhow::Result<()> {
    match action {
        DaemonCommand::Install => {
            launchd::install().map_err(runtime_failure)?;
            println!("snap focus-history daemon installed and running.");
            Ok(())
        }
        DaemonCommand::Uninstall => {
            launchd::uninstall().map_err(runtime_failure)?;
            println!("snap focus-history daemon stopped and removed.");
            Ok(())
        }
        DaemonCommand::Run => {
            // `accessibility::is_trusted()` can report `true` here even
            // when it shouldn't: interactively-run snap commands inherit
            // Terminal's own Accessibility trust as their "responsible
            // process," but launchd has no such parent to inherit from, so
            // this process is evaluated on its own — separately from every
            // other snap invocation on the machine. It is genuinely
            // untrusted the first time the daemon ever runs.
            //
            // Don't just prompt-and-exit: under launchd's `KeepAlive`, an
            // immediate exit triggers an immediate relaunch, which would
            // re-prompt (and re-register a fresh TCC entry) every time,
            // racing the user's own attempt to grant trust in System
            // Settings before it can ever stick. Prompt once, then poll
            // quietly until trust is granted, so restart never happens
            // mid-grant.
            let debug = std::env::var_os("SNAP_DEBUG").is_some();
            if !accessibility::is_trusted() {
                if debug {
                    eprintln!("[snap debug] daemon: not trusted at startup, prompting and waiting");
                }
                accessibility::prompt_for_trust();
                while !accessibility::is_trusted() {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    if debug {
                        eprintln!("[snap debug] daemon: still waiting for trust");
                    }
                }
            }
            if debug {
                eprintln!("[snap debug] daemon: trusted, entering focus_watch::run()");
            }
            focus_watch::run().map_err(runtime_failure)
        }
    }
}

fn run_tile(gap: f64, stage_manager_width: f64, layout: TileLayout) -> anyhow::Result<()> {
    let debug = std::env::var_os("SNAP_DEBUG").is_some();

    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let focused_rect = focused.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(focused_rect, stage_manager_width).map_err(runtime_failure)?;

    let mut candidates =
        window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    let focused_index = candidates
        .iter()
        .position(|c| rects_roughly_equal(c.rect, focused_rect));

    let mut ordered = Vec::with_capacity(candidates.len().max(1));
    match focused_index {
        Some(idx) => ordered.push(candidates.remove(idx)),
        // The focused window wasn't in the tileable candidate set (e.g. a
        // dialog Accessibility can still move) — fall back to a minimal
        // candidate for it directly rather than dropping it from the tile.
        None => ordered.push(window::TileCandidate {
            window: focused,
            rect: focused_rect,
            pid: 0,
            app_name: String::new(),
            title: None,
            window_number: -1,
        }),
    }
    ordered.extend(candidates);

    // The same padding value governs both the outer margin (window-to-screen-edge)
    // and the inter-tile gap, matching how `snap left/right/top/bottom/full/center`
    // apply it as a uniform screen-edge inset.
    let usable = padded(target_display.usable, gap);

    if debug {
        eprintln!("[snap debug] usable={usable:?} (padding={gap})");
        eprintln!("[snap debug] {} window(s) to tile", ordered.len());
    }

    let rects = tile::tile_rects_with_layout(usable, ordered.len(), gap, layout);
    for (candidate, rect) in ordered.into_iter().zip(rects) {
        // An individual unmanageable window is skipped, not fatal (PRD §23).
        let previous_rect = candidate.rect;
        let result = candidate.window.set_rect(rect);
        if result.is_ok() && candidate.window_number >= 0 {
            undo::record(candidate.window_number, previous_rect);
        }
        if debug {
            let after = candidate.window.rect();
            eprintln!("[snap debug] requested={rect:?} set_rect={result:?} actual_after={after:?}");
        }
    }
    Ok(())
}

/// `snap list` — read-only, the one command allowed to print on success
/// (PRD §29). Uses the same candidate set/filters as `snap tile`.
fn run_list(scope: ListScope, stage_manager_width: f64) -> anyhow::Result<()> {
    let focused_pid = window::frontmost_app_pid();
    let focused_rect = window::Window::focused().ok().and_then(|w| w.rect().ok());

    let displays = display::ordered_displays(stage_manager_width).map_err(runtime_failure)?;

    let mut rows: Vec<(usize, window::TileCandidate)> = Vec::new();
    match scope {
        ListScope::Current => {
            let window_rect = focused_rect.ok_or_else(|| {
                ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE)
            })?;
            let idx = display::display_index_containing(&displays, window_rect);
            let candidates =
                window::visible_windows_on(displays[idx].frame).map_err(runtime_failure)?;
            rows.extend(candidates.into_iter().map(|c| (idx, c)));
        }
        ListScope::All => {
            for (idx, d) in displays.iter().enumerate() {
                let candidates = window::visible_windows_on(d.frame).map_err(runtime_failure)?;
                rows.extend(candidates.into_iter().map(|c| (idx, c)));
            }
        }
    }

    // Focused first, then the existing top-to-bottom/left-to-right tile
    // order within (and across, for `--display all`) displays.
    if let Some(pos) = rows.iter().position(|(_, c)| {
        Some(c.pid) == focused_pid && focused_rect.is_some_and(|r| rects_roughly_equal(r, c.rect))
    }) {
        let focused_row = rows.remove(pos);
        rows.insert(0, focused_row);
    }

    println!(
        "{:<8} {:<20} {:<7} {:<7} TITLE",
        "ID", "APP", "DISPLAY", "FOCUSED"
    );
    for (i, (display_index, candidate)) in rows.iter().enumerate() {
        let is_focused = i == 0
            && Some(candidate.pid) == focused_pid
            && focused_rect.is_some_and(|r| rects_roughly_equal(r, candidate.rect));
        println!(
            "{:<8} {:<20} {:<7} {:<7} {}",
            candidate.window_number,
            candidate.app_name,
            display_index + 1,
            if is_focused { "*" } else { "" },
            candidate.title.as_deref().unwrap_or(""),
        );
    }
    Ok(())
}

/// `snap focus left|right|up|down` — raises/activates the nearest window in
/// `direction` on the current display. Never moves or resizes a window.
fn run_focus(direction: Direction, stage_manager_width: f64) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let focused_rect = focused.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(focused_rect, stage_manager_width).map_err(runtime_failure)?;

    let mut candidates =
        window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    candidates.retain(|c| !rects_roughly_equal(c.rect, focused_rect));

    let rects: Vec<Rect> = candidates.iter().map(|c| c.rect).collect();
    let index = neighbor_in_direction(focused_rect, &rects, direction).ok_or_else(|| {
        ExitError(
            format!("error: no window to the {}", direction_word(direction)),
            EXIT_RUNTIME_FAILURE,
        )
    })?;

    let target = &candidates[index];
    target.window.raise().map_err(runtime_failure)?;
    window::activate_app(target.pid);
    Ok(())
}

/// `snap swap left|right|up|down` — exchanges frames with the nearest
/// window in `direction` on the current display. Focus stays on the
/// originally focused window (it just moved).
fn run_swap(direction: Direction, stage_manager_width: f64) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let focused_rect = focused.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(focused_rect, stage_manager_width).map_err(runtime_failure)?;

    let mut candidates =
        window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    candidates.retain(|c| !rects_roughly_equal(c.rect, focused_rect));

    let rects: Vec<Rect> = candidates.iter().map(|c| c.rect).collect();
    let index = neighbor_in_direction(focused_rect, &rects, direction).ok_or_else(|| {
        ExitError(
            format!("error: no window to the {}", direction_word(direction)),
            EXIT_RUNTIME_FAILURE,
        )
    })?;

    let neighbor = &candidates[index];
    let neighbor_rect = neighbor.rect;

    focused.set_rect(neighbor_rect).map_err(runtime_failure)?;
    if let Err(err) = neighbor.window.set_rect(focused_rect) {
        // Best-effort restore: the first half already moved, so put the
        // focused window back rather than leaving a half-applied swap.
        let _ = focused.set_rect(focused_rect);
        return Err(runtime_failure(err));
    }
    Ok(())
}

fn direction_word(direction: Direction) -> &'static str {
    match direction {
        Direction::Left => "left",
        Direction::Right => "right",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

/// `snap stack [next|previous]` — one-shot accordion on the current
/// display: one window fills usable bounds, the rest peek from the edges.
/// Uses the same candidate set as `snap tile`.
fn run_stack(
    action: Option<StackAction>,
    padding: f64,
    stage_manager_width: f64,
    accordion_padding: f64,
) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let focused_rect = focused.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(focused_rect, stage_manager_width).map_err(runtime_failure)?;

    let mut candidates =
        window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    let focused_index = candidates
        .iter()
        .position(|c| rects_roughly_equal(c.rect, focused_rect));

    let mut all = Vec::with_capacity(candidates.len().max(1));
    match focused_index {
        Some(idx) => all.push(candidates.remove(idx)),
        None => all.push(window::TileCandidate {
            window: focused,
            rect: focused_rect,
            pid: 0,
            app_name: String::new(),
            title: None,
            window_number: -1,
        }),
    }
    all.extend(candidates); // all[0] = focused; the rest in tile (visual) order.

    let n = all.len();
    let usable = padded(target_display.usable, padding);

    // Tile (visual) order over the same candidate set, independent of
    // which one is focused — used to detect/cycle an existing accordion.
    let mut visual_order: Vec<usize> = (0..n).collect();
    visual_order.sort_by(|&a, &b| {
        all[a]
            .rect
            .y
            .partial_cmp(&all[b].rect.y)
            .unwrap()
            .then(all[a].rect.x.partial_cmp(&all[b].rect.x).unwrap())
    });

    // Fresh cascade: rest in tile order (bottom of the stack first), focused
    // last (front — flush against the trailing edge, on top).
    let fresh_order = || -> Vec<usize> {
        let mut order: Vec<usize> = visual_order.iter().copied().filter(|&i| i != 0).collect();
        order.push(0);
        order
    };

    match action {
        None => {
            if n == 1 {
                return all[0].window.set_rect(usable).map_err(runtime_failure);
            }
            let order = fresh_order();
            apply_cascade(&all, &order, usable, accordion_padding);
            raise_and_activate(&all[*order.last().unwrap()]).map_err(runtime_failure)
        }
        Some(direction) => {
            if n == 1 {
                return Err(
                    ExitError("error: only one window".into(), EXIT_RUNTIME_FAILURE).into(),
                );
            }
            let frames: Vec<Rect> = all.iter().map(|c| c.rect).collect();
            let mut order = accordion::detect_order(usable, &frames)
                // Not stacked yet: treat as `stack` (focused as front) then advance once.
                .unwrap_or_else(fresh_order);

            match direction {
                StackAction::Next => order.rotate_right(1),
                StackAction::Previous => order.rotate_left(1),
            }
            apply_cascade(&all, &order, usable, accordion_padding);
            raise_and_activate(&all[*order.last().unwrap()]).map_err(runtime_failure)
        }
    }
}

/// Applies the cascade layout, best-effort — an individual unmanageable
/// window is skipped, not fatal (same policy as `snap tile`, PRD §23).
///
/// Every window is the same size (see `accordion::cascade_rects`); the peek
/// effect comes entirely from z-order, so each window is raised in bottom-
/// to-top order (`order[0]` first, the front last) — otherwise a window
/// placed correctly but left behind in z-order would cover the ones meant
/// to be in front of it.
fn apply_cascade(all: &[window::TileCandidate], order: &[usize], usable: Rect, peek: f64) {
    let n = order.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        let _ = all[order[0]].window.set_rect(usable);
        return;
    }
    let debug = std::env::var_os("SNAP_DEBUG").is_some();
    let rects = accordion::cascade_rects(usable, n, peek);
    for (slot, &idx) in order.iter().enumerate() {
        let candidate = &all[idx];
        let set_result = candidate.window.set_rect(rects[slot]);
        let _ = candidate.window.raise();
        if debug {
            eprintln!(
                "[snap debug] cascade slot={slot} idx={idx} rect={:?} set_result={set_result:?} readback={:?}",
                rects[slot],
                candidate.window.rect()
            );
        }
    }
}

fn raise_and_activate(candidate: &window::TileCandidate) -> Result<(), anyhow::Error> {
    candidate.window.raise()?;
    window::activate_app(candidate.pid);
    Ok(())
}

/// Used to match a window's own `Window::rect()` (Accessibility) reading
/// against its `TileCandidate::rect` (CGWindowList) reading for the same
/// window — e.g. excluding the focused window from `tile`/`focus`/`swap`
/// candidates, or looking up its `window_number` for `undo`. `EPS` matches
/// `window::rects_roughly_equal`'s tolerance for that exact cross-source
/// comparison (AX and CG occasionally disagree by a point or two); `1.0`
/// was too tight and could miss the match, leaving the focused window in
/// its own candidate set or `undo` unable to find its identity.
fn rects_roughly_equal(a: Rect, b: Rect) -> bool {
    const EPS: f64 = 2.0;
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.width - b.width).abs() < EPS
        && (a.height - b.height).abs() < EPS
}

fn runtime_failure(err: anyhow::Error) -> anyhow::Error {
    ExitError(format!("error: {err}"), EXIT_RUNTIME_FAILURE).into()
}
