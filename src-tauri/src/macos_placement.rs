//! macOS desktop points, never per-display physical pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
impl Rect {
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height, self.x + self.width, self.y + self.height].iter().all(|n| n.is_finite()) && self.width > 0.0 && self.height > 0.0
    }
}
pub(super) fn logical_rect(x: f64, y: f64, width: f64, height: f64, scale: f64) -> Option<Rect> {
    if !scale.is_finite() || scale <= 0.0 { return None; }
    let rect = Rect { x: x / scale, y: y / scale, width: width / scale, height: height / scale };
    rect.valid().then_some(rect)
}
pub(super) fn place(current: Rect, desired: (f64, f64), areas: &[Rect], fallback: usize) -> Option<Rect> {
    if !current.valid() || !desired.0.is_finite() || !desired.1.is_finite() || desired.0 <= 0.0 || desired.1 <= 0.0 { return None; }
    // Pixels on different screens have no common scale on macOS. Only compare
    // the converted desktop points. Cursor location never displaces a valid window.
    let mut owner = None;
    let mut largest = 0.0;
    for (index, area) in areas.iter().enumerate().filter(|(_, area)| area.valid()) {
        let width = (current.x + current.width).min(area.x + area.width) - current.x.max(area.x);
        let height = (current.y + current.height).min(area.y + area.height) - current.y.max(area.y);
        let overlap = width.max(0.0) * height.max(0.0);
        if overlap > largest { largest = overlap; owner = Some(index); }
    }
    let area = owner.and_then(|index| areas.get(index))
        .or_else(|| areas.get(fallback).filter(|area| area.valid()))
        .or_else(|| areas.iter().find(|area| area.valid()))?;
    let width = desired.0.min(area.width);
    let height = desired.1.min(area.height);
    let (x, y) = if owner.is_some() {
        (current.x.clamp(area.x, area.x + area.width - width), current.y.clamp(area.y, area.y + area.height - height))
    } else {
        (area.x + (area.width - width) / 2.0, area.y + (area.height - height) / 2.0)
    };
    Some(Rect { x, y, width, height })
}
pub(super) fn cursor_point(x: f64, y: f64, primary_scale: f64) -> Option<(f64, f64)> {
    if ![x, y, primary_scale].iter().all(|n| n.is_finite()) || primary_scale <= 0.0 { return None; }
    let point = (x / primary_scale, y / primary_scale);
    (point.0.is_finite() && point.1.is_finite()).then_some(point)
}
#[derive(Debug, PartialEq)]
pub(super) enum Change { Resize(f64, f64), Move(f64, f64) }
pub(super) fn changes(current: Rect, target: Rect) -> Vec<Change> {
    let resized = (current.width - target.width).abs() > 0.5 || (current.height - target.height).abs() > 0.5;
    let mut changes = Vec::new();
    if resized { changes.push(Change::Resize(target.width, target.height)); }
    if resized || (current.x - target.x).abs() > 0.5 || (current.y - target.y).abs() > 0.5 { changes.push(Change::Move(target.x, target.y)); }
    changes
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: f64, y: f64, width: f64, height: f64) -> Rect { Rect { x, y, width, height } }
    #[test]
    fn unchanged_position_emits_nothing_but_resize_always_restores_top_left_last() {
        let current = r(2400.,200.,720.,480.);
        assert!(changes(current, current).is_empty());
        assert_eq!(changes(current, r(2400.,200.,600.,400.)), vec![Change::Resize(600.,400.),Change::Move(2400.,200.)]);
        assert_eq!(changes(current, r(2300.,200.,720.,480.)), vec![Change::Move(2300.,200.)]);
    }
    #[test]
    fn cursor_physical_coordinates_use_primary_scale_not_destination_scale() {
        assert_eq!(cursor_point(4800.,400.,2.),Some((2400.,200.)));
        assert_eq!(cursor_point(-2400.,-200.,2.),Some((-1200.,-100.)));
        assert_eq!(cursor_point(4800.,400.,0.),None);
        assert_eq!(cursor_point(f64::NAN,400.,2.),None);
    }
    #[test]
    fn mixed_scale_external_position_wins_over_pointer_primary() {
        let primary = logical_rect(0., 50., 2880., 1750., 2.).unwrap();
        let external = logical_rect(1440., 25., 1920., 1055., 1.).unwrap();
        let current = logical_rect(2400., 200., 720., 480., 1.).unwrap();
        assert_eq!(place(current, (720.,480.), &[primary,external], 0), Some(current));
        let on_retina = logical_rect(320., 240., 1440., 960., 2.).unwrap();
        assert_eq!(place(on_retina, (720.,480.), &[primary,external], 1), Some(on_retina));
    }
    #[test]
    fn negative_and_vertical_displays_preserve_global_points() {
        for area in [r(-1920., 0., 1920., 1080.),r(0.,-1080.,1920.,1080.)] {
            let current = r(area.x+100.,area.y+80.,720.,480.);
            assert_eq!(place(current,(720.,480.), &[r(0.,0.,1440.,900.),area],0),Some(current));
        }
    }
    #[test]
    fn detached_screen_falls_back_and_small_work_area_fits() {
        assert_eq!(place(r(2400.,200.,720.,480.), (720.,480.), &[r(0.,25.,600.,400.)],0),Some(r(0.,25.,600.,400.)));
        assert_eq!(place(r(-3000.,0.,720.,480.), (720.,480.), &[r(0.,0.,1440.,900.),r(1440.,0.,1920.,1080.)],1),Some(r(2040.,300.,720.,480.)));
    }
    #[test]
    fn partial_overlap_clamps_and_largest_overlap_owns() {
        assert_eq!(place(r(1300.,100.,720.,480.),(720.,480.),&[r(0.,0.,1440.,900.),r(1440.,0.,1920.,1080.)],0),Some(r(1440.,100.,720.,480.)));
        assert_eq!(place(r(100.,100.,600.,400.),(720.,480.),&[r(0.,0.,800.,600.)],0),Some(r(80.,100.,720.,480.)));
    }
    #[test]
    fn invalid_geometry_never_reaches_native_operations() {
        for scale in [0., -1., f64::NAN, f64::INFINITY] { assert!(logical_rect(0.,0.,800.,600.,scale).is_none()); }
        assert!(place(r(0.,0.,720.,480.),(720.,480.),&[],0).is_none());
        assert!(place(r(0.,0.,720.,480.),(f64::NAN,480.),&[r(0.,0.,800.,600.)],0).is_none());
        assert!(place(r(0.,0.,720.,480.),(720.,480.),&[r(0.,0.,0.,600.)],0).is_none());
    }
}
