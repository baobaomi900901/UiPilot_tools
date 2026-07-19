#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShowTarget {
    Launcher,
    Settings,
}

#[derive(Debug, Default)]
struct Readiness {
    setup_ready: bool,
    frontend_ready: bool,
    pending_target: Option<ShowTarget>,
}

impl Readiness {
    fn request(&mut self, target: ShowTarget) -> Option<ShowTarget> {
        if self.setup_ready && self.frontend_ready {
            Some(target)
        } else {
            self.pending_target = Some(target);
            None
        }
    }

    fn mark_setup_ready(&mut self) -> Option<ShowTarget> {
        self.setup_ready = true;
        self.take_if_ready()
    }

    fn mark_frontend_ready(&mut self) -> Option<ShowTarget> {
        self.frontend_ready = true;
        self.take_if_ready()
    }

    fn take_if_ready(&mut self) -> Option<ShowTarget> {
        if self.setup_ready && self.frontend_ready {
            self.pending_target.take()
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModalState {
    Normal,
    Open,
    AwaitingFocusRestore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusDecision {
    Suppress,
    ClearAndHide,
    ReportWindowFailureAndHide,
}

impl ModalState {
    fn begin_export<F>(&mut self, query_focus: F) -> Result<(), FocusDecision>
    where
        F: FnOnce() -> Result<bool, ()>,
    {
        match self {
            Self::Normal => {
                *self = Self::Open;
                Ok(())
            }
            Self::Open => Err(FocusDecision::Suppress),
            Self::AwaitingFocusRestore => {
                *self = Self::Normal;
                match query_focus() {
                    Ok(true) => {
                        *self = Self::Open;
                        Ok(())
                    }
                    Ok(false) => Err(FocusDecision::ClearAndHide),
                    Err(()) => Err(FocusDecision::ReportWindowFailureAndHide),
                }
            }
        }
    }

    fn finish_export(&mut self) {
        if *self == Self::Open {
            *self = Self::AwaitingFocusRestore;
        }
    }

    fn on_focus<F>(&mut self, focused: bool, query_focus: F) -> FocusDecision
    where
        F: FnOnce() -> Result<bool, ()>,
    {
        match (*self, focused) {
            (Self::Open, _) => FocusDecision::Suppress,
            (Self::AwaitingFocusRestore, true) => {
                *self = Self::Normal;
                FocusDecision::Suppress
            }
            (Self::AwaitingFocusRestore, false) => {
                *self = Self::Normal;
                match query_focus() {
                    Ok(true) => FocusDecision::Suppress,
                    Ok(false) => FocusDecision::ClearAndHide,
                    Err(()) => FocusDecision::ReportWindowFailureAndHide,
                }
            }
            (Self::Normal, true) => FocusDecision::Suppress,
            (Self::Normal, false) => FocusDecision::ClearAndHide,
        }
    }

    fn on_successful_show(&mut self) {
        if *self == Self::AwaitingFocusRestore {
            *self = Self::Normal;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
    };

    use super::*;

    #[derive(Clone, Copy)]
    enum ReadyOrder {
        SetupFirst,
        FrontendFirst,
    }

    fn apply_ready_order(state: &mut Readiness, order: ReadyOrder) -> Vec<ShowTarget> {
        let results = match order {
            ReadyOrder::SetupFirst => [state.mark_setup_ready(), state.mark_frontend_ready()],
            ReadyOrder::FrontendFirst => [state.mark_frontend_ready(), state.mark_setup_ready()],
        };
        results.into_iter().flatten().collect()
    }

    #[test]
    fn readiness_does_not_synthesize_a_startup_show() {
        for order in [ReadyOrder::SetupFirst, ReadyOrder::FrontendFirst] {
            let mut state = Readiness::default();
            assert_eq!(
                apply_ready_order(&mut state, order),
                Vec::<ShowTarget>::new()
            );
            assert_eq!(state.pending_target, None);
        }
    }

    #[test]
    fn readiness_drains_only_the_last_early_explicit_target_once() {
        for (target, order) in [
            (ShowTarget::Launcher, ReadyOrder::SetupFirst),
            (ShowTarget::Settings, ReadyOrder::FrontendFirst),
        ] {
            let mut state = Readiness::default();
            assert_eq!(state.request(target), None);
            assert_eq!(apply_ready_order(&mut state, order), vec![target]);
            assert_eq!(state.mark_setup_ready(), None);
            assert_eq!(state.mark_frontend_ready(), None);
        }

        let mut state = Readiness::default();
        assert_eq!(state.request(ShowTarget::Launcher), None);
        assert_eq!(state.request(ShowTarget::Settings), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_frontend_ready(), Some(ShowTarget::Settings));
        assert_eq!(state.mark_frontend_ready(), None);
    }

    #[test]
    fn readiness_returns_each_request_after_both_ready() {
        let mut state = Readiness::default();
        assert!(apply_ready_order(&mut state, ReadyOrder::SetupFirst).is_empty());

        for target in [ShowTarget::Launcher, ShowTarget::Settings] {
            assert_eq!(state.request(target), Some(target));
            assert_eq!(state.pending_target, None);
        }
    }

    #[test]
    fn readiness_marks_are_idempotent_and_state_has_only_approved_fields() {
        let mut state = Readiness {
            setup_ready: false,
            frontend_ready: false,
            pending_target: None,
        };
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.request(ShowTarget::Launcher), None);
        assert_eq!(state.mark_setup_ready(), None);
        assert_eq!(state.mark_frontend_ready(), Some(ShowTarget::Launcher));
        assert_eq!(state.mark_frontend_ready(), None);
    }

    #[test]
    fn modal_normal_and_open_export_paths_allow_only_one_chooser() {
        let queries = Cell::new(0);
        let mut state = ModalState::Normal;
        assert_eq!(
            state.begin_export(|| {
                queries.set(queries.get() + 1);
                Ok(true)
            }),
            Ok(())
        );
        assert_eq!(state, ModalState::Open);
        assert_eq!(
            state.begin_export(|| {
                queries.set(queries.get() + 1);
                Ok(true)
            }),
            Err(FocusDecision::Suppress)
        );
        assert_eq!(state, ModalState::Open);
        assert_eq!(queries.get(), 0);
    }

    #[test]
    fn modal_guard_drop_and_focus_events_restore_normal() {
        let mut state = ModalState::Open;
        assert_eq!(
            state.on_focus(false, || panic!("Open must not query focus")),
            FocusDecision::Suppress
        );
        assert_eq!(state, ModalState::Open);

        state.finish_export();
        assert_eq!(state, ModalState::AwaitingFocusRestore);
        assert_eq!(
            state.on_focus(true, || panic!("Focused(true) must not query focus")),
            FocusDecision::Suppress
        );
        assert_eq!(state, ModalState::Normal);
    }

    #[test]
    fn modal_awaiting_false_classifies_after_leaving_awaiting() {
        for (focus_result, expected) in [
            (Ok(true), FocusDecision::Suppress),
            (Ok(false), FocusDecision::ClearAndHide),
            (Err(()), FocusDecision::ReportWindowFailureAndHide),
        ] {
            let mut state = ModalState::AwaitingFocusRestore;
            assert_eq!(state.on_focus(false, || focus_result), expected);
            assert_eq!(state, ModalState::Normal);
        }

        let mut state = ModalState::AwaitingFocusRestore;
        let query = catch_unwind(AssertUnwindSafe(|| {
            state.on_focus(false, || panic!("focus query sentinel"));
        }));
        assert!(query.is_err());
        assert_eq!(state, ModalState::Normal);
    }

    #[test]
    fn modal_retry_and_successful_show_cannot_leave_awaiting_stuck() {
        let mut focused = ModalState::AwaitingFocusRestore;
        assert_eq!(focused.begin_export(|| Ok(true)), Ok(()));
        assert_eq!(focused, ModalState::Open);

        for (focus_result, expected) in [
            (Ok(false), FocusDecision::ClearAndHide),
            (Err(()), FocusDecision::ReportWindowFailureAndHide),
        ] {
            let mut state = ModalState::AwaitingFocusRestore;
            assert_eq!(state.begin_export(|| focus_result), Err(expected));
            assert_eq!(state, ModalState::Normal);
            assert_eq!(
                state.begin_export(|| panic!("Normal must not query focus")),
                Ok(())
            );
            assert_eq!(state, ModalState::Open);
        }

        let mut state = ModalState::AwaitingFocusRestore;
        state.on_successful_show();
        assert_eq!(state, ModalState::Normal);
        state = ModalState::Open;
        state.on_successful_show();
        assert_eq!(state, ModalState::Open);

        let mut state = ModalState::AwaitingFocusRestore;
        let query = catch_unwind(AssertUnwindSafe(|| {
            let _ = state.begin_export(|| panic!("retry focus query sentinel"));
        }));
        assert!(query.is_err());
        assert_eq!(state, ModalState::Normal);
    }
}
