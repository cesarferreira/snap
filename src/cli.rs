use clap::{Parser, Subcommand};

use crate::layout::Position;

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
    #[arg(value_name = "SIZE")]
    pub size: Option<u32>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Anchor the focused window to the left, sized to <SIZE>% of the screen.
    Left { size: u32 },
    /// Anchor the focused window to the right, sized to <SIZE>% of the screen.
    Right { size: u32 },
    /// Anchor the focused window to the top, sized to <SIZE>% of the screen.
    Top { size: u32 },
    /// Anchor the focused window to the bottom, sized to <SIZE>% of the screen.
    Bottom { size: u32 },
    /// Fill the usable bounds of the current display.
    Full,
    /// Center the focused window without changing its size.
    Center,
    /// Tile visible windows on the current display.
    Tile {
        /// Gap between tiles, in logical points. Defaults to the configured
        /// padding (see `~/.config/snap.toml`).
        #[arg(long, allow_hyphen_values = true)]
        gap: Option<f64>,
    },
}

impl Command {
    pub fn as_position_and_size(&self) -> Option<(Position, u32)> {
        match self {
            Command::Left { size } => Some((Position::Left, *size)),
            Command::Right { size } => Some((Position::Right, *size)),
            Command::Top { size } => Some((Position::Top, *size)),
            Command::Bottom { size } => Some((Position::Bottom, *size)),
            _ => None,
        }
    }
}
