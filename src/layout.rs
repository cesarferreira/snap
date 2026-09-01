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
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Any integer percent in this (inclusive) range is a valid explicit size
/// for centered/directional placement. `0` is meaningless (a zero-size
/// window) and `>100` would overflow the usable area.
pub const MIN_PERCENT: u32 = 1;
pub const MAX_PERCENT: u32 = 100;

pub fn is_supported_percent(percent: u32) -> bool {
    (MIN_PERCENT..=MAX_PERCENT).contains(&percent)
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

/// `snap left|right|top|bottom <percent>` (PRD §9) and the four corner
/// anchors: a corner occupies `percent%` of usable **width and height**,
/// anchored to that corner, rather than a full-height/width strip.
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
        Position::TopLeft => Rect::new(usable.x, usable.y, usable.width * f, usable.height * f),
        Position::TopRight => {
            let width = usable.width * f;
            Rect::new(
                usable.x + usable.width - width,
                usable.y,
                width,
                usable.height * f,
            )
        }
        Position::BottomLeft => {
            let height = usable.height * f;
            Rect::new(
                usable.x,
                usable.y + usable.height - height,
                usable.width * f,
                height,
            )
        }
        Position::BottomRight => {
            let width = usable.width * f;
            let height = usable.height * f;
            Rect::new(
                usable.x + usable.width - width,
                usable.y + usable.height - height,
                width,
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

/// `snap third [left|center|right]` — ultrawide-friendly full-height thirds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Third {
    Left,
    Center,
    Right,
}

const THIRD_CYCLE: [Third; 3] = [Third::Left, Third::Center, Third::Right];

/// Partitions `usable` width into three full-height columns with no overlap
/// and no gap: the first two columns are `floor(width / 3)`, the last one
/// absorbs the remainder so the set exactly covers `usable.width`.
pub fn third_rect(usable: Rect, third: Third) -> Rect {
    let col_width = (usable.width / 3.0).floor();
    match third {
        Third::Left => Rect::new(usable.x, usable.y, col_width, usable.height),
        Third::Center => Rect::new(usable.x + col_width, usable.y, col_width, usable.height),
        Third::Right => {
            let x = usable.x + col_width * 2.0;
            Rect::new(x, usable.y, usable.x + usable.width - x, usable.height)
        }
    }
}

/// Which third `window` currently matches, if any (same tolerance as the
/// side/corner cycle detection).
pub fn detect_third(usable: Rect, window: Rect) -> Option<Third> {
    THIRD_CYCLE
        .into_iter()
        .find(|&t| rects_match(third_rect(usable, t), window))
}

/// `snap third` with no explicit position: left → center → right → left,
/// stateless — restarts at `left` if the window isn't currently on a third.
pub fn next_third(current: Option<Third>) -> Third {
    match current.and_then(|t| THIRD_CYCLE.iter().position(|&step| step == t)) {
        Some(i) => THIRD_CYCLE[(i + 1) % THIRD_CYCLE.len()],
        None => Third::Left,
    }
}

/// Fraction of usable width/height added or removed per `snap grow`/`shrink`
/// invocation, so repeated presses reach `usable` bounds in a predictable
/// number of hits.
const RESIZE_STEP_FRACTION: f64 = 0.10;

/// Neither `grow` nor `shrink` will make a window smaller than this fraction
/// of usable width/height.
const MIN_SIZE_FRACTION: f64 = 0.10;

/// `snap grow` — scales `window` up toward `usable` bounds, keeping any edge
/// already flush with a usable edge (within tolerance) fixed in place;
/// otherwise scales about the window's current center.
pub fn grow_rect(usable: Rect, window: Rect) -> Rect {
    nudge_rect(usable, window, RESIZE_STEP_FRACTION)
}

/// `snap shrink` — the inverse of [`grow_rect`], never smaller than
/// [`MIN_SIZE_FRACTION`] of usable width/height.
pub fn shrink_rect(usable: Rect, window: Rect) -> Rect {
    nudge_rect(usable, window, -RESIZE_STEP_FRACTION)
}

fn nudge_rect(usable: Rect, window: Rect, delta_fraction: f64) -> Rect {
    const EDGE_EPS: f64 = 2.0;
    let min_width = usable.width * MIN_SIZE_FRACTION;
    let min_height = usable.height * MIN_SIZE_FRACTION;

    let new_width = (window.width + usable.width * delta_fraction).clamp(min_width, usable.width);
    let new_height =
        (window.height + usable.height * delta_fraction).clamp(min_height, usable.height);

    let left_flush = (window.x - usable.x).abs() < EDGE_EPS;
    let right_flush = ((window.x + window.width) - (usable.x + usable.width)).abs() < EDGE_EPS;
    let top_flush = (window.y - usable.y).abs() < EDGE_EPS;
    let bottom_flush = ((window.y + window.height) - (usable.y + usable.height)).abs() < EDGE_EPS;

    let x = if left_flush && !right_flush {
        window.x
    } else if right_flush && !left_flush {
        window.x + window.width - new_width
    } else {
        window.x + (window.width - new_width) / 2.0
    };
    let y = if top_flush && !bottom_flush {
        window.y
    } else if bottom_flush && !top_flush {
        window.y + window.height - new_height
    } else {
        window.y + (window.height - new_height) / 2.0
    };

    let x = x.clamp(usable.x, usable.x + usable.width - new_width);
    let y = y.clamp(usable.y, usable.y + usable.height - new_height);
    Rect::new(x, y, new_width, new_height)
}

/// `snap almost` — like [`full_rect`] but inset further by `almost_padding`
/// beyond the already-padded `usable` bounds, so the desktop edges stay
/// visible. Never native fullscreen.
pub fn almost_rect(usable: Rect, almost_padding: f64) -> Rect {
    padded(usable, almost_padding)
}

/// Target for `snap display next|previous|N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTarget {
    Next,
    Previous,
    /// 1-based index, as typed by the user.
    Index(u32),
}

/// Resolves `target` to a 0-based index into a `count`-long, already-ordered
/// display list, given the 0-based index of the display the window is
/// currently on. Returns `None` for an out-of-range explicit index or an
/// empty display list.
pub fn resolve_display_index(current: usize, count: usize, target: DisplayTarget) -> Option<usize> {
    if count == 0 {
        return None;
    }
    match target {
        DisplayTarget::Next => Some((current + 1) % count),
        DisplayTarget::Previous => Some((current + count - 1) % count),
        DisplayTarget::Index(n) => {
            if n >= 1 && (n as usize) <= count {
                Some(n as usize - 1)
            } else {
                None
            }
        }
    }
}

/// Maps `window`'s frame from `from_usable` to `to_usable`, preserving its
/// relative position and size (PRD-style proportional move across
/// displays), then clamps so the result stays fully inside `to_usable` even
/// when the destination is smaller than the window.
pub fn map_rect_between_displays(window: Rect, from_usable: Rect, to_usable: Rect) -> Rect {
    let rel_x = if from_usable.width > 0.0 {
        (window.x - from_usable.x) / from_usable.width
    } else {
        0.0
    };
    let rel_y = if from_usable.height > 0.0 {
        (window.y - from_usable.y) / from_usable.height
    } else {
        0.0
    };
    let rel_w = if from_usable.width > 0.0 {
        window.width / from_usable.width
    } else {
        1.0
    };
    let rel_h = if from_usable.height > 0.0 {
        window.height / from_usable.height
    } else {
        1.0
    };

    let width = (rel_w * to_usable.width).min(to_usable.width);
    let height = (rel_h * to_usable.height).min(to_usable.height);
    let x = (to_usable.x + rel_x * to_usable.width)
        .clamp(to_usable.x, to_usable.x + to_usable.width - width);
    let y = (to_usable.y + rel_y * to_usable.height)
        .clamp(to_usable.y, to_usable.y + to_usable.height - height);
    Rect::new(x, y, width, height)
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
    fn top_left_occupies_that_corner() {
        for percent in [25, 50, 75] {
            let r = directional_rect(SCREEN, Position::TopLeft, percent);
            let f = percent as f64 / 100.0;
            assert_eq!(r, Rect::new(0.0, 0.0, 1728.0 * f, 1117.0 * f));
        }
    }

    #[test]
    fn top_right_occupies_that_corner() {
        let r = directional_rect(SCREEN, Position::TopRight, 50);
        assert_eq!(r, Rect::new(864.0, 0.0, 864.0, 558.5));
    }

    #[test]
    fn bottom_left_occupies_that_corner() {
        let r = directional_rect(SCREEN, Position::BottomLeft, 50);
        assert_eq!(r, Rect::new(0.0, 558.5, 864.0, 558.5));
    }

    #[test]
    fn bottom_right_occupies_that_corner() {
        let r = directional_rect(SCREEN, Position::BottomRight, 50);
        assert_eq!(r, Rect::new(864.0, 558.5, 864.0, 558.5));
    }

    #[test]
    fn corner_at_100_percent_equals_full() {
        for corner in [
            Position::TopLeft,
            Position::TopRight,
            Position::BottomLeft,
            Position::BottomRight,
        ] {
            assert_eq!(directional_rect(SCREEN, corner, 100), full_rect(SCREEN));
        }
    }

    #[test]
    fn corners_respect_padding_and_non_origin_display() {
        let display = Rect::new(1728.0, 100.0, 1920.0, 1080.0);
        let usable = padded(display, 16.0);
        let r = directional_rect(usable, Position::TopLeft, 50);
        assert_eq!(r.x, usable.x);
        assert_eq!(r.y, usable.y);
        assert_eq!(r.width, usable.width * 0.5);
        assert_eq!(r.height, usable.height * 0.5);
    }

    #[test]
    fn corner_cycle_is_independent_and_stateless_per_corner() {
        let top_left_50 = directional_rect(SCREEN, Position::TopLeft, 50);
        // A top-left-50% window is not mistaken for a left-50% strip.
        assert_eq!(
            detect_directional_percent(SCREEN, Position::Left, top_left_50),
            None
        );
        assert_eq!(
            detect_directional_percent(SCREEN, Position::TopLeft, top_left_50),
            Some(50)
        );

        let current = detect_directional_percent(SCREEN, Position::TopLeft, top_left_50);
        assert_eq!(next_cycle_percent(current), 75);
    }

    #[test]
    fn grow_scales_up_about_center_when_not_flush() {
        let window = Rect::new(500.0, 400.0, 400.0, 300.0); // centered-ish, not flush
        let r = grow_rect(SCREEN, window);
        assert_eq!(r.width, 400.0 + SCREEN.width * 0.10);
        assert_eq!(r.height, 300.0 + SCREEN.height * 0.10);
        // Center stays the same.
        assert!((r.x + r.width / 2.0 - (window.x + window.width / 2.0)).abs() < 1e-6);
        assert!((r.y + r.height / 2.0 - (window.y + window.height / 2.0)).abs() < 1e-6);
    }

    #[test]
    fn grow_keeps_left_edge_flush_after_a_left_snap() {
        let window = directional_rect(SCREEN, Position::Left, 50);
        let r = grow_rect(SCREEN, window);
        assert_eq!(r.x, SCREEN.x);
        assert!(r.width > window.width);
    }

    #[test]
    fn grow_keeps_right_edge_flush_after_a_right_snap() {
        let window = directional_rect(SCREEN, Position::Right, 50);
        let r = grow_rect(SCREEN, window);
        assert_eq!(r.x + r.width, SCREEN.x + SCREEN.width);
    }

    #[test]
    fn grow_clamps_to_usable_bounds() {
        let window = Rect::new(0.0, 0.0, SCREEN.width * 0.98, SCREEN.height * 0.98);
        let r = grow_rect(SCREEN, window);
        assert!(r.width <= SCREEN.width);
        assert!(r.height <= SCREEN.height);
    }

    #[test]
    fn shrink_scales_down_and_stops_at_minimum() {
        let window = Rect::new(300.0, 200.0, 1000.0, 800.0);
        let r = shrink_rect(SCREEN, window);
        assert_eq!(r.width, 1000.0 - SCREEN.width * 0.10);
        assert_eq!(r.height, 800.0 - SCREEN.height * 0.10);

        // Repeated shrinking never goes below 10% of usable.
        let mut w = SCREEN;
        for _ in 0..50 {
            w = shrink_rect(SCREEN, w);
        }
        assert!(w.width >= SCREEN.width * MIN_SIZE_FRACTION - 1e-6);
        assert!(w.height >= SCREEN.height * MIN_SIZE_FRACTION - 1e-6);
    }

    #[test]
    fn grow_is_repeatable_and_reaches_full_in_bounded_steps() {
        let mut w = Rect::new(700.0, 450.0, 300.0, 200.0);
        for _ in 0..20 {
            w = grow_rect(SCREEN, w);
        }
        assert_eq!(w, full_rect(SCREEN));
    }

    #[test]
    fn almost_insets_beyond_the_already_padded_usable() {
        let usable = padded(SCREEN, 16.0);
        let r = almost_rect(usable, 48.0);
        assert_eq!(r, padded(usable, 48.0));
        assert!(r.x > usable.x);
        assert!(r.width < usable.width);
    }

    #[test]
    fn almost_zero_padding_equals_full() {
        let usable = padded(SCREEN, 16.0);
        assert_eq!(almost_rect(usable, 0.0), full_rect(usable));
    }

    #[test]
    fn thirds_cover_usable_width_with_no_gap_or_overlap() {
        let usable = Rect::new(0.0, 0.0, 3440.0, 1440.0); // divisible by 3? no (1146.66)
        let left = third_rect(usable, Third::Left);
        let center = third_rect(usable, Third::Center);
        let right = third_rect(usable, Third::Right);
        assert_eq!(left.x, usable.x);
        assert_eq!(center.x, left.x + left.width);
        assert_eq!(right.x, center.x + center.width);
        assert_eq!(right.x + right.width, usable.x + usable.width);
        assert_eq!(left.height, usable.height);
        assert_eq!(center.height, usable.height);
        assert_eq!(right.height, usable.height);
    }

    #[test]
    fn thirds_split_evenly_on_a_width_divisible_by_three() {
        let usable = Rect::new(0.0, 0.0, 1200.0, 800.0);
        assert_eq!(third_rect(usable, Third::Left).width, 400.0);
        assert_eq!(third_rect(usable, Third::Center).width, 400.0);
        assert_eq!(third_rect(usable, Third::Right).width, 400.0);
    }

    #[test]
    fn third_cycle_starts_at_left_and_advances() {
        assert_eq!(next_third(None), Third::Left);
        assert_eq!(next_third(Some(Third::Left)), Third::Center);
        assert_eq!(next_third(Some(Third::Center)), Third::Right);
        assert_eq!(next_third(Some(Third::Right)), Third::Left);
    }

    #[test]
    fn detect_third_is_stateless() {
        let usable = Rect::new(0.0, 0.0, 1200.0, 800.0);
        let center = third_rect(usable, Third::Center);
        assert_eq!(detect_third(usable, center), Some(Third::Center));
        assert_eq!(
            detect_third(usable, Rect::new(10.0, 10.0, 50.0, 50.0)),
            None
        );
    }

    #[test]
    fn supported_percent_accepts_any_integer_from_1_to_100() {
        for p in [1, 25, 33, 40, 50, 67, 75, 99, 100] {
            assert!(is_supported_percent(p));
        }
        assert!(!is_supported_percent(0));
        assert!(!is_supported_percent(101));
    }

    #[test]
    fn arbitrary_percent_sizes_are_a_fraction_of_usable() {
        let r = directional_rect(SCREEN, Position::Left, 33);
        assert_eq!(r.width, 1728.0 * 0.33);
        assert_eq!(r.height, SCREEN.height);

        let r = sized_rect(SCREEN, 40);
        assert_eq!(r.width, 1728.0 * 0.40);
        assert_eq!(r.height, 1117.0 * 0.40);
    }

    #[test]
    fn cycle_ignores_arbitrary_percents_and_still_enters_at_50() {
        let window = directional_rect(SCREEN, Position::Left, 33);
        let current = detect_directional_percent(SCREEN, Position::Left, window);
        assert_eq!(current, None);
        assert_eq!(next_cycle_percent(current), 50);
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
    fn resolve_display_index_next_wraps() {
        assert_eq!(resolve_display_index(0, 3, DisplayTarget::Next), Some(1));
        assert_eq!(resolve_display_index(2, 3, DisplayTarget::Next), Some(0));
    }

    #[test]
    fn resolve_display_index_previous_wraps() {
        assert_eq!(
            resolve_display_index(0, 3, DisplayTarget::Previous),
            Some(2)
        );
        assert_eq!(
            resolve_display_index(1, 3, DisplayTarget::Previous),
            Some(0)
        );
    }

    #[test]
    fn resolve_display_index_explicit_index_is_one_based() {
        assert_eq!(
            resolve_display_index(0, 3, DisplayTarget::Index(1)),
            Some(0)
        );
        assert_eq!(
            resolve_display_index(0, 3, DisplayTarget::Index(3)),
            Some(2)
        );
    }

    #[test]
    fn resolve_display_index_rejects_out_of_range() {
        assert_eq!(resolve_display_index(0, 3, DisplayTarget::Index(0)), None);
        assert_eq!(resolve_display_index(0, 3, DisplayTarget::Index(4)), None);
    }

    #[test]
    fn map_rect_between_displays_preserves_relative_position() {
        let from = Rect::new(0.0, 0.0, 1000.0, 1000.0);
        let to = Rect::new(2000.0, 0.0, 2000.0, 1000.0);
        let window = Rect::new(0.0, 0.0, 500.0, 500.0); // left-50%
        let mapped = map_rect_between_displays(window, from, to);
        assert_eq!(mapped, Rect::new(2000.0, 0.0, 1000.0, 500.0));
    }

    #[test]
    fn map_rect_between_displays_handles_negative_origin_and_different_scale() {
        let from = Rect::new(-1920.0, 0.0, 1920.0, 1080.0);
        let to = Rect::new(0.0, 0.0, 1728.0, 1117.0);
        let window = Rect::new(-1920.0 + 960.0, 0.0, 960.0, 1080.0); // right-50% of A
        let mapped = map_rect_between_displays(window, from, to);
        assert_eq!(mapped.x, 864.0);
        assert_eq!(mapped.width, 864.0);
        assert_eq!(mapped.height, 1117.0);
    }

    #[test]
    fn map_rect_between_displays_clamps_oversized_window_into_smaller_destination() {
        let from = Rect::new(0.0, 0.0, 3440.0, 1440.0);
        let to = Rect::new(0.0, 0.0, 1280.0, 800.0);
        let window = Rect::new(0.0, 0.0, 3440.0, 1440.0); // full on A
        let mapped = map_rect_between_displays(window, from, to);
        assert_eq!(mapped, to);
        assert!(mapped.x >= to.x);
        assert!(mapped.y >= to.y);
        assert!(mapped.x + mapped.width <= to.x + to.width + 1e-9);
        assert!(mapped.y + mapped.height <= to.y + to.height + 1e-9);
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
