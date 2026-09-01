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
    /// Omit entirely to cycle through 25/50/75%.
    #[arg(value_name = "SIZE")]
    pub size: Option<u32>,
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
    pub fn as_position_and_size(&self) -> Option<(Position, Option<u32>)> {
        match self {
            Command::Left { size } => Some((Position::Left, *size)),
            Command::Right { size } => Some((Position::Right, *size)),
            Command::Top { size } => Some((Position::Top, *size)),
            Command::Bottom { size } => Some((Position::Bottom, *size)),
            _ => None,
        }
    }
}
