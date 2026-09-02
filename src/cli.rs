use clap::{Parser, Subcommand};

use crate::layout::{DisplayTarget, Position, Third};
use crate::spatial::Direction;
use crate::tile::TileLayout;

#[derive(Parser, Debug)]
#[command(
    name = "snap",
    version,
    about = "Fast, minimal macOS window manipulation from the terminal",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// `snap <percent>` — resize the focused window, keeping it centered.
    /// Omit entirely to cycle through 25/50/75%.
    #[arg(value_name = "SIZE")]
    pub size: Option<u32>,

    /// Target a window by application name instead of the focused window.
    /// Exact match, case-insensitive, against the app name as CGWindowList
    /// (and Activity Monitor) report it. Applies to size, sides, corners,
    /// `full`, `center`, and `display`; not to `tile` or other display-wide
    /// commands.
    #[arg(long, global = true, value_name = "NAME")]
    pub app: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Anchor the focused window to the left. Omit SIZE to cycle 25/50/75%.
    Left { size: Option<u32> },
    /// Anchor the focused window to the right. Omit SIZE to cycle 25/50/75%.
    Right { size: Option<u32> },
    /// Anchor the focused window to the top. Omit SIZE to cycle 25/50/75%.
    Top { size: Option<u32> },
    /// Anchor the focused window to the bottom. Omit SIZE to cycle 25/50/75%.
    Bottom { size: Option<u32> },
    /// Anchor the focused window to the top-left corner. Omit SIZE to cycle 25/50/75%.
    #[command(name = "top-left")]
    TopLeft { size: Option<u32> },
    /// Anchor the focused window to the top-right corner. Omit SIZE to cycle 25/50/75%.
    #[command(name = "top-right")]
    TopRight { size: Option<u32> },
    /// Anchor the focused window to the bottom-left corner. Omit SIZE to cycle 25/50/75%.
    #[command(name = "bottom-left")]
    BottomLeft { size: Option<u32> },
    /// Anchor the focused window to the bottom-right corner. Omit SIZE to cycle 25/50/75%.
    #[command(name = "bottom-right")]
    BottomRight { size: Option<u32> },
    /// Fill the usable bounds of the current display.
    Full,
    /// Increase the focused window's size toward the usable bounds.
    Grow,
    /// Decrease the focused window's size toward a minimum.
    Shrink,
    /// Fill the usable area minus an extra inset (`almost_padding`), leaving
    /// the desktop edges visible. Not native fullscreen.
    Almost,
    /// Center the focused window without changing its size.
    Center,
    /// Tile visible windows on the current display.
    Tile {
        /// Named layout: `columns` (n equal columns), `rows` (n equal rows),
        /// or `master` (focused ~50% left, rest stacked right). Omit for the
        /// default deterministic 1/2/3/4/5+ assignment.
        #[arg(value_enum)]
        layout: Option<TileLayout>,
        /// Gap between tiles, in logical points. Defaults to the configured
        /// padding (see `~/.config/snap.toml`).
        #[arg(long, allow_hyphen_values = true)]
        gap: Option<f64>,
    },
    /// Anchor the focused window to a left/center/right third of the display.
    /// Omit POSITION to cycle left → center → right → left.
    #[command(alias = "thirds")]
    Third {
        #[arg(value_name = "POSITION", value_parser = parse_third)]
        position: Option<Third>,
    },
    /// Print visible, manipulable windows without moving anything.
    List {
        /// Restrict to the display containing the focused window (default),
        /// or list every attached display.
        #[arg(long, value_enum, default_value = "current")]
        display: ListScope,
    },
    /// Focus/raise the nearest window in a direction on the current
    /// display, without moving or resizing anything.
    Focus {
        #[arg(value_enum)]
        direction: Direction,
    },
    /// Exchange frames with the nearest window in a direction on the
    /// current display. Focus stays on the (now-moved) originally focused
    /// window.
    Swap {
        #[arg(value_enum)]
        direction: Direction,
    },
    /// One-shot accordion: one window fills the usable bounds, the rest peek
    /// from the edges. Omit ACTION to apply with the focused window as
    /// front; `next`/`previous` (alias `prev`) rotate which window is front.
    Stack {
        #[arg(value_name = "ACTION", value_parser = parse_stack_action)]
        action: Option<StackAction>,
    },
    /// Restore the focused window to its previous frame (toggles: a second
    /// `undo` returns to where the first one started).
    Undo,
    /// Focus the window that was focused immediately before the current
    /// one (toggle: a second `last` returns to where you started).
    /// Requires the focus-history daemon: `snap daemon install`.
    Last,
    /// Manage the optional background daemon that tracks focus history for
    /// `snap last`. snap has no daemon by default; this opts in.
    Daemon {
        #[command(subcommand)]
        action: DaemonCommand,
    },
    /// Print Accessibility trust, config, displays, and the focused window,
    /// for debugging a broken setup. Read-only; the one other command
    /// (besides `list`) that prints on success.
    Doctor,
    /// Move the focused window to another display, preserving its relative
    /// position and size.
    Display {
        /// `next`, `previous`, or a 1-based display index.
        #[arg(value_name = "TARGET", value_parser = parse_display_target)]
        target: DisplayTarget,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ListScope {
    Current,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackAction {
    Next,
    Previous,
}

#[derive(Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonCommand {
    /// Install and start the focus-history launch agent (runs at login).
    Install,
    /// Stop and remove the focus-history launch agent.
    Uninstall,
    /// Internal: the long-running focus-watch process launchd invokes.
    /// Not meant to be run directly.
    #[command(hide = true)]
    Run,
}

fn parse_stack_action(s: &str) -> Result<StackAction, String> {
    match s {
        "next" => Ok(StackAction::Next),
        "previous" | "prev" => Ok(StackAction::Previous),
        _ => Err(format!(
            "invalid stack action '{s}' (expected next or previous)"
        )),
    }
}

fn parse_third(s: &str) -> Result<Third, String> {
    match s {
        "left" => Ok(Third::Left),
        "center" => Ok(Third::Center),
        "right" => Ok(Third::Right),
        _ => Err(format!(
            "invalid third '{s}' (expected left, center, or right)"
        )),
    }
}

fn parse_display_target(s: &str) -> Result<DisplayTarget, String> {
    match s {
        "next" => Ok(DisplayTarget::Next),
        "previous" => Ok(DisplayTarget::Previous),
        _ => s.parse::<u32>().map(DisplayTarget::Index).map_err(|_| {
            format!("invalid display target '{s}' (expected next, previous, or a display number)")
        }),
    }
}

impl Command {
    pub fn as_position_and_size(&self) -> Option<(Position, Option<u32>)> {
        match self {
            Command::Left { size } => Some((Position::Left, *size)),
            Command::Right { size } => Some((Position::Right, *size)),
            Command::Top { size } => Some((Position::Top, *size)),
            Command::Bottom { size } => Some((Position::Bottom, *size)),
            Command::TopLeft { size } => Some((Position::TopLeft, *size)),
            Command::TopRight { size } => Some((Position::TopRight, *size)),
            Command::BottomLeft { size } => Some((Position::BottomLeft, *size)),
            Command::BottomRight { size } => Some((Position::BottomRight, *size)),
            _ => None,
        }
    }
}
