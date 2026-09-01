//! Pure spatial-neighbor picking, shared by `snap focus` and `snap swap`
//! (and later, `snap stack`'s raise). No Accessibility calls here —
//! everything is unit-testable given a focused rect and candidate rects.

use crate::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Index into `others` of the nearest window in `direction` from `focused`,
/// or `None` if nothing qualifies.
///
/// A candidate `B` is "in direction `D`" of `focused` iff `B`'s center lies
/// in the half-plane of `D` relative to `focused`'s center (snap's `Rect`
/// has y increasing downward — Quartz/CG space — so `Up` means a smaller
/// `y`). Among qualifying candidates, nearest Euclidean center distance
/// wins; ties break by larger overlap along the perpendicular axis, then
/// lower `y`, then lower `x` (matching tile's sort) for determinism.
/// Candidates whose center exactly coincides with `focused`'s are skipped.
pub fn neighbor_in_direction(
    focused: Rect,
    others: &[Rect],
    direction: Direction,
) -> Option<usize> {
    let (fx, fy) = center(focused);

    let mut candidates: Vec<(usize, f64, f64, f64, f64)> = others
        .iter()
        .enumerate()
        .filter_map(|(i, &r)| {
            let (cx, cy) = center(r);
            let in_half_plane = match direction {
                Direction::Left => cx < fx,
                Direction::Right => cx > fx,
                Direction::Up => cy < fy,
                Direction::Down => cy > fy,
            };
            if !in_half_plane {
                return None;
            }
            let dist = ((cx - fx).powi(2) + (cy - fy).powi(2)).sqrt();
            if dist <= 0.0 {
                return None;
            }
            let overlap = match direction {
                Direction::Left | Direction::Right => vertical_overlap(focused, r),
                Direction::Up | Direction::Down => horizontal_overlap(focused, r),
            };
            Some((i, dist, overlap, r.y, r.x))
        })
        .collect();

    candidates.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap()
            .then(b.2.partial_cmp(&a.2).unwrap())
            .then(a.3.partial_cmp(&b.3).unwrap())
            .then(a.4.partial_cmp(&b.4).unwrap())
    });
    candidates.first().map(|c| c.0)
}

fn center(r: Rect) -> (f64, f64) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn vertical_overlap(a: Rect, b: Rect) -> f64 {
    let top = a.y.max(b.y);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (bottom - top).max(0.0)
}

fn horizontal_overlap(a: Rect, b: Rect) -> f64 {
    let left = a.x.max(b.x);
    let right = (a.x + a.width).min(b.x + b.width);
    (right - left).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_window_to_the_right() {
        let focused = Rect::new(0.0, 0.0, 600.0, 800.0);
        let right = Rect::new(600.0, 0.0, 600.0, 800.0);
        assert_eq!(
            neighbor_in_direction(focused, &[right], Direction::Right),
            Some(0)
        );
        assert_eq!(
            neighbor_in_direction(focused, &[right], Direction::Left),
            None
        );
    }

    #[test]
    fn picks_the_window_above_with_cg_y_down_axis() {
        // Up means smaller y in snap's Rect (CG space, y increases downward).
        let focused = Rect::new(0.0, 400.0, 600.0, 400.0);
        let above = Rect::new(0.0, 0.0, 600.0, 400.0);
        assert_eq!(
            neighbor_in_direction(focused, &[above], Direction::Up),
            Some(0)
        );
        assert_eq!(
            neighbor_in_direction(focused, &[above], Direction::Down),
            None
        );
    }

    #[test]
    fn nearest_center_wins_among_multiple_candidates() {
        let focused = Rect::new(0.0, 0.0, 400.0, 400.0);
        let near = Rect::new(400.0, 0.0, 400.0, 400.0);
        let far = Rect::new(1200.0, 0.0, 400.0, 400.0);
        assert_eq!(
            neighbor_in_direction(focused, &[far, near], Direction::Right),
            Some(1)
        );
    }

    #[test]
    fn three_master_stack_navigation() {
        // Master (left half) + two stacked panes on the right (PRD example).
        let master = Rect::new(0.0, 0.0, 600.0, 800.0);
        let upper_stack = Rect::new(600.0, 0.0, 600.0, 400.0);
        let lower_stack = Rect::new(600.0, 400.0, 600.0, 400.0);
        let others = [upper_stack, lower_stack];

        // From master, right hits the upper stack (closer center).
        assert_eq!(
            neighbor_in_direction(master, &others, Direction::Right),
            Some(0)
        );
        // From upper stack, down hits lower stack.
        assert_eq!(
            neighbor_in_direction(upper_stack, &[master, lower_stack], Direction::Down),
            Some(1)
        );
        // From upper stack, left returns to master.
        assert_eq!(
            neighbor_in_direction(upper_stack, &[master, lower_stack], Direction::Left),
            Some(0)
        );
    }

    #[test]
    fn tie_break_prefers_larger_perpendicular_overlap() {
        let focused = Rect::new(0.0, 0.0, 400.0, 400.0);
        // Both candidates' centers are equidistant to the right, but `b`
        // overlaps `focused` vertically more.
        let a = Rect::new(400.0, -300.0, 400.0, 400.0);
        let b = Rect::new(400.0, -100.0, 400.0, 800.0);
        let result = neighbor_in_direction(focused, &[a, b], Direction::Right);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn no_neighbor_returns_none() {
        let focused = Rect::new(0.0, 0.0, 400.0, 400.0);
        let left = Rect::new(-400.0, 0.0, 400.0, 400.0);
        assert_eq!(
            neighbor_in_direction(focused, &[left], Direction::Right),
            None
        );
    }

    #[test]
    fn other_display_candidates_are_excluded_by_caller_not_picker() {
        // The picker itself is display-agnostic; callers pass only
        // same-display candidates (documented contract, not enforced here).
        let focused = Rect::new(0.0, 0.0, 400.0, 400.0);
        let far_right = Rect::new(3000.0, 0.0, 400.0, 400.0);
        assert_eq!(
            neighbor_in_direction(focused, &[far_right], Direction::Right),
            Some(0)
        );
    }

    #[test]
    fn coincident_centers_are_skipped() {
        let focused = Rect::new(0.0, 0.0, 400.0, 400.0);
        let same_center = Rect::new(0.0, 0.0, 400.0, 400.0);
        assert_eq!(
            neighbor_in_direction(focused, &[same_center], Direction::Right),
            None
        );
    }

    #[test]
    fn empty_candidates_returns_none() {
        let focused = Rect::new(0.0, 0.0, 400.0, 400.0);
        assert_eq!(neighbor_in_direction(focused, &[], Direction::Left), None);
    }
}
