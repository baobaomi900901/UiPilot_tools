//! Dedicated host binding. The lifecycle operation gate serializes every edit.
use crate::{hotkey::command_chord, settings::Settings};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Mutex,
    },
};
use tauri_plugin_global_shortcut::Shortcut;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub(crate) shortcut: Shortcut,
    pub(crate) revision: u64,
}

#[derive(Debug, Default)]
struct State {
    revision: u64,
    active: Option<Binding>,
    // Includes uncertain registrations; these never dispatch and are retried on edit/exit.
    retired: BTreeMap<u32, Shortcut>,
}

#[derive(Debug, Default)]
pub(crate) struct ScreenshotHotkeys {
    state: Mutex<State>,
    native_id: AtomicU32,
    pressed: AtomicBool,
}

impl ScreenshotHotkeys {
    pub(crate) fn candidate(&self, id: u32) -> Option<Binding> {
        self.state
            .lock()
            .unwrap()
            .active
            .filter(|b| b.shortcut.id() == id)
    }
    pub(crate) fn accepts_press(&self, shortcut: Shortcut) -> bool {
        shortcut.id() != self.native_id.load(Ordering::Acquire)
            || !self.pressed.swap(true, Ordering::AcqRel)
    }
    pub(crate) fn release(&self, shortcut: Shortcut) {
        if shortcut.id() == self.native_id.load(Ordering::Acquire) {
            self.pressed.store(false, Ordering::Release);
        }
    }
    pub(crate) fn registrations(&self) -> Vec<Shortcut> {
        let state = self.state.lock().unwrap();
        state
            .active
            .iter()
            .map(|b| b.shortcut)
            .chain(state.retired.values().copied())
            .collect()
    }
    pub(crate) fn status(&self, saved: Option<&str>) -> &'static str {
        let Some(saved) = saved else {
            return "unset";
        };
        if command_chord(saved)
            .ok()
            .and_then(|s| self.candidate(s.id()))
            .is_some()
        {
            "active"
        } else {
            "unavailable"
        }
    }
    pub(crate) fn edit<R, U, P>(
        &self,
        desired: Option<Shortcut>,
        register: &mut R,
        unregister: &mut U,
        persist: P,
    ) -> Result<(), ()>
    where
        R: FnMut(Shortcut) -> Result<(), ()>,
        U: FnMut(Shortcut) -> Result<(), ()>,
        P: FnOnce() -> Result<(), ()>,
    {
        let (old, retired, revision) = {
            let state = self.state.lock().unwrap();
            (
                state.active,
                state.retired.clone(),
                state.revision.checked_add(1).ok_or(())?,
            )
        };
        let mut observed = retired.clone();
        if let Some(old) = old {
            observed.insert(old.shortcut.id(), old.shortcut);
        }
        let reused = old.is_some_and(|b| Some(b.shortcut) == desired);
        let mut added = false;
        let mut removed = false;
        let mut uncertain = false;
        let result = (|| {
            for shortcut in retired.values() {
                if unregister(*shortcut).is_err() {
                    uncertain = true;
                    return Err(());
                }
                observed.remove(&shortcut.id());
            }
            if let Some(new) = desired.filter(|_| !reused) {
                register(new)?;
                observed.insert(new.id(), new);
                added = true;
            }
            if let Some(old) = old.filter(|_| !reused) {
                if unregister(old.shortcut).is_err() {
                    uncertain = true;
                    return Err(());
                }
                observed.remove(&old.shortcut.id());
                removed = true;
            }
            persist()
        })();
        if result.is_err() {
            if added {
                let new = desired.unwrap();
                if unregister(new).is_ok() {
                    observed.remove(&new.id());
                } else {
                    uncertain = true;
                }
            }
            if removed {
                let old = old.unwrap();
                if register(old.shortcut).is_ok() {
                    observed.insert(old.shortcut.id(), old.shortcut);
                } else {
                    uncertain = true;
                }
            }
            if uncertain || !retired.is_empty() {
                let mut state = self.state.lock().unwrap();
                state.active = None;
                state.revision = revision;
                state.retired = observed;
                self.native_id.store(0, Ordering::Release);
                self.pressed.store(false, Ordering::Release);
            }
            return Err(());
        }
        let mut state = self.state.lock().unwrap();
        state.revision = revision;
        state.active = desired.map(|shortcut| Binding { shortcut, revision });
        state.retired.clear();
        self.native_id
            .store(desired.map_or(0, |s| s.id()), Ordering::Release);
        self.pressed.store(false, Ordering::Release);
        Ok(())
    }
}

pub(crate) fn conflicts(settings: &Settings, chord: &str) -> bool {
    builtin_conflicts(
        settings,
        crate::settings::ManagedBuiltinFeature::Screenshot,
        chord,
    )
}

pub(crate) fn builtin_conflicts(
    settings: &Settings,
    feature: crate::settings::ManagedBuiltinFeature,
    chord: &str,
) -> bool {
    settings.translation_selection_hotkey.as_deref().is_some_and(|s| command_chord(s).is_ok_and(|s| s.to_string() == chord))
        || crate::hotkey::HotkeyKind::parse(&settings.hotkey).is_ok_and(|h| h.canonical() == chord)
        || crate::settings::ManagedBuiltinFeature::ALL
            .into_iter()
            .filter(|other| *other != feature)
            .any(|other| {
                settings
                    .builtin_hotkey(other)
                    .is_some_and(|saved| command_chord(saved).is_ok_and(|s| s.to_string() == chord))
            })
        || settings
            .public_plugin_command_hotkeys
            .values()
            .any(|commands| {
                commands
                    .values()
                    .any(|saved| command_chord(saved).is_ok_and(|s| s.to_string() == chord))
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeSet,
    };

    #[test]
    fn builtin_chords_conflict_except_for_their_own_binding() {
        use crate::settings::ManagedBuiltinFeature as F;
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "hotkey": "Alt+Space", "autostart": false, "screenshotHotkey": "Ctrl+S",
            "builtinHotkeys": {"quicklinks": "Ctrl+Q", "webSearch": "Ctrl+W"}
        }))
        .unwrap();
        assert!(!builtin_conflicts(
            &settings,
            F::Quicklinks,
            &command_chord("Ctrl+Q").unwrap().to_string()
        ));
        assert!(builtin_conflicts(
            &settings,
            F::Quicklinks,
            &command_chord("Ctrl+W").unwrap().to_string()
        ));
        assert!(builtin_conflicts(
            &settings,
            F::Screenshot,
            &command_chord("Ctrl+Q").unwrap().to_string()
        ));
        assert!(builtin_conflicts(
            &settings,
            F::WebSearch,
            &command_chord("Ctrl+S").unwrap().to_string()
        ));
    }
    #[test]
    fn screenshot_hotkey_unset_restore_clear_and_repeat() {
        let keys = ScreenshotHotkeys::default();
        assert_eq!(keys.status(None), "unset");
        assert!(keys.registrations().is_empty());
        let a = command_chord("Ctrl+P").unwrap();
        let actual = RefCell::new(BTreeSet::new());
        let mut register = |s: Shortcut| actual.borrow_mut().insert(s.id()).then_some(()).ok_or(());
        let mut unregister =
            |s: Shortcut| actual.borrow_mut().remove(&s.id()).then_some(()).ok_or(());
        keys.edit(Some(a), &mut register, &mut unregister, || Ok(()))
            .unwrap();
        let old = keys.candidate(a.id()).unwrap();
        assert!(keys.accepts_press(a));
        assert!(!keys.accepts_press(a));
        keys.release(a);
        assert!(keys.accepts_press(a));
        keys.edit(None, &mut register, &mut unregister, || Ok(()))
            .unwrap();
        assert!(keys.candidate(old.shortcut.id()).is_none());
        assert!(actual.borrow().is_empty());
    }

    #[test]
    fn screenshot_hotkey_persistence_failure_restores_old_or_disables_all_and_cleans_retired() {
        for fail_restore in [false, true] {
            let keys = ScreenshotHotkeys::default();
            let a = command_chord("Ctrl+P").unwrap();
            let b = command_chord("Ctrl+O").unwrap();
            let actual = RefCell::new(BTreeSet::new());
            let fail = Cell::new(false);
            let mut register = |s: Shortcut| {
                if s == a && fail.replace(false) {
                    return Err(());
                }
                actual.borrow_mut().insert(s.id()).then_some(()).ok_or(())
            };
            let mut unregister =
                |s: Shortcut| actual.borrow_mut().remove(&s.id()).then_some(()).ok_or(());
            keys.edit(Some(a), &mut register, &mut unregister, || Ok(()))
                .unwrap();
            fail.set(fail_restore);
            assert!(keys
                .edit(Some(b), &mut register, &mut unregister, || Err(()))
                .is_err());
            assert_eq!(
                keys.status(Some(&a.to_string())),
                if fail_restore {
                    "unavailable"
                } else {
                    "active"
                }
            );
            assert!(keys.candidate(b.id()).is_none());
            if fail_restore {
                assert!(keys.candidate(a.id()).is_none());
            }
            keys.edit(None, &mut register, &mut unregister, || Ok(()))
                .unwrap();
            assert!(actual.borrow().is_empty());
        }
    }

    #[test]
    fn screenshot_hotkey_failed_rollback_retains_only_possible_native_registrations() {
        let keys = ScreenshotHotkeys::default();
        let a = command_chord("Ctrl+P").unwrap();
        let b = command_chord("Ctrl+O").unwrap();
        keys.edit(Some(a), &mut |_| Ok(()), &mut |_| Ok(()), || Ok(()))
            .unwrap();
        assert!(keys
            .edit(
                Some(b),
                &mut |_| Ok(()),
                &mut |s| if s == b { Err(()) } else { Ok(()) },
                || Err(())
            )
            .is_err());
        assert!(keys.candidate(a.id()).is_none());
        assert!(keys.candidate(b.id()).is_none());
        assert_eq!(keys.registrations().len(), 2);
        let removed = RefCell::new(Vec::new());
        keys.edit(
            None,
            &mut |_| Ok(()),
            &mut |s| {
                removed.borrow_mut().push(s.id());
                Ok(())
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(removed.borrow().len(), 2);
    }
}
