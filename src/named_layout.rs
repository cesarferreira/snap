//! Deterministic placement grammar and inverse mapping for named layouts.

use crate::layout::{self, Position, Rect, Third};

/// Parses a numbered key when the layout value explicitly starts with
/// `window N`. Without that value prefix, the whole key is a literal app name.
pub fn parse_window_key(key: &str) -> Option<(&str, Option<usize>)> {
    if let Some((app, suffix)) = key.rsplit_once('[') {
        if let Some(index) = suffix.strip_suffix(']') {
            let index = index.parse::<usize>().ok().filter(|index| *index > 0)?;
            return (!app.is_empty()).then_some((app, Some(index)));
        }
    }
    (!key.is_empty()).then_some((key, None))
}

pub fn ordered_window_ids(ids: &[i64]) -> Vec<i64> {
    let mut ordered = ids.to_vec();
    ordered.sort_unstable();
    ordered.dedup();
    ordered
}

pub fn window_id_for_slot(ids: &[i64], index: usize) -> Option<i64> {
    ordered_window_ids(ids).get(index.checked_sub(1)?).copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Sized(u32),
    Directional(Position, u32),
    Third(Third),
    Full,
    Almost,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub placement: Placement,
    pub display: Option<usize>, // 1-based
}

impl Spec {
    pub fn rect(self, usable: Rect, current: Rect, almost_padding: f64) -> Rect {
        match self.placement {
            Placement::Sized(p) => layout::sized_rect(usable, p),
            Placement::Directional(pos, p) => layout::directional_rect(usable, pos, p),
            Placement::Third(third) => layout::third_rect(usable, third),
            Placement::Full => layout::full_rect(usable),
            Placement::Almost => layout::almost_rect(usable, almost_padding),
            Placement::Center => layout::center_rect(usable, current.width, current.height),
        }
    }
}

pub fn parse(raw: &str) -> Option<Spec> {
    let mut words: Vec<&str> = raw.split_whitespace().collect();
    let display = if words.len() >= 2 && words[words.len() - 2] == "on" {
        let n = words.pop()?.parse::<usize>().ok().filter(|n| *n > 0)?;
        words.pop();
        Some(n)
    } else {
        None
    };
    let placement = match words.as_slice() {
        ["full"] => Placement::Full,
        ["almost"] => Placement::Almost,
        ["center"] => Placement::Center,
        ["third", "left"] => Placement::Third(Third::Left),
        ["third", "center"] => Placement::Third(Third::Center),
        ["third", "right"] => Placement::Third(Third::Right),
        [size] => Placement::Sized(
            size.parse()
                .ok()
                .filter(|p| layout::is_supported_percent(*p))?,
        ),
        [side, size] => {
            let position = match *side {
                "left" => Position::Left,
                "right" => Position::Right,
                "top" => Position::Top,
                "bottom" => Position::Bottom,
                "top-left" => Position::TopLeft,
                "top-right" => Position::TopRight,
                "bottom-left" => Position::BottomLeft,
                "bottom-right" => Position::BottomRight,
                _ => return None,
            };
            Placement::Directional(
                position,
                size.parse()
                    .ok()
                    .filter(|p| layout::is_supported_percent(*p))?,
            )
        }
        _ => return None,
    };
    Some(Spec { placement, display })
}

fn matches(a: Rect, b: Rect) -> bool {
    const EPS: f64 = 20.0;
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.width - b.width).abs() < EPS
        && (a.height - b.height).abs() < EPS
}

fn distance(a: Rect, b: Rect) -> f64 {
    (a.x - b.x).abs() + (a.y - b.y).abs() + (a.width - b.width).abs() + (a.height - b.height).abs()
}

/// Returns the first canonical deterministic placement matching the frame.
pub fn capture(usable: Rect, window: Rect, almost_padding: f64) -> Option<String> {
    if matches(layout::full_rect(usable), window) {
        return Some("full".into());
    }
    if matches(layout::almost_rect(usable, almost_padding), window) {
        return Some("almost".into());
    }
    for (name, third) in [
        ("left", Third::Left),
        ("center", Third::Center),
        ("right", Third::Right),
    ] {
        if matches(layout::third_rect(usable, third), window) {
            return Some(format!("third {name}"));
        }
    }
    for (name, position) in [
        ("left", Position::Left),
        ("right", Position::Right),
        ("top", Position::Top),
        ("bottom", Position::Bottom),
        ("top-left", Position::TopLeft),
        ("top-right", Position::TopRight),
        ("bottom-left", Position::BottomLeft),
        ("bottom-right", Position::BottomRight),
    ] {
        let percent = (1..=100)
            .min_by(|&a, &b| {
                distance(layout::directional_rect(usable, position, a), window).total_cmp(
                    &distance(layout::directional_rect(usable, position, b), window),
                )
            })
            .unwrap();
        if matches(layout::directional_rect(usable, position, percent), window) {
            return Some(format!("{name} {percent}"));
        }
    }
    let percent = (1..=100)
        .min_by(|&a, &b| {
            distance(layout::sized_rect(usable, a), window)
                .total_cmp(&distance(layout::sized_rect(usable, b), window))
        })
        .unwrap();
    if matches(layout::sized_rect(usable, percent), window) {
        return Some(percent.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_selectors_use_stable_id_order() {
        assert_eq!(parse_window_key("Ghostty"), Some(("Ghostty", None)));
        assert_eq!(parse_window_key("Ghostty[2]"), Some(("Ghostty", Some(2))));
        assert_eq!(parse_window_key("Ghostty[0]"), None);
        assert_eq!(parse_window_key("Ghostty[nope]"), None);
        assert_eq!(ordered_window_ids(&[30, 10, 20]), vec![10, 20, 30]);
        assert_eq!(window_id_for_slot(&[30, 10, 20], 2), Some(20));
        assert_eq!(window_id_for_slot(&[20, 30], 3), None);
    }

    #[test]
    fn accepts_only_deterministic_placements() {
        for value in [
            "70",
            "left 60",
            "top-right 33",
            "third center",
            "full on 2",
            "almost",
            "center",
        ] {
            assert!(parse(value).is_some(), "{value}");
        }
        for value in [
            "",
            "left",
            "third",
            "tile",
            "grow",
            "left 0",
            "left 101",
            "full on 0",
            "full on x",
            "full --app X",
            "full on 2 more",
        ] {
            assert!(parse(value).is_none(), "{value}");
        }
    }

    #[test]
    fn captures_arbitrary_percent_and_priority() {
        let usable = Rect::new(16.0, 40.0, 1200.0, 900.0);
        assert_eq!(
            capture(
                usable,
                layout::directional_rect(usable, Position::Left, 60),
                48.0
            ),
            Some("left 60".into())
        );
        assert_eq!(
            capture(
                usable,
                layout::directional_rect(usable, Position::TopRight, 33),
                48.0
            ),
            Some("top-right 33".into())
        );
        assert_eq!(
            capture(usable, layout::sized_rect(usable, 70), 48.0),
            Some("70".into())
        );
        assert_eq!(capture(usable, usable, 48.0), Some("full".into()));
    }

    #[test]
    fn captured_specs_reproduce_frames() {
        let usable = Rect::new(10.0, 35.0, 1200.0, 900.0);
        for raw in [
            "left 60",
            "right 40",
            "top 37",
            "bottom 25",
            "top-left 33",
            "top-right 33",
            "bottom-left 75",
            "bottom-right 65",
            "third center",
            "full",
            "almost",
            "70",
        ] {
            let spec = parse(raw).unwrap();
            let original = spec.rect(usable, usable, 48.0);
            let captured = capture(usable, original, 48.0).unwrap();
            let roundtrip = parse(&captured).unwrap().rect(usable, usable, 48.0);
            assert!(matches(original, roundtrip), "{raw} -> {captured}");
        }
    }

    #[test]
    fn capture_tolerates_small_frame_adjustments_and_rejects_unmatched_frames() {
        let usable = Rect::new(16.0, 40.0, 1200.0, 900.0);
        let mut adjusted = layout::directional_rect(usable, Position::Left, 60);
        adjusted.x += 3.0;
        adjusted.width -= 5.0;
        assert_eq!(capture(usable, adjusted, 48.0), Some("left 60".into()));
        assert_eq!(
            capture(usable, Rect::new(200.0, 150.0, 350.0, 300.0), 48.0),
            None
        );
    }
}
