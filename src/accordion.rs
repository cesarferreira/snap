//! Pure accordion/stack geometry for `snap stack`. No Accessibility calls
//! here — everything is unit-testable given a usable rect and a window
//! count/front index.

use crate::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Picked automatically from the display shape (AeroSpace's
/// `default-root-container-orientation = auto`): wide/square displays peek
/// left/right, tall ones peek top/bottom.
pub fn orientation_for(usable: Rect) -> Orientation {
    if usable.width >= usable.height {
        Orientation::Horizontal
    } else {
        Orientation::Vertical
    }
}

/// The front (focused) window's rect among `n` total stacked windows.
/// `n <= 1` is just `usable` (equivalent to `snap full`). For `n >= 2`, both
/// edges are inset by `peek` unconditionally, so both peek strips always
/// show something regardless of which window is currently front.
pub fn front_rect(usable: Rect, n: usize, peek: f64, orientation: Orientation) -> Rect {
    if n <= 1 {
        return usable;
    }
    let peek = peek.min(match orientation {
        Orientation::Horizontal => usable.width / 2.0,
        Orientation::Vertical => usable.height / 2.0,
    });
    match orientation {
        Orientation::Horizontal => Rect::new(
            usable.x + peek,
            usable.y,
            (usable.width - peek * 2.0).max(0.0),
            usable.height,
        ),
        Orientation::Vertical => Rect::new(
            usable.x,
            usable.y + peek,
            usable.width,
            (usable.height - peek * 2.0).max(0.0),
        ),
    }
}

/// The left/top peek strip, where the "previous" window in stack order sits.
fn prev_peek_rect(usable: Rect, peek: f64, orientation: Orientation) -> Rect {
    match orientation {
        Orientation::Horizontal => Rect::new(usable.x, usable.y, peek, usable.height),
        Orientation::Vertical => Rect::new(usable.x, usable.y, usable.width, peek),
    }
}

/// The right/bottom peek strip, where the "next" window in stack order sits.
fn next_peek_rect(usable: Rect, peek: f64, orientation: Orientation) -> Rect {
    match orientation {
        Orientation::Horizontal => Rect::new(
            usable.x + usable.width - peek,
            usable.y,
            peek,
            usable.height,
        ),
        Orientation::Vertical => Rect::new(
            usable.x,
            usable.y + usable.height - peek,
            usable.width,
            peek,
        ),
    }
}

/// One rect per window in stack order (`order[k]` is the rect for the `k`th
/// window), for `n` windows with `front` (0-based, wrapping) at the front.
/// `front - 1` (wrapping) gets the left/top peek, `front + 1` (wrapping)
/// gets the right/bottom peek, and every other window is hidden directly
/// behind the front (same rect — still "in the stack" for `next`/`previous`,
/// per the simpler v1 in the issue).
pub fn accordion_rects(usable: Rect, n: usize, front: usize, peek: f64) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![usable];
    }
    let orientation = orientation_for(usable);
    let front_r = front_rect(usable, n, peek, orientation);
    let prev_r = prev_peek_rect(usable, peek, orientation);
    let next_r = next_peek_rect(usable, peek, orientation);
    let prev_idx = (front + n - 1) % n;
    let next_idx = (front + 1) % n;

    (0..n)
        .map(|k| {
            if k == front {
                front_r
            } else if k == prev_idx {
                prev_r
            } else if k == next_idx {
                next_r
            } else {
                front_r
            }
        })
        .collect()
}

/// Generous tolerance, matching the other cycle-detection code in
/// `layout.rs` — apps commonly land a few points off an exact request.
const EPS: f64 = 20.0;

fn rects_match(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.width - b.width).abs() < EPS
        && (a.height - b.height).abs() < EPS
}

/// Which position in `frames` (stack order) currently looks like the front,
/// by matching [`front_rect`] — used so `snap stack next` works without
/// tracking any state between invocations.
pub fn detect_front(usable: Rect, peek: f64, frames: &[Rect]) -> Option<usize> {
    let n = frames.len();
    if n == 0 {
        return None;
    }
    let orientation = orientation_for(usable);
    let front_r = front_rect(usable, n, peek, orientation);
    frames.iter().position(|&f| rects_match(f, front_r))
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
        assert_eq!(
            front_rect(HORIZONTAL, 1, 30.0, Orientation::Horizontal),
            HORIZONTAL
        );
        let rects = accordion_rects(HORIZONTAL, 1, 0, 30.0);
        assert_eq!(rects, vec![HORIZONTAL]);
    }

    #[test]
    fn front_is_inset_on_both_sides_horizontal() {
        let r = front_rect(HORIZONTAL, 2, 30.0, Orientation::Horizontal);
        assert_eq!(
            r,
            Rect::new(30.0, 0.0, HORIZONTAL.width - 60.0, HORIZONTAL.height)
        );
    }

    #[test]
    fn front_is_inset_on_both_sides_vertical() {
        let r = front_rect(VERTICAL, 2, 30.0, Orientation::Vertical);
        assert_eq!(
            r,
            Rect::new(0.0, 30.0, VERTICAL.width, VERTICAL.height - 60.0)
        );
    }

    #[test]
    fn two_windows_other_gets_a_peek_strip() {
        let rects = accordion_rects(HORIZONTAL, 2, 0, 30.0);
        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            front_rect(HORIZONTAL, 2, 30.0, Orientation::Horizontal)
        );
        // The other window sits in one of the two peek strips, not hidden.
        let peek_left = Rect::new(0.0, 0.0, 30.0, HORIZONTAL.height);
        let peek_right = Rect::new(HORIZONTAL.width - 30.0, 0.0, 30.0, HORIZONTAL.height);
        assert!(rects[1] == peek_left || rects[1] == peek_right);
        assert_ne!(rects[1], rects[0]);
    }

    #[test]
    fn three_windows_each_next_brings_a_different_window_to_front() {
        let n = 3;
        let peek = 30.0;
        let mut fronts = std::collections::HashSet::new();
        for front in 0..n {
            let rects = accordion_rects(HORIZONTAL, n, front, peek);
            assert_eq!(
                rects[front],
                front_rect(HORIZONTAL, n, peek, Orientation::Horizontal)
            );
            fronts.insert(front);
        }
        assert_eq!(fronts.len(), n);
        // After n `next`s, we're back to the start.
        let mut front = 0;
        for _ in 0..n {
            front = (front + 1) % n;
        }
        assert_eq!(front, 0);
    }

    #[test]
    fn four_windows_neighbors_get_peek_strips_others_hidden_behind_front() {
        let n = 4;
        let peek = 30.0;
        let front = 1;
        let rects = accordion_rects(HORIZONTAL, n, front, peek);
        let front_r = front_rect(HORIZONTAL, n, peek, Orientation::Horizontal);
        assert_eq!(rects[1], front_r);
        assert_eq!(
            rects[0],
            prev_peek_rect(HORIZONTAL, peek, Orientation::Horizontal)
        ); // front-1
        assert_eq!(
            rects[2],
            next_peek_rect(HORIZONTAL, peek, Orientation::Horizontal)
        ); // front+1
        assert_eq!(rects[3], front_r); // hidden behind
    }

    #[test]
    fn peek_zero_front_still_fills_usable() {
        let rects = accordion_rects(HORIZONTAL, 2, 0, 0.0);
        assert_eq!(rects[0], HORIZONTAL);
    }

    #[test]
    fn detect_front_finds_the_matching_index() {
        let n = 3;
        let peek = 30.0;
        let rects = accordion_rects(HORIZONTAL, n, 2, peek);
        assert_eq!(detect_front(HORIZONTAL, peek, &rects), Some(2));
    }

    #[test]
    fn detect_front_none_when_frames_are_not_stacked() {
        let frames = [
            Rect::new(0.0, 0.0, 400.0, 400.0),
            Rect::new(400.0, 0.0, 400.0, 400.0),
        ];
        assert_eq!(detect_front(HORIZONTAL, 30.0, &frames), None);
    }

    #[test]
    fn remainder_pixels_do_not_panic_and_stay_inside_bounds() {
        let usable = Rect::new(0.0, 0.0, 1001.0, 667.0);
        for n in 1..=4 {
            for front in 0..n {
                let rects = accordion_rects(usable, n, front, 30.0);
                for r in &rects {
                    assert!(r.x >= usable.x - 1e-9);
                    assert!(r.y >= usable.y - 1e-9);
                    assert!(r.x + r.width <= usable.x + usable.width + 1e-9);
                    assert!(r.y + r.height <= usable.y + usable.height + 1e-9);
                }
            }
        }
    }
}
