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

/// Sizes cycled through when a size is omitted (e.g. `snap left` with no
/// percent) — repeated invocations step 50% → 75% → 25% → 50% → ..., like
/// Rectangle's cycling. Deliberately excludes 100/full, which stays an
/// explicit-only command.
pub const CYCLE_PERCENTS: [u32; 3] = [25, 50, 75];

/// The step to enter the cycle at when the window doesn't currently match
/// any step (matches Rectangle: the first press snaps to half, not quarter).
const CYCLE_ENTRY_PERCENT: u32 = 50;

/// The percent to apply next in the cycle. `current` is the step the window
/// is presently at (from [`detect_directional_percent`] /
/// [`detect_centered_percent`]), or `None` if it doesn't match any step —
/// in which case the cycle (re)starts at [`CYCLE_ENTRY_PERCENT`]. This makes
/// cycling stateless: each invocation derives "where we are" from the
/// window's live geometry rather than remembering prior invocations.
pub fn next_cycle_percent(current: Option<u32>) -> u32 {
    match current.and_then(|p| CYCLE_PERCENTS.iter().position(|&step| step == p)) {
        Some(i) => CYCLE_PERCENTS[(i + 1) % CYCLE_PERCENTS.len()],
        None => CYCLE_ENTRY_PERCENT,
    }
}

/// Generous on purpose: apps that snap their size to a grid (terminal
/// emulators snapping to character cells, editors snapping to a column
/// width) commonly land a few points off an exact request (PRD §23). Cycle
/// steps are hundreds of points apart, so this can't be confused for a
/// neighboring step.
fn rects_match(a: Rect, b: Rect) -> bool {
    const EPS: f64 = 20.0;
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.width - b.width).abs() < EPS
        && (a.height - b.height).abs() < EPS
}

/// Which cycle step `window` currently matches for `position`, if any.
pub fn detect_directional_percent(usable: Rect, position: Position, window: Rect) -> Option<u32> {
    CYCLE_PERCENTS
        .into_iter()
        .find(|&p| rects_match(directional_rect(usable, position, p), window))
}

/// Which cycle step `window` currently matches for the centered layout, if any.
pub fn detect_centered_percent(usable: Rect, window: Rect) -> Option<u32> {
    CYCLE_PERCENTS
        .into_iter()
        .find(|&p| rects_match(sized_rect(usable, p), window))
}

/// Shrinks `usable` by `padding` on every side. Applied uniformly before any
/// layout calculation so every command respects the configured screen-edge
/// padding, not just `tile`. Clamped so padding can never invert the rect.
pub fn padded(usable: Rect, padding: f64) -> Rect {
    if padding <= 0.0 {
        return usable;
    }
    let padding = padding.min(usable.width / 2.0).min(usable.height / 2.0);
    Rect::new(
        usable.x + padding,
        usable.y + padding,
        usable.width - padding * 2.0,
        usable.height - padding * 2.0,
    )
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

    #[test]
    fn padded_insets_all_sides() {
        let r = padded(SCREEN, 16.0);
        assert_eq!(r, Rect::new(16.0, 16.0, 1696.0, 1085.0));
    }

    #[test]
    fn padded_zero_is_noop() {
        assert_eq!(padded(SCREEN, 0.0), SCREEN);
    }

    #[test]
    fn padded_clamps_instead_of_inverting() {
        let tiny = Rect::new(0.0, 0.0, 20.0, 20.0);
        let r = padded(tiny, 1000.0);
        assert_eq!(r.width, 0.0);
        assert_eq!(r.height, 0.0);
    }

    #[test]
    fn cycle_advances_through_25_50_75_and_wraps() {
        assert_eq!(next_cycle_percent(Some(25)), 50);
        assert_eq!(next_cycle_percent(Some(50)), 75);
        assert_eq!(next_cycle_percent(Some(75)), 25);
    }

    #[test]
    fn cycle_starts_at_50_when_no_match() {
        assert_eq!(next_cycle_percent(None), 50);
        assert_eq!(next_cycle_percent(Some(33)), 50);
        assert_eq!(next_cycle_percent(Some(100)), 50);
    }

    #[test]
    fn detect_directional_percent_matches_current_step() {
        let window = directional_rect(SCREEN, Position::Left, 50);
        assert_eq!(
            detect_directional_percent(SCREEN, Position::Left, window),
            Some(50)
        );
    }

    #[test]
    fn detect_directional_percent_none_when_unrelated_rect() {
        let window = Rect::new(200.0, 200.0, 300.0, 300.0);
        assert_eq!(
            detect_directional_percent(SCREEN, Position::Left, window),
            None
        );
    }

    #[test]
    fn detect_directional_percent_ignores_other_positions() {
        let left_50 = directional_rect(SCREEN, Position::Left, 50);
        assert_eq!(
            detect_directional_percent(SCREEN, Position::Right, left_50),
            None
        );
    }

    #[test]
    fn detect_centered_percent_matches_current_step() {
        let window = sized_rect(SCREEN, 75);
        assert_eq!(detect_centered_percent(SCREEN, window), Some(75));
    }

    #[test]
    fn full_directional_cycle_reaches_every_step_in_order() {
        let mut window = Rect::new(999.0, 999.0, 999.0, 999.0); // no match, starts fresh
        let mut steps = Vec::new();
        for _ in 0..6 {
            let current = detect_directional_percent(SCREEN, Position::Left, window);
            let next = next_cycle_percent(current);
            steps.push(next);
            window = directional_rect(SCREEN, Position::Left, next);
        }
        assert_eq!(steps, vec![50, 75, 25, 50, 75, 25]);
    }
}
