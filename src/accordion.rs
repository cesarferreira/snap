//! Pure "stack of papers" geometry for `snap stack`. No Accessibility calls
//! here — everything is unit-testable given a usable rect and a window
//! count.
//!
//! Every window in the stack is the *same size* — like real sheets of
//! paper, not windows resized down to thin strips. The front (top of the
//! stack, currently focused) sits flush against the trailing edge (right
//! for wide displays, bottom for tall ones); each window behind it is
//! offset toward the leading edge by a fixed step, so it's mostly covered
//! by the ones in front of it and only its leading `peek`-wide sliver shows
//! — the same effect a real messy pile of same-sized papers gives when
//! fanned slightly. This means correct z-order matters: callers must raise
//! each window bottom-to-top (see `main::apply_cascade`) so a window
//! actually covers the ones behind it.

use crate::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Picked automatically from the display shape (AeroSpace's
/// `default-root-container-orientation = auto`): wide/square displays fan
/// left-to-right, tall ones fan top-to-bottom.
pub fn orientation_for(usable: Rect) -> Orientation {
    if usable.width >= usable.height {
        Orientation::Horizontal
    } else {
        Orientation::Vertical
    }
}

/// The total leading-to-trailing offset budget is capped at this fraction of
/// the display's relevant dimension, so every window (all sharing the same
/// size) keeps a usable minimum size regardless of how many are stacked.
const MAX_FAN_FRACTION: f64 = 0.6;

pub fn primary_size(r: Rect, orientation: Orientation) -> f64 {
    match orientation {
        Orientation::Horizontal => r.width,
        Orientation::Vertical => r.height,
    }
}

fn primary_coord(r: Rect, orientation: Orientation) -> f64 {
    match orientation {
        Orientation::Horizontal => r.x,
        Orientation::Vertical => r.y,
    }
}

/// One rect per stack slot, all the same size: `slot[n-1]` (the front) sits
/// flush against the trailing edge; `slot[0]` (the bottom of the stack) sits
/// flush against the leading edge; everything in between steps evenly from
/// one to the other. Rendered with slot 0 raised first and slot `n-1` last
/// (see `main::apply_cascade`), each slot's leading `peek`-wide sliver is
/// the only part not covered by the slot in front of it.
pub fn cascade_rects(usable: Rect, n: usize, peek: f64) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![usable];
    }
    let orientation = orientation_for(usable);
    let dim = match orientation {
        Orientation::Horizontal => usable.width,
        Orientation::Vertical => usable.height,
    };
    let count = n - 1;
    let max_total = (dim * MAX_FAN_FRACTION).max(0.0);
    let step = peek.min(max_total / count as f64).max(0.0);
    let size = (dim - step * count as f64).max(0.0);

    (0..n)
        .map(|slot| {
            let offset = step * slot as f64;
            match orientation {
                Orientation::Horizontal => {
                    Rect::new(usable.x + offset, usable.y, size, usable.height)
                }
                Orientation::Vertical => Rect::new(usable.x, usable.y + offset, usable.width, size),
            }
        })
        .collect()
}

/// Generous tolerance, matching the other cycle-detection code in
/// `layout.rs` — apps commonly land a few points off an exact request.
const EPS: f64 = 20.0;

/// Recovers the current stack order (candidate index per slot, `frames`
/// index space) from live window frames — used so `snap stack
/// next`/`previous` work without tracking any state between invocations.
/// Requires: at least 2 windows, all the same size, strictly increasing
/// along the primary axis, and consecutive windows overlapping (the gap
/// between them is smaller than their shared size) — that combination is
/// specific enough to not be confused with e.g. `tile columns` (equal
/// sizes, but no overlap: the gap equals the size) or `tile master` (one
/// big window leading, not trailing). Returns `None` if the frames don't
/// currently look like a cascade at all (e.g. the user dragged one).
pub fn detect_order(usable: Rect, frames: &[Rect]) -> Option<Vec<usize>> {
    let n = frames.len();
    if n < 2 {
        return None;
    }
    let orientation = orientation_for(usable);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        primary_coord(frames[a], orientation)
            .partial_cmp(&primary_coord(frames[b], orientation))
            .unwrap()
    });
    let front_size = primary_size(frames[order[n - 1]], orientation);
    if front_size <= 0.0 {
        return None;
    }
    for pair in order.windows(2) {
        let (a, b) = (frames[pair[0]], frames[pair[1]]);
        let gap = primary_coord(b, orientation) - primary_coord(a, orientation);
        if gap <= EPS {
            return None; // not strictly increasing enough to be distinct slots
        }
        if gap >= front_size - EPS {
            return None; // no overlap — looks like a tile, not a cascade
        }
        if (primary_size(a, orientation) - front_size).abs() > EPS {
            return None; // sizes differ — not our uniform-size cascade
        }
    }
    Some(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HORIZONTAL: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1728.0,
        height: 1117.0,
    };
    const VERTICAL: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1117.0,
        height: 1728.0,
    };

    #[test]
    fn orientation_picks_horizontal_for_wide_displays() {
        assert_eq!(orientation_for(HORIZONTAL), Orientation::Horizontal);
    }

    #[test]
    fn orientation_picks_vertical_for_tall_displays() {
        assert_eq!(orientation_for(VERTICAL), Orientation::Vertical);
    }

    #[test]
    fn single_window_fills_usable() {
        let rects = cascade_rects(HORIZONTAL, 1, 30.0);
        assert_eq!(rects, vec![HORIZONTAL]);
    }

    #[test]
    fn two_windows_same_size_front_flush_trailing() {
        let rects = cascade_rects(HORIZONTAL, 2, 30.0);
        let size = HORIZONTAL.width - 30.0;
        assert_eq!(rects[0], Rect::new(0.0, 0.0, size, HORIZONTAL.height));
        assert_eq!(rects[1], Rect::new(30.0, 0.0, size, HORIZONTAL.height));
        // Same size, just offset — not resized to a thin sliver.
        assert_eq!(rects[0].width, rects[1].width);
    }

    #[test]
    fn two_windows_vertical_front_flush_bottom() {
        let rects = cascade_rects(VERTICAL, 2, 30.0);
        let size = VERTICAL.height - 30.0;
        assert_eq!(rects[0], Rect::new(0.0, 0.0, VERTICAL.width, size));
        assert_eq!(rects[1], Rect::new(0.0, 30.0, VERTICAL.width, size));
    }

    #[test]
    fn four_windows_all_same_size_stepped_evenly() {
        let n = 4;
        let peek = 30.0;
        let rects = cascade_rects(HORIZONTAL, n, peek);
        let size = HORIZONTAL.width - peek * 3.0;
        for (slot, r) in rects.iter().enumerate() {
            assert_eq!(
                *r,
                Rect::new(peek * slot as f64, 0.0, size, HORIZONTAL.height)
            );
        }
        // Every slot shares the exact same width — this is the whole point.
        for w in rects.windows(2) {
            assert_eq!(w[0].width, w[1].width);
        }
        // The front (last slot) is flush against the trailing edge.
        assert_eq!(rects[n - 1].x + rects[n - 1].width, HORIZONTAL.width);
        // Consecutive slots overlap substantially (that's what makes only a
        // sliver of each visible once z-order is applied).
        for w in rects.windows(2) {
            let overlap = (w[0].x + w[0].width) - w[1].x;
            assert!(overlap > 0.0 && overlap < size);
        }
    }

    #[test]
    fn peek_zero_collapses_all_windows_onto_usable() {
        let rects = cascade_rects(HORIZONTAL, 2, 0.0);
        assert_eq!(rects[0], HORIZONTAL);
        assert_eq!(rects[1], HORIZONTAL);
    }

    #[test]
    fn detect_order_recovers_the_permutation() {
        let n = 3;
        let peek = 30.0;
        let rects = cascade_rects(HORIZONTAL, n, peek);
        // Windows applied in a shuffled candidate order: candidate 2 is the
        // bottom of the stack, candidate 0 the middle, candidate 1 the front.
        let frames = vec![rects[1], rects[2], rects[0]];
        assert_eq!(detect_order(HORIZONTAL, &frames), Some(vec![2, 0, 1]));
    }

    #[test]
    fn detect_order_none_when_frames_are_not_stacked() {
        let frames = [
            Rect::new(0.0, 0.0, 400.0, 400.0),
            Rect::new(400.0, 0.0, 400.0, 400.0),
        ];
        assert_eq!(detect_order(HORIZONTAL, &frames), None);
    }

    #[test]
    fn detect_order_none_for_tile_columns_equal_size_but_no_overlap() {
        // Equal widths, increasing x, but the gap equals the size (no
        // overlap) — a plain tile, not a cascade.
        let w = HORIZONTAL.width / 2.0;
        let frames = [
            Rect::new(0.0, 0.0, w, HORIZONTAL.height),
            Rect::new(w, 0.0, w, HORIZONTAL.height),
        ];
        assert_eq!(detect_order(HORIZONTAL, &frames), None);
    }

    #[test]
    fn detect_order_none_for_master_tile_where_sizes_differ() {
        let frames = [
            Rect::new(0.0, 0.0, 900.0, HORIZONTAL.height),
            Rect::new(900.0, 0.0, 828.0, HORIZONTAL.height),
        ];
        assert_eq!(detect_order(HORIZONTAL, &frames), None);
    }

    #[test]
    fn remainder_pixels_do_not_panic_and_stay_inside_bounds() {
        let usable = Rect::new(0.0, 0.0, 1001.0, 667.0);
        for n in 1..=6 {
            let rects = cascade_rects(usable, n, 30.0);
            for r in &rects {
                assert!(r.x >= usable.x - 1e-9);
                assert!(r.y >= usable.y - 1e-9);
                assert!(r.x + r.width <= usable.x + usable.width + 1e-9);
                assert!(r.y + r.height <= usable.y + usable.height + 1e-9);
            }
        }
    }

    #[test]
    fn many_windows_still_leaves_a_usable_minimum_size() {
        let usable = HORIZONTAL;
        let n = 10;
        let rects = cascade_rects(usable, n, 30.0);
        assert!(rects[0].width >= usable.width * (1.0 - MAX_FAN_FRACTION) - 1e-9);
    }
}
