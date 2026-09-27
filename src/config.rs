//! Optional user config. Snap requires no configuration for normal usage
//! (PRD §2); this only overrides the built-in defaults (padding, the Stage
//! Manager reserved width) when `~/.config/snap.toml` exists.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedLayout {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

pub type Layouts = Vec<NamedLayout>;

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
    /// Peek strip width/height, in logical points, `snap stack` gives
    /// background windows so they show at the accordion's edges. `0`
    /// disables the peek (front-only, still raises on `next`/`previous`).
    pub accordion_padding: f64,
    /// Whether window geometry changes should animate.
    pub animations: bool,
    /// Window transition duration in milliseconds when animations are enabled.
    pub animation_duration: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            padding: 16.0,
            stage_manager_width: 150.0,
            almost_padding: 48.0,
            accordion_padding: 30.0,
            animations: true,
            animation_duration: 180,
        }
    }
}

pub fn load_all() -> (Config, Layouts) {
    let Some(path) = config_path() else {
        return (Config::default(), Vec::new());
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return (Config::default(), Vec::new());
    };
    parse_all(&contents)
}

pub fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("snap.toml"))
}

/// Parses `key = value` lines (`#` comments, blank lines ignored). Deliberately
/// not a full TOML parser — a couple of scalar fields don't warrant pulling in
/// `toml` + `serde` (PRD §22 avoids unnecessary dependencies).
#[cfg(test)]
fn parse(contents: &str) -> Config {
    parse_all(contents).0
}

pub fn valid_layout_name(name: &str) -> bool {
    !name.is_empty()
        && name != "capture"
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_all(contents: &str) -> (Config, Layouts) {
    let mut config = Config::default();
    let mut layouts: Layouts = Vec::new();
    let mut section: Option<usize> = None;
    let mut top_level = true;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = None;
            top_level = false;
            if let Some(name) = line[1..line.len() - 1].strip_prefix("layouts.") {
                if valid_layout_name(name) {
                    section = Some(
                        match layouts.iter().position(|layout| layout.name == name) {
                            Some(index) => index,
                            None => {
                                layouts.push(NamedLayout {
                                    name: name.to_string(),
                                    entries: Vec::new(),
                                });
                                layouts.len() - 1
                            }
                        },
                    );
                }
            }
            continue;
        }
        let Some((key, value)) = split_assignment(line) else {
            continue;
        };
        let value = value.trim();
        if let Some(index) = section {
            let key = key.trim();
            let key = if key.starts_with('"') && key.ends_with('"') && key.len() >= 2 {
                key[1..key.len() - 1]
                    .replace("\\\"", "\"")
                    .replace("\\\\", "\\")
            } else if key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                key.to_string()
            } else {
                continue;
            };
            if key.is_empty() || !value.starts_with('"') || !value.ends_with('"') || value.len() < 2
            {
                continue;
            }
            let value = value[1..value.len() - 1].to_string();
            let entries = &mut layouts[index].entries;
            if let Some(existing) = entries.iter_mut().find(|(app, _)| app == &key) {
                existing.1 = value;
            } else {
                entries.push((key, value));
            }
            continue;
        }
        if !top_level {
            continue;
        }
        let value = value.trim_matches('"');
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
            "accordion_padding" => {
                if let Ok(padding) = value.parse::<f64>() {
                    config.accordion_padding = padding;
                }
            }
            "animations" => {
                if let Ok(animations) = value.parse::<bool>() {
                    config.animations = animations;
                }
            }
            "animation_duration" => {
                if let Some(duration) = value.parse::<u64>().ok().filter(|duration| *duration > 0) {
                    config.animation_duration = duration;
                }
            }
            _ => {}
        }
    }
    (config, layouts)
}

fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
        }
        if ch == '=' && !quoted {
            return Some((&line[..index], &line[index + 1..]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ordered_layouts_and_scopes_scalar_keys() {
        let (config, layouts) = parse_all(
            "padding = 20\n[layouts.code]\nGhostty = \"left 60\"\n\"Google Chrome\" = \"right 40\"\npadding = 5\n[other]\npadding = 3\n[layouts.code]\nGhostty = \"full\"",
        );
        assert_eq!(config.padding, 20.0);
        assert_eq!(
            layouts[0].entries,
            vec![
                ("Ghostty".into(), "full".into()),
                ("Google Chrome".into(), "right 40".into())
            ]
        );
    }

    #[test]
    fn ignores_other_sections_and_malformed_layout_lines() {
        let (config, layouts) = parse_all(
            "[future]\npadding = 3\n[layouts.work]\n# comment\n\nmalformed\nGhostty = left 60\n\"Google Chrome\" = \"right 40\"\n",
        );
        assert_eq!(config.padding, Config::default().padding);
        assert_eq!(
            layouts[0].entries,
            vec![("Google Chrome".into(), "right 40".into())]
        );
    }

    #[test]
    fn layout_keys_can_contain_equals_and_escaped_quotes() {
        let (_, layouts) = parse_all("[layouts.work]\n\"A=B \\\"C\\\"\" = \"full\"\n");
        assert_eq!(layouts[0].entries[0].0, "A=B \"C\"");
    }

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

    #[test]
    fn overrides_accordion_padding() {
        assert_eq!(parse("accordion_padding = 40").accordion_padding, 40.0);
    }

    #[test]
    fn animation_defaults_to_a_short_transition() {
        assert_eq!(parse("").animation_duration, 180);
    }

    #[test]
    fn animations_are_enabled_by_default() {
        assert!(parse("").animations);
    }

    #[test]
    fn animations_can_be_disabled_explicitly() {
        assert!(!parse("animations = false").animations);
    }

    #[test]
    fn invalid_animations_value_keeps_default() {
        assert!(parse("animations = sometimes").animations);
    }

    #[test]
    fn zero_animation_duration_keeps_default() {
        assert_eq!(
            parse("animation_duration = 0").animation_duration,
            Config::default().animation_duration
        );
    }

    #[test]
    fn invalid_animation_duration_keeps_default() {
        assert_eq!(
            parse("animation_duration = -1").animation_duration,
            Config::default().animation_duration
        );
    }
}
