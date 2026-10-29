//! Hide host-owned windows before freezing the desktop.
use super::NativeError;
use windows::Win32::{
    Foundation::HWND, Graphics::Dwm::DwmFlush, UI::WindowsAndMessaging::IsWindowVisible,
};

trait HideTarget {
    fn transitions(&self, disabled: bool) -> Result<(), NativeError>;
    fn hide(&self) -> Result<(), NativeError>;
    fn visible(&self) -> bool;
    fn flush(&self) -> Result<(), NativeError>;
}

fn hide_before_capture(target: &impl HideTarget) -> Result<(), NativeError> {
    // WS_VISIBLE can clear while DWM is still composing the exit animation.
    // Disable transitions BEFORE hide and keep them disabled through the present
    // barrier. These host windows otherwise use the default transition policy.
    target.transitions(true)?;
    struct Restore<'a, T: HideTarget> {
        target: &'a T,
        active: bool,
    }
    impl<T: HideTarget> Drop for Restore<'_, T> {
        fn drop(&mut self) {
            if self.active {
                let _ = self.target.transitions(false);
            }
        }
    }
    let mut restore = Restore {
        target,
        active: true,
    };
    let result = (|| {
        target.hide()?;
        if target.visible() {
            return Err(NativeError::CaptureFailed);
        }
        target.flush()
    })();
    let restored = target.transitions(false);
    restore.active = restored.is_err();
    result.and(restored)
}

pub(crate) fn hide_for_capture(
    hwnd: isize,
    hide: impl Fn() -> Result<(), NativeError>,
) -> Result<(), NativeError> {
    struct Window<F>(HWND, F);
    impl<F: Fn() -> Result<(), NativeError>> HideTarget for Window<F> {
        fn transitions(&self, disabled: bool) -> Result<(), NativeError> {
            use windows::{core::BOOL, Win32::Graphics::Dwm::*};
            let disabled = BOOL::from(disabled);
            unsafe {
                DwmSetWindowAttribute(
                    self.0,
                    DWMWA_TRANSITIONS_FORCEDISABLED,
                    &disabled as *const _ as *const _,
                    std::mem::size_of::<BOOL>() as u32,
                )
                .map_err(|_| NativeError::CaptureFailed)
            }
        }
        fn hide(&self) -> Result<(), NativeError> {
            (self.1)()
        }
        fn visible(&self) -> bool {
            unsafe { IsWindowVisible(self.0).as_bool() }
        }
        fn flush(&self) -> Result<(), NativeError> {
            unsafe { DwmFlush().map_err(|_| NativeError::CaptureFailed) }
        }
    }
    let started = std::time::Instant::now();
    let result = hide_before_capture(&Window(HWND(hwnd as *mut _), hide));
    crate::screenshot::probe::trace(format!(
        "host hide composition barrier hwnd={hwnd} elapsed_ms={} result={result:?}",
        started.elapsed().as_millis()
    ));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct AnimatedWindow {
        disabled: Cell<bool>,
        visible: Cell<bool>,
        rendered: Cell<bool>,
        failure: Option<&'static str>,
    }
    impl AnimatedWindow {
        fn shown(failure: Option<&'static str>) -> Self {
            Self {
                visible: Cell::new(true),
                rendered: Cell::new(true),
                failure,
                ..Default::default()
            }
        }
    }
    impl HideTarget for AnimatedWindow {
        fn transitions(&self, disabled: bool) -> Result<(), NativeError> {
            if disabled && self.failure == Some("disable") {
                return Err(NativeError::CaptureFailed);
            }
            self.disabled.set(disabled);
            Ok(())
        }
        fn hide(&self) -> Result<(), NativeError> {
            if self.failure == Some("panic") {
                panic!("simulated hide panic");
            }
            if self.failure == Some("hide") {
                return Err(NativeError::CaptureFailed);
            }
            if self.failure != Some("visible") {
                self.visible.set(false);
            }
            Ok(())
        }
        fn visible(&self) -> bool {
            self.visible.get()
        }
        fn flush(&self) -> Result<(), NativeError> {
            if self.failure == Some("flush") {
                return Err(NativeError::CaptureFailed);
            }
            // A present barrier alone does not finish an active exit animation.
            if self.disabled.get() && !self.visible.get() {
                self.rendered.set(false);
            }
            Ok(())
        }
    }
    #[test]
    fn frozen_desktop_cannot_contain_the_hidden_windows_exit_animation() {
        let target = AnimatedWindow::shown(None);
        hide_before_capture(&target).unwrap();
        assert!(
            !target.rendered.get(),
            "hidden HWND still has a composed exit frame"
        );
        assert!(
            !target.disabled.get(),
            "restore normal host transitions after the barrier"
        );
    }
    #[test]
    fn preparation_failure_restores_transitions_and_never_authorizes_capture() {
        for failure in ["disable", "hide", "visible", "flush"] {
            let target = AnimatedWindow::shown(Some(failure));
            assert!(hide_before_capture(&target).is_err(), "{failure}");
            assert!(!target.disabled.get(), "{failure}");
        }
    }
    #[test]
    fn unwinding_hide_restores_normal_transitions() {
        let target = AnimatedWindow::shown(Some("panic"));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = hide_before_capture(&target);
        }))
        .is_err());
        assert!(!target.disabled.get());
    }
    #[test]
    fn native_owned_window_preparation_hides_without_minimizing() {
        use windows::{core::w, Win32::UI::WindowsAndMessaging::*};
        struct Owned(HWND);
        impl Drop for Owned {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        // Hidden fixture only: do not display or capture the user's desktop.
        let window = Owned(unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("Screenshot preparation test"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        });
        let called = Cell::new(false);
        hide_for_capture(window.0 .0 as isize, || {
            called.set(true);
            unsafe {
                let _ = ShowWindow(window.0, SW_HIDE);
            }
            Ok(())
        })
        .unwrap();
        assert!(called.get());
        assert!(!unsafe { IsWindowVisible(window.0).as_bool() });
        assert!(!unsafe { IsIconic(window.0).as_bool() });
    }
}
