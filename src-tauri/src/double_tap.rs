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
}

impl DoubleTapDetector {
    pub(crate) fn on_key_down(&mut self, key: TapKey, now: Instant) -> Option<DoubleTapModifier> {
        let modifier = match key {
            TapKey::Ctrl => DoubleTapModifier::Ctrl,
            TapKey::Alt => DoubleTapModifier::Alt,
            TapKey::Other => {
                self.pending = None;
                return None;
            }
        };
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

    pub(crate) fn reset(&mut self) {
        self.pending = None;
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
        assert_eq!(
            d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(399)),
            Some(DoubleTapModifier::Ctrl)
        );
        assert_eq!(d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(400)), None);
    }

    #[test]
    fn outside_window_restarts_pending() {
        let mut d = DoubleTapDetector::default();
        let t0 = Instant::now();
        assert_eq!(d.on_key_down(TapKey::Alt, t0), None);
        assert_eq!(d.on_key_down(TapKey::Alt, t0 + Duration::from_millis(401)), None);
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
        assert_eq!(d.on_key_down(TapKey::Other, t0 + Duration::from_millis(10)), None);
        assert_eq!(d.on_key_down(TapKey::Ctrl, t0 + Duration::from_millis(20)), None);
    }
}
