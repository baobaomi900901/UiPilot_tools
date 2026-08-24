use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use tauri::{image::Image, tray::TrayIcon, Runtime};

use crate::message_center::NativeEffectError;

const FLASH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TrayVisual {
    #[default]
    Normal,
    Transparent,
}

pub(crate) trait TrayAttentionPort: Send + Sync {
    fn set_visual(&self, visual: TrayVisual) -> Result<(), NativeEffectError>;
    fn shutdown(&self);
}

#[derive(Debug, Default)]
pub(crate) struct TrayAnimation {
    active: bool,
    degraded: bool,
    visual: TrayVisual,
    next_toggle: Option<Instant>,
}

impl TrayAnimation {
    pub(crate) fn activate(&mut self, now: Instant) -> Option<TrayVisual> {
        if self.degraded || self.active {
            return None;
        }
        self.active = true;
        self.next_toggle = now.checked_add(FLASH_INTERVAL);
        self.set_visual(TrayVisual::Transparent)
    }

    pub(crate) fn focus(&mut self, focused: bool) -> Option<TrayVisual> {
        if !focused {
            return None;
        }
        self.active = false;
        self.next_toggle = None;
        self.visual = TrayVisual::Normal;
        Some(TrayVisual::Normal)
    }

    pub(crate) fn advance(&mut self, now: Instant) -> Option<TrayVisual> {
        if self.degraded || !self.active {
            return None;
        }
        let next = self.next_toggle?;
        if now < next {
            return None;
        }
        let elapsed = now.duration_since(next);
        let intervals = elapsed.as_millis() / FLASH_INTERVAL.as_millis() + 1;
        self.next_toggle =
            next.checked_add(FLASH_INTERVAL * u32::try_from(intervals).unwrap_or(u32::MAX));
        if intervals.is_multiple_of(2) {
            return None;
        }
        self.set_visual(match self.visual {
            TrayVisual::Normal => TrayVisual::Transparent,
            TrayVisual::Transparent => TrayVisual::Normal,
        })
    }

    pub(crate) fn wait_duration(&self, now: Instant) -> Option<Duration> {
        self.next_toggle
            .map(|next| next.saturating_duration_since(now))
    }

    pub(crate) fn degrade(&mut self) -> Option<TrayVisual> {
        self.degraded = true;
        self.active = false;
        self.next_toggle = None;
        self.set_visual(TrayVisual::Normal)
    }

    fn set_visual(&mut self, visual: TrayVisual) -> Option<TrayVisual> {
        if self.visual == visual {
            return None;
        }
        self.visual = visual;
        Some(visual)
    }
}

struct TauriTrayPort<R: Runtime> {
    tray: TrayIcon<R>,
    normal: Image<'static>,
}

impl<R: Runtime> TrayAttentionPort for TauriTrayPort<R> {
    fn set_visual(&self, visual: TrayVisual) -> Result<(), NativeEffectError> {
        let icon = match visual {
            TrayVisual::Normal => Some(self.normal.clone()),
            TrayVisual::Transparent => None,
        };
        self.tray.set_icon(icon).map_err(|_| NativeEffectError)
    }

    fn shutdown(&self) {
        let _ = self.tray.set_icon(Some(self.normal.clone()));
    }
}

pub(crate) fn tauri_tray_port<R: Runtime>(
    tray: TrayIcon<R>,
    normal: Image<'static>,
) -> Arc<dyn TrayAttentionPort> {
    Arc::new(TauriTrayPort { tray, normal })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{TrayAnimation, TrayVisual};

    #[test]
    fn animation_activates_once_toggles_and_focus_restores_normal() {
        let start = Instant::now();
        let mut animation = TrayAnimation::default();

        assert_eq!(animation.activate(start), Some(TrayVisual::Transparent));
        assert_eq!(animation.activate(start), None);
        assert_eq!(animation.advance(start + Duration::from_millis(499)), None);
        assert_eq!(
            animation.advance(start + Duration::from_millis(500)),
            Some(TrayVisual::Normal)
        );
        assert_eq!(animation.focus(true), Some(TrayVisual::Normal));
        assert_eq!(animation.wait_duration(start), None);
    }

    #[test]
    fn degradation_restores_normal_and_blocks_future_animation() {
        let start = Instant::now();
        let mut animation = TrayAnimation::default();
        assert_eq!(animation.activate(start), Some(TrayVisual::Transparent));

        assert_eq!(animation.degrade(), Some(TrayVisual::Normal));
        assert_eq!(animation.activate(start), None);
        assert_eq!(animation.advance(start + Duration::from_secs(1)), None);
    }
}
