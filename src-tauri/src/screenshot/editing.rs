use super::model::{PhysicalPoint, PhysicalRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

impl Handle {
    pub(crate) const ALL: [Self; 8] = [
        Self::NorthWest,
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
    ];
    fn axes(self) -> (i32, i32) {
        match self {
            Self::NorthWest => (-1, -1),
            Self::North => (0, -1),
            Self::NorthEast => (1, -1),
            Self::East => (1, 0),
            Self::SouthEast => (1, 1),
            Self::South => (0, 1),
            Self::SouthWest => (-1, 1),
            Self::West => (-1, 0),
        }
    }
    fn from_axes(x: i32, y: i32) -> Self {
        match (x, y) {
            (-1, -1) => Self::NorthWest,
            (0, -1) => Self::North,
            (1, -1) => Self::NorthEast,
            (1, 0) => Self::East,
            (1, 1) => Self::SouthEast,
            (0, 1) => Self::South,
            (-1, 1) => Self::SouthWest,
            (-1, 0) => Self::West,
            _ => unreachable!("a resize handle has at least one active axis"),
        }
    }
    pub(crate) fn center(self, rect: PhysicalRect) -> PhysicalPoint {
        let (x, y) = self.axes();
        PhysicalPoint {
            x: match x {
                -1 => rect.left,
                1 => rect.right,
                _ => midpoint(rect.left, rect.right),
            },
            y: match y {
                -1 => rect.top,
                1 => rect.bottom,
                _ => midpoint(rect.top, rect.bottom),
            },
        }
    }
    pub(crate) fn visual_rect(self, rect: PhysicalRect, dpi: u32) -> PhysicalRect {
        centered_rect(self.center(rect), dip_pixels(8, dpi))
    }
}

#[derive(Debug, Clone, Copy)]
struct ResizeDrag {
    original: PhysicalRect,
    down: PhysicalPoint,
    handle: Handle,
    active: Handle,
}

#[derive(Debug)]
pub(crate) struct EditingSelection {
    rect: PhysicalRect,
    bounds: PhysicalRect,
    drag: Option<ResizeDrag>,
}
impl EditingSelection {
    pub(crate) fn new(rect: PhysicalRect, bounds: PhysicalRect) -> Option<Self> {
        (valid(rect)
            && valid(bounds)
            && rect.left >= bounds.left
            && rect.top >= bounds.top
            && rect.right <= bounds.right
            && rect.bottom <= bounds.bottom)
            .then_some(Self {
                rect,
                bounds,
                drag: None,
            })
    }
    pub(crate) fn rect(&self) -> PhysicalRect {
        self.rect
    }
    pub(crate) fn begin_resize(&mut self, handle: Handle, point: PhysicalPoint) -> bool {
        if self.drag.is_some() {
            return false;
        }
        self.drag = Some(ResizeDrag {
            original: self.rect,
            down: point,
            handle,
            active: handle,
        });
        true
    }
    pub(crate) fn drag_to(&mut self, point: PhysicalPoint) -> Option<Handle> {
        let drag = self.drag.as_mut()?;
        let (x, y) = drag.handle.axes();
        let (active_x, active_y) = drag.active.axes();
        let (left, right, new_x) = resize_axis(
            drag.original.left,
            drag.original.right,
            self.bounds.left,
            self.bounds.right,
            x,
            active_x,
            i64::from(point.x) - i64::from(drag.down.x),
        );
        let (top, bottom, new_y) = resize_axis(
            drag.original.top,
            drag.original.bottom,
            self.bounds.top,
            self.bounds.bottom,
            y,
            active_y,
            i64::from(point.y) - i64::from(drag.down.y),
        );
        self.rect = PhysicalRect {
            left,
            top,
            right,
            bottom,
        };
        drag.active = Handle::from_axes(new_x, new_y);
        Some(drag.active)
    }
    pub(crate) fn end_resize(&mut self) {
        self.drag = None;
    }
    pub(crate) fn resizing(&self) -> bool {
        self.drag.is_some()
    }
    pub(crate) fn active_handle(&self) -> Option<Handle> {
        self.drag.map(|drag| drag.active)
    }
    /// The caller maps arrows to physical-pixel deltas (1, or 10 with Shift).
    pub(crate) fn nudge(&mut self, handle: Handle, dx: i32, dy: i32) -> Handle {
        if self.resizing() {
            return self.active_handle().unwrap_or(handle);
        }
        self.begin_resize(handle, PhysicalPoint { x: 0, y: 0 });
        let active = self
            .drag_to(PhysicalPoint { x: dx, y: dy })
            .unwrap_or(handle);
        self.end_resize();
        active
    }
}

fn resize_axis(
    low: i32,
    high: i32,
    bound_low: i32,
    bound_high: i32,
    axis: i32,
    active: i32,
    delta: i64,
) -> (i32, i32, i32) {
    if axis == 0 {
        return (low, high, 0);
    }
    let (moving, anchor) = if axis < 0 { (low, high) } else { (high, low) };
    let mut moved =
        (i64::from(moving) + delta).clamp(i64::from(bound_low), i64::from(bound_high)) as i32;
    if moved == anchor {
        // At equality retain the last direction; only crossing flips the handle.
        moved = if active < 0 && anchor > bound_low {
            anchor - 1
        } else if active > 0 && anchor < bound_high {
            anchor + 1
        } else if anchor > bound_low {
            anchor - 1
        } else {
            anchor + 1
        };
    }
    (
        moved.min(anchor),
        moved.max(anchor),
        if moved < anchor { -1 } else { 1 },
    )
}

fn valid(rect: PhysicalRect) -> bool {
    rect.left < rect.right && rect.top < rect.bottom
}
fn midpoint(a: i32, b: i32) -> i32 {
    (i64::from(a) + (i64::from(b) - i64::from(a)) / 2) as i32
}
fn dip_pixels(dip: i32, dpi: u32) -> i32 {
    ((i64::from(dip.max(1)) * i64::from(if dpi == 0 { 96 } else { dpi }) + 48) / 96)
        .clamp(1, i64::from(i32::MAX)) as i32
}
fn centered_rect(point: PhysicalPoint, size: i32) -> PhysicalRect {
    let left = point.x.saturating_sub(size / 2);
    let top = point.y.saturating_sub(size / 2);
    PhysicalRect {
        left,
        top,
        right: left.saturating_add(size),
        bottom: top.saturating_add(size),
    }
}
fn point_in(rect: PhysicalRect, point: PhysicalPoint) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}
fn overlap(a: PhysicalRect, b: PhysicalRect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}
fn distance(a: PhysicalPoint, b: PhysicalPoint) -> i128 {
    let x = i128::from(a.x) - i128::from(b.x);
    let y = i128::from(a.y) - i128::from(b.y);
    x * x + y * y
}
fn hit_with_dpi(
    rect: PhysicalRect,
    point: PhysicalPoint,
    dpi: impl Fn(PhysicalPoint) -> u32,
) -> Option<Handle> {
    Handle::ALL
        .into_iter()
        .filter_map(|handle| {
            let center = handle.center(rect);
            let dpi = dpi(center);
            let hit_size = ((16_i64 * i64::from(if dpi == 0 { 96 } else { dpi }) + 95) / 96)
                .clamp(1, i64::from(i32::MAX)) as i32;
            point_in(centered_rect(center, hit_size), point).then(|| {
                let (x, y) = handle.axes();
                (
                    handle,
                    (
                        distance(center, point),
                        if x != 0 && y != 0 { 0 } else { 1 },
                    ),
                )
            })
        })
        .min_by_key(|(_, key)| *key)
        .map(|(handle, _)| handle)
}
pub(crate) fn hit_handle(rect: PhysicalRect, point: PhysicalPoint, dpi: u32) -> Option<Handle> {
    hit_with_dpi(rect, point, |_| dpi)
}
pub(crate) fn hit_handle_on_monitors(
    rect: PhysicalRect,
    point: PhysicalPoint,
    monitors: &[(PhysicalRect, u32)],
) -> Option<Handle> {
    hit_with_dpi(rect, point, |center| dpi_at(center, monitors))
}
fn dpi_at(point: PhysicalPoint, monitors: &[(PhysicalRect, u32)]) -> u32 {
    monitors
        .iter()
        .filter(|(monitor, _)| valid(*monitor))
        .min_by_key(|(monitor, _)| {
            if point_in(*monitor, point) {
                0
            } else {
                distance(
                    point,
                    PhysicalPoint {
                        x: point.x.clamp(monitor.left, monitor.right - 1),
                        y: point.y.clamp(monitor.top, monitor.bottom - 1),
                    },
                )
            }
        })
        .map(|(_, dpi)| *dpi)
        .unwrap_or(96)
}

fn button_monitor(rect: PhysicalRect, monitors: &[(PhysicalRect, u32)]) -> (PhysicalRect, u32) {
    let corner = PhysicalPoint {
        x: rect.right.saturating_sub(1),
        y: rect.bottom.saturating_sub(1),
    };
    monitors
        .iter()
        .copied()
        .filter(|(monitor, _)| valid(*monitor))
        .max_by_key(|(monitor, _)| {
            let width = (i64::from(rect.right.min(monitor.right))
                - i64::from(rect.left.max(monitor.left)))
            .max(0);
            let height = (i64::from(rect.bottom.min(monitor.bottom))
                - i64::from(rect.top.max(monitor.top)))
            .max(0);
            (
                point_in(*monitor, corner),
                i128::from(width) * i128::from(height),
            )
        })
        .unwrap_or((
            if valid(rect) {
                rect
            } else {
                PhysicalRect {
                    left: 0,
                    top: 0,
                    right: 1,
                    bottom: 1,
                }
            },
            96,
        ))
}

pub(crate) fn save_button_rect(
    rect: PhysicalRect,
    monitors: &[(PhysicalRect, u32)],
    width_dip: i32,
    height_dip: i32,
) -> PhysicalRect {
    let (monitor, dpi) = button_monitor(rect, monitors);
    let width = i64::from(dip_pixels(width_dip, dpi))
        .min(i64::from(monitor.right) - i64::from(monitor.left));
    let height = i64::from(dip_pixels(height_dip, dpi))
        .min(i64::from(monitor.bottom) - i64::from(monitor.top));
    let min_x = i64::from(monitor.left);
    let max_x = i64::from(monitor.right) - width;
    let min_y = i64::from(monitor.top);
    let max_y = i64::from(monitor.bottom) - height;
    let x = (i64::from(rect.right) - width).clamp(min_x, max_x);
    let below = i64::from(rect.bottom) + i64::from(dip_pixels(8, dpi));
    let above = i64::from(rect.top) - i64::from(dip_pixels(8, dpi)) - height;
    let at = |x: i64, y: i64| PhysicalRect {
        left: x as i32,
        top: y as i32,
        right: (x + width) as i32,
        bottom: (y + height) as i32,
    };
    let handles =
        Handle::ALL.map(|handle| handle.visual_rect(rect, dpi_at(handle.center(rect), monitors)));
    let clear = |button: PhysicalRect| handles.iter().all(|handle| !overlap(button, *handle));
    for y in [below, above] {
        if (min_y..=max_y).contains(&y) && clear(at(x, y)) {
            return at(x, y);
        }
    }
    let preferred_y = below.clamp(min_y, max_y);
    // In-screen fallback examines the edges of every forbidden handle region.
    // This also permits placing the button inside a full-screen selection.
    let mut xs = vec![x, min_x, max_x];
    let mut ys = vec![preferred_y, min_y, max_y];
    for handle in handles {
        xs.extend([
            (i64::from(handle.left) - width).clamp(min_x, max_x),
            i64::from(handle.right).clamp(min_x, max_x),
        ]);
        ys.extend([
            (i64::from(handle.top) - height).clamp(min_y, max_y),
            i64::from(handle.bottom).clamp(min_y, max_y),
        ]);
    }
    xs.into_iter()
        .flat_map(|candidate_x| {
            ys.iter()
                .copied()
                .map(move |candidate_y| at(candidate_x, candidate_y))
        })
        .filter(|button| clear(*button))
        .min_by_key(|button| {
            distance(
                PhysicalPoint {
                    x: button.left,
                    y: button.top,
                },
                PhysicalPoint {
                    x: x as i32,
                    y: preferred_y as i32,
                },
            )
        })
        // A monitor smaller than the button plus a handle has no disjoint layout.
        .unwrap_or_else(|| at(x, preferred_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(left: i32, top: i32, right: i32, bottom: i32) -> PhysicalRect {
        PhysicalRect {
            left,
            top,
            right,
            bottom,
        }
    }
    fn p(x: i32, y: i32) -> PhysicalPoint {
        PhysicalPoint { x, y }
    }
    fn contains(outer: PhysicalRect, inner: PhysicalRect) -> bool {
        inner.left >= outer.left
            && inner.top >= outer.top
            && inner.right <= outer.right
            && inner.bottom <= outer.bottom
    }
    fn intersects(a: PhysicalRect, b: PhysicalRect) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    #[test]
    fn all_eight_handles_resize_only_their_axes_without_pointer_jump() {
        let start = r(20, 30, 80, 90);
        for (handle, expected) in [
            (Handle::NorthWest, r(27, 41, 80, 90)),
            (Handle::North, r(20, 41, 80, 90)),
            (Handle::NorthEast, r(20, 41, 87, 90)),
            (Handle::East, r(20, 30, 87, 90)),
            (Handle::SouthEast, r(20, 30, 87, 101)),
            (Handle::South, r(20, 30, 80, 101)),
            (Handle::SouthWest, r(27, 30, 80, 101)),
            (Handle::West, r(27, 30, 80, 90)),
        ] {
            let mut edit = EditingSelection::new(start, r(-200, -200, 200, 200)).unwrap();
            let center = handle.center(start);
            let down = p(center.x + 3, center.y - 2);
            assert!(edit.begin_resize(handle, down));
            assert!(!edit.begin_resize(handle, down));
            assert_eq!(edit.drag_to(down), Some(handle));
            assert_eq!(edit.rect(), start);
            assert_eq!(edit.drag_to(p(down.x + 7, down.y + 11)), Some(handle));
            assert_eq!(edit.rect(), expected, "{handle:?}");
            edit.end_resize();
            assert!(!edit.resizing());
            assert_eq!(edit.drag_to(down), None);
        }
    }

    #[test]
    fn crossover_keeps_initial_opposite_edges_and_flips_both_axes() {
        let mut edit =
            EditingSelection::new(r(-80, -70, -20, -10), r(-100, -100, 100, 100)).unwrap();
        edit.begin_resize(Handle::NorthWest, p(-80, -70));
        assert_eq!(edit.drag_to(p(10, 20)), Some(Handle::SouthEast));
        assert_eq!(edit.rect(), r(-20, -10, 10, 20));
        assert_eq!(edit.active_handle(), Some(Handle::SouthEast));
        edit.drag_to(p(-90, -80));
        assert_eq!(edit.rect(), r(-90, -80, -20, -10));
        assert_eq!(edit.active_handle(), Some(Handle::NorthWest));
    }

    #[test]
    fn minimum_pixel_and_virtual_bounds_survive_equality_and_extreme_pointer() {
        let bounds = r(-100, -100, 100, 100);
        let mut edit = EditingSelection::new(r(-80, -70, -20, -10), bounds).unwrap();
        edit.begin_resize(Handle::NorthWest, p(-80, -70));
        edit.drag_to(p(-20, -10));
        assert_eq!(edit.rect(), r(-21, -11, -20, -10));
        edit.drag_to(p(i32::MAX, i32::MAX));
        assert_eq!(edit.rect(), r(-20, -10, 100, 100));
        edit.drag_to(p(i32::MIN, i32::MIN));
        assert_eq!(edit.rect(), r(-100, -100, -20, -10));
        assert!(EditingSelection::new(r(0, 0, 0, 5), bounds).is_none());
        assert!(EditingSelection::new(r(90, 90, 110, 110), bounds).is_none());
    }

    #[test]
    fn keyboard_nudge_uses_handle_axes_and_can_be_repeated_after_crossover() {
        let mut edit = EditingSelection::new(r(10, 10, 15, 15), r(0, 0, 100, 100)).unwrap();
        let active = edit.nudge(Handle::West, 10, 10);
        assert_eq!(active, Handle::East);
        assert_eq!(edit.rect(), r(15, 10, 20, 15));
        edit.nudge(active, 1, 0);
        assert_eq!(edit.rect(), r(15, 10, 21, 15));
        assert!(!edit.resizing());
    }

    #[test]
    fn hit_testing_chooses_nearest_then_corners_and_scales_with_dpi() {
        let small = r(0, 0, 2, 2);
        assert_eq!(hit_handle(small, p(1, 1), 96), Some(Handle::North));
        assert_eq!(hit_handle(r(0, 0, 2, 4), p(1, 0), 96), Some(Handle::North));
        assert_eq!(
            hit_handle(r(0, 0, 1, 1), p(0, 0), 96),
            Some(Handle::NorthWest)
        );
        assert_eq!(
            hit_handle(r(0, 0, 4, 4), p(1, 0), 96),
            Some(Handle::NorthWest)
        );
        let rect = r(-100, -100, 100, 100);
        assert_eq!(hit_handle(rect, p(-109, -100), 96), None);
        assert_eq!(
            hit_handle(rect, p(-109, -100), 144),
            Some(Handle::NorthWest)
        );
        assert_eq!(Handle::West.visual_rect(rect, 144), r(-106, -6, -94, 6));
        assert_eq!(
            hit_handle_on_monitors(
                rect,
                p(-109, -100),
                &[(r(-200, -200, 0, 200), 144), (r(0, -200, 200, 200), 96)]
            ),
            Some(Handle::NorthWest)
        );
    }

    #[test]
    fn fractional_dpi_hit_target_rounds_up_to_at_least_sixteen_dip() {
        assert_eq!(
            hit_handle(r(-100, -100, 100, 100), p(-92, -100), 97),
            Some(Handle::NorthWest)
        );
    }

    #[test]
    fn button_fallback_avoids_handle_size_from_neighboring_higher_dpi_monitor() {
        let screen = r(0, 0, 500, 300);
        let monitors = [(screen, 96), (r(500, 0, 1000, 300), 192)];
        let button = save_button_rect(screen, &monitors, 120, 32);
        assert!(contains(screen, button));
        assert!(!intersects(
            button,
            Handle::SouthEast.visual_rect(screen, 192)
        ));
    }

    #[test]
    fn button_prefers_below_then_above_and_uses_selected_monitor_dpi() {
        let monitors = [(r(-1000, 0, 0, 800), 144), (r(0, 0, 1000, 800), 96)];
        assert_eq!(
            save_button_rect(r(100, 100, 400, 300), &monitors, 120, 32),
            r(280, 308, 400, 340)
        );
        assert_eq!(
            save_button_rect(r(100, 600, 400, 790), &monitors, 120, 32),
            r(280, 560, 400, 592)
        );
        assert_eq!(
            save_button_rect(r(-500, 100, -100, 300), &monitors, 120, 32),
            r(-280, 312, -100, 360)
        );
    }

    #[test]
    fn button_stays_on_monitor_when_corner_is_in_gap_and_avoids_handles() {
        let monitors = [(r(-800, -600, 0, 0), 96), (r(100, 100, 900, 700), 96)];
        let selection = r(-600, -400, 50, 50);
        let button = save_button_rect(selection, &monitors, 120, 32);
        assert!(contains(monitors[0].0, button));
        let screen = r(0, 0, 500, 300);
        for selection in [
            screen,
            r(0, 0, 1, 1),
            r(499, 299, 500, 300),
            r(100, 1, 400, 299),
        ] {
            let button = save_button_rect(selection, &[(screen, 96)], 120, 32);
            assert!(contains(screen, button));
            for h in Handle::ALL {
                assert!(
                    !intersects(button, h.visual_rect(selection, 96)),
                    "{selection:?} {h:?} {button:?}"
                );
            }
        }
    }
}
