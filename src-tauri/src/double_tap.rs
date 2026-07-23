use std::time::Instant;

use crate::hotkey::{DoubleTapModifier, DOUBLE_TAP_WINDOW};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TapKey {
    Ctrl,
    Alt,
    Other,
}

#[derive(Debug, Default)]
pub(crate) struct DoubleTapDetector {
    pending: Option<(DoubleTapModifier, Instant)>,
    ctrl_down: bool,
    alt_down: bool,
}

impl DoubleTapDetector {
    pub(crate) fn on_key_down(&mut self, key: TapKey, now: Instant) -> Option<DoubleTapModifier> {
        let (modifier, is_down) = match key {
            TapKey::Ctrl => (DoubleTapModifier::Ctrl, &mut self.ctrl_down),
            TapKey::Alt => (DoubleTapModifier::Alt, &mut self.alt_down),
            TapKey::Other => {
                self.pending = None;
                return None;
            }
        };
        if *is_down {
            return None;
        }
        *is_down = true;
        match self.pending.take() {
            Some((pending, at))
                if pending == modifier && now.duration_since(at) <= DOUBLE_TAP_WINDOW =>
            {
                Some(modifier)
            }
            _ => {
                self.pending = Some((modifier, now));
                None
            }
        }
    }

    pub(crate) fn on_key_up(&mut self, key: TapKey) {
        match key {
            TapKey::Ctrl => self.ctrl_down = false,
            TapKey::Alt => self.alt_down = false,
            TapKey::Other => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{DoubleTapDetector, TapKey};
    use crate::hotkey::DoubleTapModifier;

    #[test]
    fn double_ctrl_within_window_fires_once() {
        let mut d = DoubleTapDetector::default();
        let t0 = Instant::now();
        assert_eq!(d.on_key_down(TapKey::Ctrl, t0), None);
        d.on_key_up(TapKey::Ctrl);
        assert_eq!(
            d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(399)),
            Some(DoubleTapModifier::Ctrl)
        );
        d.on_key_up(TapKey::Ctrl);
        assert_eq!(
            d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(400)),
            None
        );
    }

    #[test]
    fn held_modifier_repeats_never_fire_without_release() {
        let start = Instant::now();
        for key in [TapKey::Ctrl, TapKey::Alt] {
            let mut detector = DoubleTapDetector::default();
            assert_eq!(detector.on_key_down(key, start), None);
            for elapsed in [50, 100, 200, 399] {
                assert_eq!(
                    detector.on_key_down(key, start + Duration::from_millis(elapsed)),
                    None
                );
            }
        }
    }

    #[test]
    fn outside_window_restarts_pending() {
        let mut d = DoubleTapDetector::default();
        let t0 = Instant::now();
        assert_eq!(d.on_key_down(TapKey::Alt, t0), None);
        d.on_key_up(TapKey::Alt);
        assert_eq!(
            d.on_key_down(TapKey::Alt, t0 + Duration::from_millis(401)),
            None
        );
        d.on_key_up(TapKey::Alt);
        assert_eq!(
            d.on_key_down(TapKey::Alt, t0 + Duration::from_millis(500)),
            Some(DoubleTapModifier::Alt)
        );
    }

    #[test]
    fn other_key_clears_pending() {
        let mut d = DoubleTapDetector::default();
        let t0 = Instant::now();
        assert_eq!(d.on_key_down(TapKey::Ctrl, t0), None);
        d.on_key_up(TapKey::Ctrl);
        assert_eq!(
            d.on_key_down(TapKey::Other, t0 + Duration::from_millis(10)),
            None
        );
        assert_eq!(
            d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(20)),
            None
        );
    }
}
