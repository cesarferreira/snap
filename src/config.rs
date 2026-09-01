//! Optional user config. Snap requires no configuration for normal usage
//! (PRD §2); this only overrides the built-in defaults (padding, the Stage
//! Manager reserved width) when `~/.config/snap.toml` exists.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// Screen-edge and inter-tile padding, in logical points, applied to
    /// every command unless overridden (e.g. `snap tile --gap`).
    pub padding: f64,
    /// Width, in logical points, reserved on the left edge of every display
    /// when Stage Manager is enabled, so windows don't cover its strip.
    /// `0` disables the reservation even if Stage Manager is on.
    pub stage_manager_width: f64,
    /// Extra inset, in logical points, `snap almost` applies beyond `padding`
    /// so the desktop stays visible around the edges.
    pub almost_padding: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            padding: 16.0,
            stage_manager_width: 150.0,
            almost_padding: 48.0,
        }
    }
}

pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    parse(&contents)
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("snap.toml"))
}

/// Parses `key = value` lines (`#` comments, blank lines ignored). Deliberately
/// not a full TOML parser — a couple of scalar fields don't warrant pulling in
/// `toml` + `serde` (PRD §22 avoids unnecessary dependencies).
fn parse(contents: &str) -> Config {
    let mut config = Config::default();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "padding" => {
                if let Ok(padding) = value.parse::<f64>() {
                    config.padding = padding;
                }
            }
            "stage_manager_width" => {
                if let Ok(width) = value.parse::<f64>() {
                    config.stage_manager_width = width;
                }
            }
            "almost_padding" => {
                if let Ok(padding) = value.parse::<f64>() {
                    config.almost_padding = padding;
                }
            }
            _ => {}
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_uses_default() {
        assert_eq!(parse(""), Config::default());
    }

    #[test]
    fn overrides_padding() {
        assert_eq!(parse("padding = 24").padding, 24.0);
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let config = parse("# comment\n\npadding = 8\n");
        assert_eq!(config.padding, 8.0);
    }

    #[test]
    fn ignores_unknown_keys() {
        assert_eq!(parse("foo = 1\npadding = 4").padding, 4.0);
    }

    #[test]
    fn ignores_unparseable_value_and_keeps_default() {
        assert_eq!(
            parse("padding = not-a-number").padding,
            Config::default().padding
        );
    }

    #[test]
    fn tolerates_quoted_values() {
        assert_eq!(parse(r#"padding = "12""#).padding, 12.0);
    }

    #[test]
    fn overrides_stage_manager_width() {
        assert_eq!(
            parse("stage_manager_width = 200").stage_manager_width,
            200.0
        );
    }

    #[test]
    fn overrides_almost_padding() {
        assert_eq!(parse("almost_padding = 64").almost_padding, 64.0);
    }
}
