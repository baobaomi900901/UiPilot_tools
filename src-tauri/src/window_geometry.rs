//! Convert configured logical client size using the current display, not the old window size.

pub(crate) fn main_client_size(
    logical: (f64, f64),
    scale: f64,
    work_area: (u32, u32),
    frame: (u32, u32),
) -> Option<(u32, u32)> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let dimension = |logical: f64, available: u32, frame: u32| {
        let physical = (logical * scale).round();
        let available = available.checked_sub(frame)?;
        if !physical.is_finite() || physical < 1.0 || available == 0 {
            return None;
        }
        Some(physical.min(f64::from(available)) as u32)
    };
    Some((
        dimension(logical.0, work_area.0, frame.0)?,
        dimension(logical.1, work_area.1, frame.1)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::main_client_size;

    #[test]
    fn restores_configured_size_after_remote_dpi_changes_without_accumulating_scale() {
        // The observed stale 714x416 physical client must not become the logical baseline.
        for (scale, expected) in [
            (1.0, (720, 420)),
            (1.5, (1080, 630)),
            (2.0, (1440, 840)),
            (1.5, (1080, 630)),
            (1.0, (720, 420)),
        ] {
            assert_eq!(
                main_client_size((720.0, 420.0), scale, (1920, 1040), (22, 13)),
                Some(expected)
            );
        }
    }

    #[test]
    fn small_remote_work_area_accounts_for_native_frame_and_recovers_on_larger_display() {
        assert_eq!(
            main_client_size((720.0, 420.0), 2.0, (1024, 768), (22, 13)),
            Some((1002, 755))
        );
        assert_eq!(
            main_client_size((720.0, 420.0), 2.0, (2560, 1400), (22, 13)),
            Some((1440, 840))
        );
    }

    #[test]
    fn rejects_invalid_geometry_instead_of_collapsing_the_window() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                main_client_size((720.0, 420.0), scale, (1920, 1040), (0, 0)),
                None
            );
        }
        assert_eq!(
            main_client_size((720.0, 420.0), 1.0, (10, 10), (22, 13)),
            None
        );
        assert_eq!(
            main_client_size((0.0, 420.0), 1.0, (1920, 1040), (0, 0)),
            None
        );
    }
}
