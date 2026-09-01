//! Pure geometry calculations. No macOS Accessibility/display calls here —
//! everything is unit-testable given a screen rect and a command.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Left,
    Right,
    Top,
    Bottom,
}

/// v1 only supports these four percentages (PRD §6).
pub const SUPPORTED_PERCENTS: [u32; 4] = [25, 50, 75, 100];

pub fn is_supported_percent(percent: u32) -> bool {
    SUPPORTED_PERCENTS.contains(&percent)
}

fn fraction(percent: u32) -> f64 {
    percent as f64 / 100.0
}

/// `snap <percent>` — centered window scaled on both axes (PRD §8).
pub fn sized_rect(usable: Rect, percent: u32) -> Rect {
    if percent == 100 {
        return usable;
    }
    let f = fraction(percent);
    let width = usable.width * f;
    let height = usable.height * f;
    center_rect(usable, width, height)
}

/// `snap left|right|top|bottom <percent>` (PRD §9).
pub fn directional_rect(usable: Rect, position: Position, percent: u32) -> Rect {
    let f = fraction(percent);
    match position {
        Position::Left => Rect::new(usable.x, usable.y, usable.width * f, usable.height),
        Position::Right => {
            let width = usable.width * f;
            Rect::new(
                usable.x + usable.width - width,
                usable.y,
                width,
                usable.height,
            )
        }
        Position::Top => Rect::new(usable.x, usable.y, usable.width, usable.height * f),
        Position::Bottom => {
            let height = usable.height * f;
            Rect::new(
                usable.x,
                usable.y + usable.height - height,
                usable.width,
                height,
            )
        }
    }
}

/// `snap full` / `snap 100` (PRD §10) — fills the usable display bounds.
pub fn full_rect(usable: Rect) -> Rect {
    usable
}

/// `snap center` (PRD §11) — keeps size, centers position, clamps into bounds.
pub fn center_rect(usable: Rect, width: f64, height: f64) -> Rect {
    let width = width.min(usable.width);
    let height = height.min(usable.height);
    let x = usable.x + (usable.width - width) / 2.0;
    let y = usable.y + (usable.height - height) / 2.0;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1728.0,
        height: 1117.0,
    };

    #[test]
    fn left_50() {
        let r = directional_rect(SCREEN, Position::Left, 50);
        assert_eq!(r, Rect::new(0.0, 0.0, 864.0, 1117.0));
    }

    #[test]
    fn left_25() {
        let r = directional_rect(SCREEN, Position::Left, 25);
        assert_eq!(r, Rect::new(0.0, 0.0, 432.0, 1117.0));
    }

    #[test]
    fn left_75() {
        let r = directional_rect(SCREEN, Position::Left, 75);
        assert_eq!(r, Rect::new(0.0, 0.0, 1296.0, 1117.0));
    }

    #[test]
    fn right_50() {
        let r = directional_rect(SCREEN, Position::Right, 50);
        assert_eq!(r, Rect::new(864.0, 0.0, 864.0, 1117.0));
    }

    #[test]
    fn right_25() {
        let r = directional_rect(SCREEN, Position::Right, 25);
        assert_eq!(r, Rect::new(1296.0, 0.0, 432.0, 1117.0));
    }

    #[test]
    fn right_75() {
        let r = directional_rect(SCREEN, Position::Right, 75);
        assert_eq!(r, Rect::new(432.0, 0.0, 1296.0, 1117.0));
    }

    #[test]
    fn top_50() {
        let r = directional_rect(SCREEN, Position::Top, 50);
        assert_eq!(r, Rect::new(0.0, 0.0, 1728.0, 558.5));
    }

    #[test]
    fn top_25() {
        let r = directional_rect(SCREEN, Position::Top, 25);
        assert_eq!(r, Rect::new(0.0, 0.0, 1728.0, 279.25));
    }

    #[test]
    fn top_75() {
        let r = directional_rect(SCREEN, Position::Top, 75);
        assert_eq!(r, Rect::new(0.0, 0.0, 1728.0, 837.75));
    }

    #[test]
    fn bottom_50() {
        let r = directional_rect(SCREEN, Position::Bottom, 50);
        assert_eq!(r, Rect::new(0.0, 558.5, 1728.0, 558.5));
    }

    #[test]
    fn bottom_25() {
        let r = directional_rect(SCREEN, Position::Bottom, 25);
        assert_eq!(r, Rect::new(0.0, 837.75, 1728.0, 279.25));
    }

    #[test]
    fn bottom_75() {
        let r = directional_rect(SCREEN, Position::Bottom, 75);
        assert_eq!(r, Rect::new(0.0, 279.25, 1728.0, 837.75));
    }

    #[test]
    fn centered_25() {
        let r = sized_rect(SCREEN, 25);
        assert_eq!(r.width, 432.0);
        assert_eq!(r.height, 279.25);
        assert_eq!(r.x, (1728.0 - 432.0) / 2.0);
        assert_eq!(r.y, (1117.0 - 279.25) / 2.0);
    }

    #[test]
    fn centered_50() {
        let r = sized_rect(SCREEN, 50);
        assert_eq!(r.width, 864.0);
        assert_eq!(r.height, 558.5);
    }

    #[test]
    fn centered_75() {
        let r = sized_rect(SCREEN, 75);
        assert_eq!(r.width, 1296.0);
        assert_eq!(r.height, 837.75);
    }

    #[test]
    fn full_fills_usable_bounds() {
        assert_eq!(full_rect(SCREEN), SCREEN);
        assert_eq!(sized_rect(SCREEN, 100), SCREEN);
    }

    #[test]
    fn center_keeps_size() {
        let r = center_rect(SCREEN, 800.0, 600.0);
        assert_eq!(r.width, 800.0);
        assert_eq!(r.height, 600.0);
        assert_eq!(r.x, (1728.0 - 800.0) / 2.0);
        assert_eq!(r.y, (1117.0 - 600.0) / 2.0);
    }

    #[test]
    fn center_clamps_oversized_window() {
        let r = center_rect(SCREEN, 2000.0, 2000.0);
        assert_eq!(r.width, SCREEN.width);
        assert_eq!(r.height, SCREEN.height);
        assert_eq!(r.x, SCREEN.x);
        assert_eq!(r.y, SCREEN.y);
    }

    #[test]
    fn works_on_a_different_screen_size() {
        let screen = Rect::new(0.0, 0.0, 3440.0, 1440.0);
        let r = directional_rect(screen, Position::Left, 50);
        assert_eq!(r, Rect::new(0.0, 0.0, 1720.0, 1440.0));
    }

    #[test]
    fn supported_percents_are_exactly_v1_set() {
        for p in [25, 50, 75, 100] {
            assert!(is_supported_percent(p));
        }
        assert!(!is_supported_percent(42));
        assert!(!is_supported_percent(33));
    }
}
