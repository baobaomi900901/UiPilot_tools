//! Bounded physical damage used by the reusable native screenshot surfaces.
#[cfg(test)]
use crate::screenshot::model::PhysicalPoint;
use crate::screenshot::model::PhysicalRect;
pub(super) fn subtract(a: PhysicalRect, b: PhysicalRect) -> Vec<PhysicalRect> {
    let Some(i) = a.intersection(b) else {
        return vec![a];
    };
    [
        PhysicalRect {
            left: a.left,
            top: a.top,
            right: a.right,
            bottom: i.top,
        },
        PhysicalRect {
            left: a.left,
            top: i.bottom,
            right: a.right,
            bottom: a.bottom,
        },
        PhysicalRect {
            left: a.left,
            top: i.top,
            right: i.left,
            bottom: i.bottom,
        },
        PhysicalRect {
            left: i.right,
            top: i.top,
            right: a.right,
            bottom: i.bottom,
        },
    ]
    .into_iter()
    .filter(|r| r.right > r.left && r.bottom > r.top)
    .collect()
}
pub(super) fn selection_delta(
    old: Option<PhysicalRect>,
    new: Option<PhysicalRect>,
) -> Vec<PhysicalRect> {
    match (old, new) {
        (Some(a), Some(b)) => subtract(a, b).into_iter().chain(subtract(b, a)).collect(),
        (Some(a), None) | (None, Some(a)) => vec![a],
        _ => vec![],
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct UploadPatch {
    pub desktop: PhysicalRect,
    pub source_x: i32,
    pub source_y: i32,
    pub dest_x: i32,
    pub dest_y: i32,
}
pub(super) fn upload_patch(
    source: PhysicalRect,
    monitor: PhysicalRect,
    damage: PhysicalRect,
) -> Option<UploadPatch> {
    let desktop = source.intersection(monitor)?.intersection(damage)?;
    Some(UploadPatch {
        desktop,
        source_x: desktop.left - source.left,
        source_y: desktop.top - source.top,
        dest_x: desktop.left - monitor.left,
        dest_y: desktop.top - monitor.top,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(l: i32, t: i32, r: i32, b: i32) -> PhysicalRect {
        PhysicalRect {
            left: l,
            top: t,
            right: r,
            bottom: b,
        }
    }
    #[test]
    fn resize_damage_is_strips_not_whole_selected_canvas() {
        let old = r(80, 80, 3760, 2080);
        let new = r(80, 80, 3761, 2081);
        let damage = selection_delta(Some(old), Some(new));
        let area: u64 = damage
            .iter()
            .map(|r| u64::from(r.dimensions().unwrap().0) * u64::from(r.dimensions().unwrap().1))
            .sum();
        assert!(
            area < 10000,
            "resize of one pixel must not repaint8millionpixels: {area}"
        );
        for y in 0..2160 {
            for x in [0, 79, 80, 3759, 3760, 3761, 3839] {
                let p = PhysicalPoint { x, y };
                assert_eq!(
                    damage.iter().any(|r| r.contains(p)),
                    old.contains(p) != new.contains(p)
                );
            }
        }
    }
    #[test]
    fn opposite_corners_and_disjoint_rectangles_cover_exact_symmetric_difference() {
        for (old, new) in [
            (r(-200, -100, -10, 20), r(-180, -90, 50, 100)),
            (r(0, 0, 2, 2), r(20, 20, 22, 22)),
        ] {
            let d = selection_delta(Some(old), Some(new));
            for y in -110..110 {
                for x in -210..60 {
                    let p = PhysicalPoint { x, y };
                    assert_eq!(
                        d.iter().any(|r| r.contains(p)),
                        old.contains(p) != new.contains(p)
                    );
                }
            }
        }
    }
    #[test]
    fn damage_ack_and_partial_upload_coordinates_keep_negative_monitor_origin() {
        let monitor = r(-1920, 0, 0, 1080);
        let source = r(-2000, 10, 300, 600);
        let dirty = r(-1940, 20, -1800, 80);
        let patch = upload_patch(source, monitor, dirty).unwrap();
        assert_eq!(patch.desktop, r(-1920, 20, -1800, 80));
        assert_eq!(
            (patch.source_x, patch.source_y, patch.dest_x, patch.dest_y),
            (80, 10, 0, 20)
        );
    }
}
