//! Physical layout shared by native controls and hidden rendering fixtures.
use crate::screenshot::model::{PhysicalPoint, PhysicalRect};
pub(super) struct Layout {
    pub rect: PhysicalRect,
    pub cells: Vec<PhysicalRect>,
}
pub(super) fn layout(
    selection: PhysicalRect,
    monitor: PhysicalRect,
    dpi: u32,
    properties: usize,
    avoid: Option<PhysicalRect>,
) -> Layout {
    let mut cell = ((28 * dpi as i32 + 48) / 96)
        .max(1)
        .min(monitor.right - monitor.left)
        .min(monitor.bottom - monitor.top);
    // Preserve nominal DIP size whenever rows fit. On exceptionally short/high-DPI
    // displays reduce the cell until every real row is reachable, rather than
    // clamping only the outer rectangle and emitting negative-height controls.
    while cell > 1 {
        let columns = ((monitor.right - monitor.left) / cell).clamp(1, 9) as usize;
        let rows = 9usize.div_ceil(columns) + properties.div_ceil(columns);
        if cell * rows as i32 <= monitor.bottom - monitor.top {
            break;
        }
        cell -= 1;
    }
    let columns = ((monitor.right - monitor.left) / cell).clamp(1, 9) as usize;
    let action_rows = 9usize.div_ceil(columns);
    let property_rows = properties.div_ceil(columns);
    let width = cell * (9.min(columns).max(properties.min(columns)) as i32);
    let height = (cell * (action_rows + property_rows) as i32).min(monitor.bottom - monitor.top);
    let left = (selection.right - width).clamp(monitor.left, monitor.right - width);
    let below = selection.bottom.saturating_add(8);
    let top = if below + height <= monitor.bottom {
        below
    } else {
        selection.top.saturating_sub(height + 8).max(monitor.top)
    }
    .min(monitor.bottom - height);
    let mut rect = PhysicalRect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    };
    if avoid.is_some_and(|a| a.intersection(rect).is_some()) {
        let a = avoid.unwrap();
        let candidates = [
            (left, a.top - height - 8),
            (left, a.bottom + 8),
            (a.left - width - 8, top),
            (a.right + 8, top),
            (monitor.left, monitor.top),
            (monitor.right - width, monitor.bottom - height),
        ];
        if let Some((x, y)) = candidates.into_iter().find(|(x, y)| {
            *x >= monitor.left
                && *y >= monitor.top
                && *x + width <= monitor.right
                && *y + height <= monitor.bottom
                && a.intersection(PhysicalRect {
                    left: *x,
                    top: *y,
                    right: *x + width,
                    bottom: *y + height,
                })
                .is_none()
        }) {
            rect = PhysicalRect {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            };
        }
    }
    let cells = (0..9 + properties)
        .map(|i| {
            let (row, col) = if i < 9 {
                (i / columns, i % columns)
            } else {
                (action_rows + (i - 9) / columns, (i - 9) % columns)
            };
            let x = rect.left + col as i32 * cell;
            let y = rect.top + row as i32 * cell;
            PhysicalRect {
                left: x,
                top: y,
                right: x + cell,
                bottom: y + cell,
            }
        })
        .collect();
    Layout { rect, cells }
}
pub(super) fn monitor_for(selection: PhysicalRect, monitors: &[(PhysicalRect, u32)]) -> usize {
    let p = PhysicalPoint {
        x: selection.right - 1,
        y: selection.bottom - 1,
    };
    monitors
        .iter()
        .position(|(r, _)| r.contains(p))
        .unwrap_or(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nine_actions_and_property_row_wrap_inside_small_negative_monitor() {
        for dpi in [96, 144, 192] {
            let m = PhysicalRect {
                left: -240,
                top: -300,
                right: 0,
                bottom: 0,
            };
            let s = PhysicalRect {
                left: -220,
                top: -260,
                right: -10,
                bottom: -80,
            };
            let l = layout(s, m, dpi, 8, None);
            assert_eq!(l.cells.len(), 17);
            for c in l.cells {
                assert!(
                    c.left >= m.left
                        && c.right <= m.right
                        && c.top >= m.top
                        && c.bottom <= m.bottom
                );
            }
        }
    }
    #[test]
    fn toolbar_avoids_input_when_space_exists() {
        let m = PhysicalRect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 700,
        };
        let a = PhysicalRect {
            left: 100,
            top: 100,
            right: 600,
            bottom: 500,
        };
        let l = layout(
            a,
            m,
            96,
            8,
            Some(PhysicalRect {
                left: 100,
                top: 500,
                right: 600,
                bottom: 700,
            }),
        );
        assert!(l
            .rect
            .intersection(PhysicalRect {
                left: 100,
                top: 500,
                right: 600,
                bottom: 700
            })
            .is_none());
    }
}

#[cfg(test)]
mod short_monitor_regression {
    use super::*;
    #[test]
    fn high_dpi_short_monitor_has_only_positive_reachable_cells() {
        for (w, h, dpi) in [(400, 300, 288), (160, 100, 192), (240, 160, 288)] {
            let m = PhysicalRect {
                left: -w,
                top: -h,
                right: 0,
                bottom: 0,
            };
            let l = layout(m, m, dpi, 8, None);
            for c in l.cells {
                assert!(
                    c.right > c.left
                        && c.bottom > c.top
                        && c.left >= m.left
                        && c.top >= m.top
                        && c.right <= m.right
                        && c.bottom <= m.bottom,
                    "unreachable {c:?}"
                );
            }
        }
    }
}
