//! Deterministic tile layout assignment. Pure geometry — takes a window
//! count and the usable display rect, returns one rect per window in the
//! same order the windows were given (focused window first, PRD §15).

use crate::layout::Rect;

/// Computes tile rects for `count` windows within `usable`, edge-to-edge
/// unless `gap` is non-zero (PRD §16).
pub fn tile_rects(usable: Rect, count: usize, gap: f64) -> Vec<Rect> {
    let raw = match count {
        0 => return Vec::new(),
        1 => vec![usable],
        2 => {
            let half = usable.width / 2.0;
            vec![
                Rect::new(usable.x, usable.y, half, usable.height),
                Rect::new(
                    usable.x + half,
                    usable.y,
                    usable.width - half,
                    usable.height,
                ),
            ]
        }
        3 => {
            let master_width = usable.width / 2.0;
            let stack_width = usable.width - master_width;
            let stack_x = usable.x + master_width;
            let half_height = usable.height / 2.0;
            vec![
                Rect::new(usable.x, usable.y, master_width, usable.height),
                Rect::new(stack_x, usable.y, stack_width, half_height),
                Rect::new(
                    stack_x,
                    usable.y + half_height,
                    stack_width,
                    usable.height - half_height,
                ),
            ]
        }
        4 => {
            let half_w = usable.width / 2.0;
            let half_h = usable.height / 2.0;
            vec![
                Rect::new(usable.x, usable.y, half_w, half_h),
                Rect::new(usable.x + half_w, usable.y, usable.width - half_w, half_h),
                Rect::new(usable.x, usable.y + half_h, half_w, usable.height - half_h),
                Rect::new(
                    usable.x + half_w,
                    usable.y + half_h,
                    usable.width - half_w,
                    usable.height - half_h,
                ),
            ]
        }
        n => grid(usable, n),
    };

    raw.into_iter().map(|r| inset(r, gap, usable)).collect()
}

/// 5+: balanced grid, `columns = ceil(sqrt(n))`, `rows = ceil(n / columns)`,
/// last row may have fewer windows (PRD §14).
fn grid(usable: Rect, n: usize) -> Vec<Rect> {
    let columns = (n as f64).sqrt().ceil() as usize;
    let rows = n.div_ceil(columns);

    let mut rects = Vec::with_capacity(n);
    let mut remaining = n;
    for row in 0..rows {
        let cols_in_row = if row == rows - 1 && remaining < columns {
            remaining
        } else {
            columns
        };
        let cell_h = usable.height / rows as f64;
        let cell_w = usable.width / cols_in_row as f64;
        for col in 0..cols_in_row {
            rects.push(Rect::new(
                usable.x + col as f64 * cell_w,
                usable.y + row as f64 * cell_h,
                cell_w,
                cell_h,
            ));
        }
        remaining -= cols_in_row;
    }
    rects
}

/// Shrinks a tile rect by `gap / 2` on each side that does NOT sit on the
/// usable area's boundary, so adjacent tiles keep an even gap between them
/// while the outer edges stay flush with the screen.
fn inset(rect: Rect, gap: f64, usable: Rect) -> Rect {
    if gap <= 0.0 {
        return rect;
    }
    const EPS: f64 = 1e-6;
    let half = gap / 2.0;
    let on_left = (rect.x - usable.x).abs() < EPS;
    let on_right = (rect.x + rect.width - (usable.x + usable.width)).abs() < EPS;
    let on_top = (rect.y - usable.y).abs() < EPS;
    let on_bottom = (rect.y + rect.height - (usable.y + usable.height)).abs() < EPS;

    let dx0 = if on_left { 0.0 } else { half };
    let dx1 = if on_right { 0.0 } else { half };
    let dy0 = if on_top { 0.0 } else { half };
    let dy1 = if on_bottom { 0.0 } else { half };
    Rect::new(
        rect.x + dx0,
        rect.y + dy0,
        rect.width - dx0 - dx1,
        rect.height - dy0 - dy1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1200.0,
        height: 800.0,
    };

    fn assert_no_overlap(rects: &[Rect]) {
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let a = rects[i];
                let b = rects[j];
                let overlap_x = a.x < b.x + b.width && b.x < a.x + a.width;
                let overlap_y = a.y < b.y + b.height && b.y < a.y + a.height;
                assert!(
                    !(overlap_x && overlap_y),
                    "tiles {i} and {j} overlap: {a:?} {b:?}"
                );
            }
        }
    }

    fn assert_inside_bounds(rects: &[Rect], bounds: Rect) {
        for r in rects {
            assert!(r.x >= bounds.x - 1e-9);
            assert!(r.y >= bounds.y - 1e-9);
            assert!(r.x + r.width <= bounds.x + bounds.width + 1e-9);
            assert!(r.y + r.height <= bounds.y + bounds.height + 1e-9);
        }
    }

    #[test]
    fn one_window_fills_screen() {
        let rects = tile_rects(SCREEN, 1, 0.0);
        assert_eq!(rects, vec![SCREEN]);
    }

    #[test]
    fn two_windows_split_50_50() {
        let rects = tile_rects(SCREEN, 2, 0.0);
        assert_eq!(rects[0], Rect::new(0.0, 0.0, 600.0, 800.0));
        assert_eq!(rects[1], Rect::new(600.0, 0.0, 600.0, 800.0));
    }

    #[test]
    fn three_windows_master_stack() {
        let rects = tile_rects(SCREEN, 3, 0.0);
        assert_eq!(rects[0], Rect::new(0.0, 0.0, 600.0, 800.0));
        assert_eq!(rects[1], Rect::new(600.0, 0.0, 600.0, 400.0));
        assert_eq!(rects[2], Rect::new(600.0, 400.0, 600.0, 400.0));
    }

    #[test]
    fn four_windows_grid() {
        let rects = tile_rects(SCREEN, 4, 0.0);
        assert_eq!(rects[0], Rect::new(0.0, 0.0, 600.0, 400.0));
        assert_eq!(rects[1], Rect::new(600.0, 0.0, 600.0, 400.0));
        assert_eq!(rects[2], Rect::new(0.0, 400.0, 600.0, 400.0));
        assert_eq!(rects[3], Rect::new(600.0, 400.0, 600.0, 400.0));
    }

    #[test]
    fn five_windows_balanced_grid_three_columns() {
        // columns = ceil(sqrt(5)) = 3, rows = ceil(5/3) = 2, last row has 2.
        let rects = tile_rects(SCREEN, 5, 0.0);
        assert_eq!(rects.len(), 5);
        assert_no_overlap(&rects);
        assert_inside_bounds(&rects, SCREEN);
    }

    #[test]
    fn six_windows_two_by_three() {
        let rects = tile_rects(SCREEN, 6, 0.0);
        assert_eq!(rects.len(), 6);
        assert_eq!(rects[0], Rect::new(0.0, 0.0, 400.0, 400.0));
        assert_eq!(rects[3], Rect::new(0.0, 400.0, 400.0, 400.0));
        assert_no_overlap(&rects);
        assert_inside_bounds(&rects, SCREEN);
    }

    #[test]
    fn seven_plus_windows_stay_balanced_and_inside_bounds() {
        for n in 7..=12 {
            let rects = tile_rects(SCREEN, n, 0.0);
            assert_eq!(rects.len(), n);
            assert_no_overlap(&rects);
            assert_inside_bounds(&rects, SCREEN);
        }
    }

    #[test]
    fn zero_windows_returns_empty() {
        assert_eq!(tile_rects(SCREEN, 0, 0.0), Vec::new());
    }

    #[test]
    fn gap_shrinks_shared_edges_but_not_screen_edges() {
        let rects = tile_rects(SCREEN, 2, 8.0);
        assert_eq!(rects[0], Rect::new(0.0, 0.0, 596.0, 800.0));
        assert_eq!(rects[1], Rect::new(604.0, 0.0, 596.0, 800.0));
        assert_no_overlap(&rects);
    }

    #[test]
    fn gap_tiling_stays_inside_bounds_for_grids() {
        let rects = tile_rects(SCREEN, 6, 8.0);
        assert_no_overlap(&rects);
        assert_inside_bounds(&rects, SCREEN);
    }

    #[test]
    fn deterministic_repeat_calls_produce_identical_layout() {
        let a = tile_rects(SCREEN, 6, 4.0);
        let b = tile_rects(SCREEN, 6, 4.0);
        assert_eq!(a, b);
    }
}
