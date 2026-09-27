mod accessibility;
mod accordion;
mod animation;
mod ax;
mod cli;
mod config;
mod display;
mod error_log;
mod json;
mod layout;
mod legacy_cleanup;
mod named_layout;
mod spatial;
mod tile;
mod undo;
mod window;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command, LegacyDaemonCommand, ListScope, StackAction};
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
    let animation_generation = animation::Generation::now();
    let cli = Cli::parse();
    legacy_cleanup::remove_legacy_daemon();
    match run(cli, animation_generation) {
        Ok(()) => ExitCode::from(EXIT_SUCCESS),
        Err(err) => {
            if let Some(exit_err) = err.downcast_ref::<ExitError>() {
                if !exit_err.0.to_string().is_empty() {
                    error_log::record("cli", &exit_err.0);
                    eprintln!("{}", exit_err.0);
                }
                return ExitCode::from(exit_err.1);
            }
            let message = format!("error: {err}");
            error_log::record("cli", &message);
            eprintln!("{message}");
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

fn animate_one(
    window: &window::Window,
    from: Rect,
    to: Rect,
    animation_settings: animation::Settings,
    complete: impl FnOnce(),
) -> anyhow::Result<bool> {
    let transition = animation::Transition::new(window, from, to).map_err(runtime_failure)?;
    match animation::run(
        &[transition],
        animation_settings,
        || {},
        |applied| {
            if applied[0] {
                complete();
            }
            Ok(())
        },
    )
    .map_err(runtime_failure)?
    .remove(0)
    {
        animation::Outcome::Applied => Ok(true),
        animation::Outcome::Cancelled => Ok(false),
        animation::Outcome::Failed(error) => Err(runtime_failure(error)),
    }
}

fn outcome_label(outcome: &animation::Outcome) -> &'static str {
    match outcome {
        animation::Outcome::Applied => "applied",
        animation::Outcome::Failed(_) => "failed",
        animation::Outcome::Cancelled => "cancelled",
    }
}

fn run(cli: Cli, animation_generation: animation::Generation) -> anyhow::Result<()> {
    let (config, layouts) = config::load_all();
    let action = resolve_action(&cli, &config)?;

    // `doctor` reports Accessibility status rather than requiring it —
    // unlike every other command, it must still produce output (and exit
    // 0) when snap isn't trusted yet.
    if let Action::Doctor { json } = action {
        return run_doctor(&config, &layouts, config.stage_manager_width, json);
    }

    if let Action::Layout(None) = action {
        return list_layouts(&layouts);
    }

    if let Action::Layout(Some(LayoutAction::Apply(ref name))) = action {
        layout_specs(name, &layouts)?;
    }

    if let Action::LegacyDaemonCleanup = action {
        return Ok(());
    }

    let app = cli.app.as_deref();
    let window_id = cli.window;

    if !accessibility::is_trusted() {
        accessibility::prompt_for_trust();
        return Err(accessibility_unavailable());
    }

    let animation_settings = animation::configured(
        config.animation_duration,
        config.animations,
        animation_generation,
    );

    match action {
        Action::Tile { gap, layout } => run_tile(
            gap.unwrap_or(config.padding),
            config.stage_manager_width,
            layout,
            animation_settings,
        ),
        Action::Reposition(compute) => run_reposition(
            compute,
            config.padding,
            config.stage_manager_width,
            app,
            window_id,
            animation_settings,
        ),
        Action::Display(target) => run_display_move(
            target,
            config.padding,
            config.stage_manager_width,
            app,
            window_id,
            animation_settings,
        ),
        Action::List(scope, json) => run_list(scope, config.stage_manager_width, json),
        Action::Focus(direction) => run_focus(direction, config.stage_manager_width),
        Action::Swap(direction) => {
            run_swap(direction, config.stage_manager_width, animation_settings)
        }
        Action::Stack(action) => run_stack(
            action,
            config.padding,
            config.stage_manager_width,
            config.accordion_padding,
            animation_settings,
        ),
        Action::Undo => run_undo(config.stage_manager_width, animation_settings),
        Action::Layout(Some(LayoutAction::Apply(name))) => {
            run_layout(&name, &layouts, &config, animation_settings)
        }
        Action::Layout(Some(LayoutAction::Capture(name))) => run_capture(&name, &config),
        Action::Layout(None) => unreachable!("handled before the accessibility gate"),
        Action::LegacyDaemonCleanup => unreachable!("handled before the accessibility gate"),
        Action::Doctor { .. } => unreachable!("handled above before the accessibility gate"),
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
    List(ListScope, bool),
    Focus(Direction),
    Swap(Direction),
    Stack(Option<StackAction>),
    Undo,
    LegacyDaemonCleanup,
    Doctor {
        json: bool,
    },
    Layout(Option<LayoutAction>),
}

enum LayoutAction {
    Apply(String),
    Capture(String),
}

fn resolve_action(cli: &Cli, config: &config::Config) -> anyhow::Result<Action> {
    if cli.window.is_some() {
        let supported = cli.command.as_ref().is_none_or(|command| {
            command.as_position_and_size().is_some()
                || matches!(
                    command,
                    Command::Full
                        | Command::Center
                        | Command::Grow
                        | Command::Shrink
                        | Command::Almost
                        | Command::Third { .. }
                        | Command::Display { .. }
                )
        });
        if !supported {
            return Err(invalid_args(
                "error: --window is not supported with this command",
            ));
        }
    }
    if let Some(command) = &cli.command {
        if cli.app.is_some() && matches!(command, Command::Layout { .. }) {
            return Err(invalid_args("error: --app is not supported with layout"));
        }
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
            Command::List { display, json } => Ok(Action::List(*display, *json)),
            Command::Focus { direction } => Ok(Action::Focus(*direction)),
            Command::Swap { direction } => Ok(Action::Swap(*direction)),
            Command::Stack { action } => Ok(Action::Stack(*action)),
            Command::Undo => Ok(Action::Undo),
            Command::LegacyDaemonCleanup {
                action: LegacyDaemonCommand::Run,
            } => Ok(Action::LegacyDaemonCleanup),
            Command::Doctor { json } => Ok(Action::Doctor { json: *json }),
            Command::Layout { name, capture_name } => {
                let action = match (name.as_deref(), capture_name.as_deref()) {
                    (None, None) => None,
                    (Some("capture"), Some(name)) if config::valid_layout_name(name) => {
                        Some(LayoutAction::Capture(name.into()))
                    }
                    (Some(name), None) if config::valid_layout_name(name) => {
                        Some(LayoutAction::Apply(name.into()))
                    }
                    _ => {
                        return Err(invalid_args(
                            "error: invalid layout name or capture command",
                        ));
                    }
                };
                Ok(Action::Layout(action))
            }
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
    window_id: Option<i64>,
    stage_manager_width: f64,
) -> anyhow::Result<(window::Window, Rect, Option<i64>)> {
    if let Some(id) = window_id {
        let candidate = find_window_by_id(id, stage_manager_width)?;
        return Ok((candidate.window, candidate.rect, Some(id)));
    }
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

fn find_window_by_id(id: i64, stage_manager_width: f64) -> anyhow::Result<window::TileCandidate> {
    let displays = display::ordered_displays(stage_manager_width).map_err(runtime_failure)?;
    for display in displays {
        if let Some(candidate) = window::visible_windows_on(display.frame)
            .map_err(runtime_failure)?
            .into_iter()
            .find(|candidate| candidate.window_number == id)
        {
            return Ok(candidate);
        }
    }
    Err(ExitError(
        format!("error: no window with id {id}"),
        EXIT_RUNTIME_FAILURE,
    )
    .into())
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
    window_id: Option<i64>,
    animation_duration: animation::Settings,
) -> anyhow::Result<()> {
    let (target, window_rect, window_number) = resolve_target(app, window_id, stage_manager_width)?;
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
    animate_one(&target, window_rect, rect, animation_duration, || {
        if let Some(window_number) = window_number {
            undo::record(window_number, window_rect);
        }
    })?;
    Ok(())
}

fn run_display_move(
    target: DisplayTarget,
    padding: f64,
    stage_manager_width: f64,
    app: Option<&str>,
    window_id: Option<i64>,
    animation_duration: animation::Settings,
) -> anyhow::Result<()> {
    let (focused, window_rect, window_number) =
        resolve_target(app, window_id, stage_manager_width)?;

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
    animate_one(&focused, window_rect, new_rect, animation_duration, || {
        if let Some(window_number) = window_number {
            undo::record(window_number, window_rect);
        }
    })?;
    Ok(())
}

/// `snap undo` — restores the focused window to its previously recorded
/// frame, then swaps the cache entry so a second `undo` toggles back.
/// `snap doctor` — read-only diagnostic report (PRD issue #10). Unlike
/// every other command it does not require Accessibility trust to run: it
/// reports trust status as one line among several, exiting 0 as long as it
/// could produce a report at all.
fn run_doctor(
    config: &config::Config,
    layouts: &[config::NamedLayout],
    stage_manager_width: f64,
    json_output: bool,
) -> anyhow::Result<()> {
    if json_output {
        return run_doctor_json(config, layouts, stage_manager_width);
    }
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
    println!("  animations = {}", config.animations);
    println!("  animation_duration = {}", config.animation_duration);
    println!("Layouts:");
    for layout in layouts {
        let invalid: Vec<_> = layout
            .entries
            .iter()
            .filter(|(_, raw)| named_layout::parse(raw).is_none())
            .map(|(app, _)| app.as_str())
            .collect();
        println!(
            "  {}: {} entries{}",
            layout.name,
            layout.entries.len(),
            if invalid.is_empty() {
                String::new()
            } else {
                format!(" (invalid: {})", invalid.join(", "))
            }
        );
    }
    match error_log::path() {
        Some(path) => println!("Error log: {}", path.display()),
        None => println!("Error log: unavailable ($HOME not set)"),
    }

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

fn run_doctor_json(
    config: &config::Config,
    layouts: &[config::NamedLayout],
    stage_manager_width: f64,
) -> anyhow::Result<()> {
    let trusted = accessibility::is_trusted();
    let config_path = config::config_path();
    let found = config_path.as_ref().is_some_and(|path| path.exists());
    let stage_manager_on = display::stage_manager_enabled();
    let displays = display::ordered_displays(stage_manager_width).unwrap_or_default();
    let focused_rect = window::Window::focused().ok().and_then(|w| w.rect().ok());
    let current_index = focused_rect.map(|rect| display::display_index_containing(&displays, rect));
    let mut out = format!(
        "{{\"version\":{},\"binary\":{},\"accessibility\":{{\"trusted\":{trusted}}},\"config\":{{\"path\":{},\"found\":{found},\"padding\":{},\"stage_manager_width\":{},\"almost_padding\":{},\"accordion_padding\":{},\"animations\":{},\"animation_duration\":{}}},\"error_log\":{},\"stage_manager\":{{\"enabled\":{stage_manager_on},\"inset_applied\":{}}},\"layouts\":[",
        json::string(env!("CARGO_PKG_VERSION")),
        json::optional(
            std::env::current_exe()
                .ok()
                .as_ref()
                .and_then(|p| p.to_str())
        ),
        json::optional(config_path.as_ref().and_then(|p| p.to_str())),
        json::number(config.padding),
        json::number(config.stage_manager_width),
        json::number(config.almost_padding),
        json::number(config.accordion_padding),
        config.animations,
        config.animation_duration,
        json::optional(error_log::path().as_ref().and_then(|p| p.to_str())),
        stage_manager_on && config.stage_manager_width > 0.0
    );
    for (i, layout) in layouts.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let invalid: Vec<_> = layout
            .entries
            .iter()
            .filter(|(_, raw)| named_layout::parse(raw).is_none())
            .map(|(app, _)| json::string(app))
            .collect();
        out.push_str(&format!(
            "{{\"name\":{},\"entries\":{},\"invalid\":[{}]}}",
            json::string(&layout.name),
            layout.entries.len(),
            invalid.join(",")
        ));
    }
    out.push_str("],\"displays\":[");
    for (i, display) in displays.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"index\":{},\"current\":{},\"frame\":{},\"usable\":{}}}",
            i + 1,
            current_index == Some(i),
            json::rect(display.frame),
            json::rect(display.usable)
        ));
    }
    out.push_str("],\"focused\":");
    match focused_rect {
        None => out.push_str("null"),
        Some(rect) => {
            let candidate = current_index
                .and_then(|i| displays.get(i))
                .and_then(|d| window::visible_windows_on(d.frame).ok())
                .and_then(|candidates| {
                    candidates
                        .into_iter()
                        .find(|c| rects_roughly_equal(c.rect, rect))
                });
            let app = candidate.as_ref().map(|c| c.app_name.as_str());
            let title = candidate.as_ref().and_then(|c| c.title.as_deref());
            out.push_str(&format!(
                "{{\"app\":{},\"title\":{},\"display\":{},\"frame\":{}}}",
                json::optional(app),
                json::optional(title),
                current_index
                    .map(|i| (i + 1).to_string())
                    .unwrap_or_else(|| "null".into()),
                json::rect(rect)
            ));
        }
    }
    out.push('}');
    println!("{out}");
    Ok(())
}

fn run_undo(
    stage_manager_width: f64,
    animation_duration: animation::Settings,
) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let snapshot = animation::coordinate(|| -> anyhow::Result<_> {
        let current_rect = focused.rect().map_err(runtime_failure)?;
        let target_display = display::target_display_for(current_rect, stage_manager_width)
            .map_err(runtime_failure)?;
        let candidates =
            window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
        let window_number = candidates
            .iter()
            .find(|c| rects_roughly_equal(c.rect, current_rect))
            .map(|c| c.window_number)
            .ok_or_else(|| ExitError("error: nothing to undo".into(), EXIT_RUNTIME_FAILURE))?;
        let previous = undo::previous_group(window_number);
        if previous.is_empty() {
            return Err(ExitError("error: nothing to undo".into(), EXIT_RUNTIME_FAILURE).into());
        }
        Ok(previous)
    })
    .map_err(runtime_failure)?;
    let previous = snapshot?;
    let displays = display::ordered_displays(stage_manager_width).map_err(runtime_failure)?;
    let mut candidates = Vec::new();
    for display in displays {
        candidates.extend(window::visible_windows_on(display.frame).map_err(runtime_failure)?);
    }
    let mut moves = Vec::new();
    for (id, rect) in previous {
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.window_number == id)
        {
            moves.push((candidate, rect));
        }
    }
    let mut transitions = Vec::new();
    let mut indices = Vec::new();
    for (index, (candidate, rect)) in moves.iter().enumerate() {
        if let Ok(transition) = animation::Transition::new(&candidate.window, candidate.rect, *rect)
        {
            transitions.push(transition);
            indices.push(index);
        }
    }
    if transitions.is_empty() {
        return Err(ExitError("error: nothing to undo".into(), EXIT_RUNTIME_FAILURE).into());
    }
    animation::run(
        &transitions,
        animation_duration,
        || {},
        |applied| {
            let toggled: Vec<_> = applied
                .iter()
                .zip(&indices)
                .filter_map(|(&applied, &index)| {
                    if applied {
                        let c = moves[index].0;
                        Some((c.window_number, c.rect))
                    } else {
                        None
                    }
                })
                .collect();
            if toggled.len() > 1 {
                undo::record_group(&toggled);
            } else if let Some(&(id, rect)) = toggled.first() {
                undo::record(id, rect);
            }
            Ok(())
        },
    )
    .map_err(runtime_failure)?;
    Ok(())
}

fn run_tile(
    gap: f64,
    stage_manager_width: f64,
    layout: TileLayout,
    animation_duration: animation::Settings,
) -> anyhow::Result<()> {
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
    let mut prepared = Vec::new();
    let mut indices = Vec::new();
    for (index, (candidate, &rect)) in ordered.iter().zip(&rects).enumerate() {
        // An individual unmanageable window is skipped, not fatal (PRD §23).
        if let Ok(transition) = animation::Transition::new(&candidate.window, candidate.rect, rect)
        {
            prepared.push(transition);
            indices.push(index);
        }
    }
    let outcomes = animation::run(
        &prepared,
        animation_duration,
        || {},
        |applied| {
            let moved: Vec<_> = applied
                .iter()
                .zip(&indices)
                .filter_map(|(&applied, &index)| {
                    let candidate = &ordered[index];
                    (applied && candidate.window_number >= 0)
                        .then_some((candidate.window_number, candidate.rect))
                })
                .collect();
            undo::record_group(&moved);
            Ok(())
        },
    )
    .map_err(runtime_failure)?;
    for (outcome, &index) in outcomes.iter().zip(&indices) {
        let candidate = &ordered[index];
        if debug {
            let requested = rects[index];
            let outcome = outcome_label(outcome);
            let after = candidate.window.rect();
            eprintln!(
                "[snap debug] requested={requested:?} animation={outcome} actual_after={after:?}"
            );
        }
    }
    Ok(())
}

/// `snap list` — read-only. Uses the same candidate set/filters as `snap tile`.
type ListRows = (
    Vec<(usize, window::TileCandidate)>,
    Option<i32>,
    Option<Rect>,
);

fn list_rows(scope: ListScope, stage_manager_width: f64) -> anyhow::Result<ListRows> {
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

    Ok((rows, focused_pid, focused_rect))
}

fn run_list(scope: ListScope, stage_manager_width: f64, json: bool) -> anyhow::Result<()> {
    let (rows, focused_pid, focused_rect) = list_rows(scope, stage_manager_width)?;
    if json {
        let mut out = String::from("{\"windows\":[");
        for (i, (display_index, candidate)) in rows.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let is_focused = i == 0
                && Some(candidate.pid) == focused_pid
                && focused_rect.is_some_and(|r| rects_roughly_equal(r, candidate.rect));
            out.push_str(&format!("{{\"id\":{},\"app\":{},\"pid\":{},\"display\":{},\"focused\":{},\"title\":{},\"frame\":{}}}",
                candidate.window_number, json::string(&candidate.app_name), candidate.pid, display_index + 1,
                is_focused, candidate.title.as_deref().map(json::string).unwrap_or_else(|| "null".into()), json::rect(candidate.rect)));
        }
        out.push_str("]}");
        println!("{out}");
        return Ok(());
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

fn list_layouts(layouts: &[config::NamedLayout]) -> anyhow::Result<()> {
    if layouts.is_empty() {
        println!("No layouts configured; add [layouts.name] to ~/.config/snap.toml");
    }
    for layout in layouts {
        println!("{}:", layout.name);
        for (app, spec) in &layout.entries {
            println!("  {app} = \"{spec}\"");
        }
    }
    Ok(())
}

fn layout_specs<'a>(
    name: &str,
    layouts: &'a [config::NamedLayout],
) -> anyhow::Result<Vec<(&'a str, named_layout::Spec)>> {
    let layout = layouts
        .iter()
        .find(|layout| layout.name == name)
        .ok_or_else(|| invalid_args(format!("error: no layout named '{name}'")))?;
    let mut seen = std::collections::HashSet::new();
    for (app, _) in &layout.entries {
        if !seen.insert(app.to_ascii_lowercase()) {
            return Err(invalid_args(format!(
                "error: layout '{name}': duplicate app '{app}'"
            )));
        }
    }
    layout
        .entries
        .iter()
        .map(|(app, raw)| {
            named_layout::parse(raw)
                .map(|spec| (app.as_str(), spec))
                .ok_or_else(|| {
                    invalid_args(format!(
                        "error: layout '{name}': invalid spec for '{app}': \"{raw}\""
                    ))
                })
        })
        .collect()
}

fn run_layout(
    name: &str,
    layouts: &[config::NamedLayout],
    config: &config::Config,
    animation_settings: animation::Settings,
) -> anyhow::Result<()> {
    let specs = layout_specs(name, layouts)?;
    let displays =
        display::ordered_displays(config.stage_manager_width).map_err(runtime_failure)?;
    let mut candidates = Vec::new();
    for (app, spec) in specs {
        let candidate = match find_app_window(app, config.stage_manager_width) {
            Ok(candidate) => candidate,
            Err(err) if err.to_string().starts_with("error: no window for app") => continue,
            Err(err) => return Err(err),
        };
        if candidates
            .iter()
            .any(|(existing, _): &(window::TileCandidate, Rect)| {
                existing.window_number == candidate.window_number
            })
        {
            return Err(invalid_args(format!(
                "error: layout '{name}': multiple entries target window {}",
                candidate.window_number
            )));
        }
        let current = display::display_index_containing(&displays, candidate.rect);
        let target = spec
            .display
            .and_then(|index| displays.get(index - 1))
            .unwrap_or(&displays[current]);
        let usable = padded(target.usable, config.padding);
        let rect = spec.rect(usable, candidate.rect, config.almost_padding);
        candidates.push((candidate, rect));
    }
    if candidates.is_empty() {
        return Err(ExitError(
            format!("error: no windows for layout '{name}'"),
            EXIT_RUNTIME_FAILURE,
        )
        .into());
    }
    let mut prepared = Vec::new();
    let mut indices = Vec::new();
    for (index, (candidate, rect)) in candidates.iter().enumerate() {
        if let Ok(transition) = animation::Transition::new(&candidate.window, candidate.rect, *rect)
        {
            prepared.push(transition);
            indices.push(index);
        }
    }
    animation::run(
        &prepared,
        animation_settings,
        || {},
        |applied| {
            let moved: Vec<_> = applied
                .iter()
                .zip(&indices)
                .filter_map(|(&applied, &index)| {
                    applied.then_some((candidates[index].0.window_number, candidates[index].0.rect))
                })
                .collect();
            undo::record_group(&moved);
            Ok(())
        },
    )
    .map_err(runtime_failure)?;
    Ok(())
}

fn run_capture(name: &str, config: &config::Config) -> anyhow::Result<()> {
    let (candidates, _, _) = list_rows(ListScope::All, config.stage_manager_width)?;
    let displays =
        display::ordered_displays(config.stage_manager_width).map_err(runtime_failure)?;
    let usable: Vec<_> = displays
        .iter()
        .map(|display| padded(display.usable, config.padding))
        .collect();
    let rows: Vec<_> = candidates
        .into_iter()
        .map(|(index, candidate)| (index, candidate.app_name, candidate.rect))
        .collect();
    let snippet = capture_snippet(name, &rows, &usable, config.almost_padding)
        .map_err(|skipped| capture_error(&skipped))?;
    println!("{snippet}");
    Ok(())
}

fn capture_error(skipped: &[String]) -> ExitError {
    let mut message = String::from("error: no windows to capture");
    for reason in skipped {
        message.push('\n');
        message.push_str(reason.trim_start_matches("# "));
    }
    ExitError(message, EXIT_RUNTIME_FAILURE)
}

fn capture_snippet(
    name: &str,
    rows: &[(usize, String, Rect)],
    usable: &[Rect],
    almost_padding: f64,
) -> Result<String, Vec<String>> {
    let mut counts = std::collections::HashMap::new();
    for (_, app, _) in rows {
        *counts.entry(app.as_str()).or_insert(0usize) += 1;
    }
    let mut lines = vec![format!("[layouts.{name}]")];
    let mut captured = 0;
    for (row_index, (index, app, rect)) in rows.iter().enumerate() {
        if counts[app.as_str()] > 1 {
            if rows
                .iter()
                .take(row_index)
                .any(|(_, prior_app, _)| prior_app == app)
            {
                continue;
            }
            lines.push(format!(
                "# {app}: {} windows open; layouts target one window per app, skipped",
                counts[app.as_str()]
            ));
            continue;
        }
        match named_layout::capture(usable[*index], *rect, almost_padding) {
            Some(mut spec) => {
                if usable.len() > 1 {
                    spec.push_str(&format!(" on {}", index + 1));
                }
                let key = if app
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    app.clone()
                } else {
                    format!("\"{}\"", app.replace('\\', "\\\\").replace('"', "\\\""))
                };
                lines.push(format!("{key} = \"{spec}\""));
                captured += 1;
            }
            None => lines.push(format!("# {app}: frame ({}, {}, {}x{}) doesn't match a snap placement; place it with snap and capture again",
                rect.x, rect.y, rect.width, rect.height)),
        }
    }
    if captured == 0 {
        return Err(lines.into_iter().skip(1).collect());
    }
    Ok(lines.join("\n"))
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
fn run_swap(
    direction: Direction,
    stage_manager_width: f64,
    animation_duration: animation::Settings,
) -> anyhow::Result<()> {
    let focused = window::Window::focused()
        .map_err(|_| ExitError("error: no focused window".into(), EXIT_RUNTIME_FAILURE))?;
    let focused_rect = focused.rect().map_err(runtime_failure)?;
    let target_display =
        display::target_display_for(focused_rect, stage_manager_width).map_err(runtime_failure)?;

    let mut candidates =
        window::visible_windows_on(target_display.frame).map_err(runtime_failure)?;
    candidates.retain(|c| !rects_roughly_equal(c.rect, focused_rect));

    let rects: Vec<Rect> = candidates.iter().map(|c| c.rect).collect();
    let Some(index) = swap_target_index(focused_rect, &rects, direction) else {
        return Ok(());
    };

    let neighbor = &candidates[index];
    let neighbor_rect = neighbor.rect;

    // Prepare both before moving either, preserving swap's all-or-nothing
    // validation for fixed-size or otherwise unmanageable windows.
    let focused_transition = animation::Transition::new(&focused, focused_rect, neighbor_rect)
        .map_err(runtime_failure)?;
    let neighbor_transition =
        animation::Transition::new(&neighbor.window, neighbor_rect, focused_rect)
            .map_err(runtime_failure)?;
    let outcomes = animation::run(
        &[focused_transition, neighbor_transition],
        animation_duration,
        || {},
        |_| Ok(()),
    )
    .map_err(runtime_failure)?;
    if let Some(error) = outcomes.into_iter().find_map(|outcome| match outcome {
        animation::Outcome::Failed(error) => Some(error),
        _ => None,
    }) {
        // Best-effort restore only while this failed swap still owns the
        // generation; an older process must never overwrite its successor.
        animation::if_current(animation_duration, || {
            let _ = focused.set_rect(focused_rect);
            let _ = neighbor.window.set_rect(neighbor_rect);
        })
        .map_err(runtime_failure)?;
        return Err(runtime_failure(error));
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

fn swap_target_index(focused_rect: Rect, rects: &[Rect], direction: Direction) -> Option<usize> {
    neighbor_in_direction(focused_rect, rects, direction)
}

/// `snap stack [next|previous]` — one-shot accordion on the current
/// display: one window fills usable bounds, the rest peek from the edges.
/// Uses the same candidate set as `snap tile`.
fn run_stack(
    action: Option<StackAction>,
    padding: f64,
    stage_manager_width: f64,
    accordion_padding: f64,
    animation_duration: animation::Settings,
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
                return animate_one(
                    &all[0].window,
                    all[0].rect,
                    usable,
                    animation_duration,
                    || {
                        if all[0].window_number >= 0 {
                            undo::record(all[0].window_number, all[0].rect);
                        }
                    },
                )
                .map(|_| ());
            }
            let order = fresh_order();
            apply_cascade(&all, &order, usable, accordion_padding, animation_duration)
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
            apply_cascade(&all, &order, usable, accordion_padding, animation_duration)
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
fn apply_cascade(
    all: &[window::TileCandidate],
    order: &[usize],
    usable: Rect,
    peek: f64,
    animation_duration: animation::Settings,
) -> anyhow::Result<()> {
    let n = order.len();
    if n == 0 {
        return Ok(());
    }
    if n == 1 {
        let candidate = &all[order[0]];
        return animate_one(
            &candidate.window,
            candidate.rect,
            usable,
            animation_duration,
            || {
                if candidate.window_number >= 0 {
                    undo::record(candidate.window_number, candidate.rect);
                }
            },
        )
        .map(|_| ());
    }
    let debug = std::env::var_os("SNAP_DEBUG").is_some();
    let rects = accordion::cascade_rects(usable, n, peek);
    let mut prepared = Vec::new();
    let mut slots = Vec::new();
    for (slot, &idx) in order.iter().enumerate() {
        let candidate = &all[idx];
        if let Ok(transition) =
            animation::Transition::new(&candidate.window, candidate.rect, rects[slot])
        {
            prepared.push(transition);
            slots.push((slot, idx));
        }
    }
    let front = &all[*order.last().unwrap()];
    let outcomes = animation::run(
        &prepared,
        animation_duration,
        || {
            for &idx in order {
                let _ = all[idx].window.raise();
            }
        },
        |applied| {
            let moved: Vec<_> = applied
                .iter()
                .zip(&slots)
                .filter_map(|(&applied, &(_, idx))| {
                    let candidate = &all[idx];
                    (applied && candidate.window_number >= 0)
                        .then_some((candidate.window_number, candidate.rect))
                })
                .collect();
            undo::record_group(&moved);
            raise_and_activate(front)
        },
    )
    .map_err(runtime_failure)?;
    if debug {
        for (outcome, &(slot, idx)) in outcomes.iter().zip(&slots) {
            let candidate = &all[idx];
            let outcome = outcome_label(outcome);
            eprintln!(
                "[snap debug] cascade slot={slot} idx={idx} rect={:?} animation={outcome} readback={:?}",
                rects[slot],
                candidate.window.rect()
            );
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_id_rejects_display_wide_commands() {
        let cli = Cli::try_parse_from(["snap", "--window", "3", "tile"]).unwrap();
        assert!(resolve_action(&cli, &config::Config::default()).is_err());
        let cli = Cli::try_parse_from(["snap", "--window", "3", "layout", "code"]).unwrap();
        assert!(resolve_action(&cli, &config::Config::default()).is_err());
    }

    #[test]
    fn layout_validation_rejects_unknown_invalid_and_duplicate_entries() {
        let layouts = vec![config::NamedLayout {
            name: "code".into(),
            entries: vec![
                ("Ghostty".into(), "left 60".into()),
                ("ghostty".into(), "right 40".into()),
            ],
        }];
        assert!(layout_specs("missing", &layouts).is_err());
        assert!(layout_specs("code", &layouts).is_err());
        let invalid = vec![config::NamedLayout {
            name: "code".into(),
            entries: vec![("Ghostty".into(), "left".into())],
        }];
        assert!(layout_specs("code", &invalid).is_err());
    }

    #[test]
    fn duplicate_app_capture_explains_why_visible_windows_were_skipped() {
        let usable = Rect::new(201.0, 55.0, 1583.0, 1033.0);
        let rows = vec![
            (0, "Ghostty".into(), Rect::new(201.0, 55.0, 784.0, 1033.0)),
            (0, "Ghostty".into(), Rect::new(1000.0, 55.0, 784.0, 1033.0)),
        ];
        assert_eq!(
            named_layout::capture(usable, rows[0].2, 48.0),
            Some("left 50".into())
        );
        assert_eq!(
            named_layout::capture(usable, rows[1].2, 48.0),
            Some("right 50".into())
        );
        let skipped = capture_snippet("code", &rows, &[usable], 48.0).unwrap_err();
        let error = capture_error(&skipped);
        assert!(error.0.contains("Ghostty: 2 windows open"), "{}", error.0);
    }

    #[test]
    fn swap_target_index_returns_none_at_display_edge() {
        let focused = Rect::new(0.0, 0.0, 400.0, 400.0);
        let right = Rect::new(400.0, 0.0, 400.0, 400.0);

        assert_eq!(swap_target_index(focused, &[right], Direction::Left), None);
    }
}
