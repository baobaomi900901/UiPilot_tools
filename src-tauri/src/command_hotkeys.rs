//! Host-owned command bindings; native operations are serialized by LifecycleCoordinator.
use crate::{
    hotkey::{command_chord, HotkeyKind},
    settings::Settings,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Mutex};
use tauri_plugin_global_shortcut::Shortcut;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandEntry {
    pub(crate) plugin_id: String,
    pub(crate) command_id: String,
    pub(crate) hotkey: String,
    pub(crate) binding_revision: String,
}
#[derive(Clone, Debug)]
pub(crate) struct NativeBinding {
    pub(crate) shortcut: Shortcut,
    pub(crate) entry: CommandEntry,
    pub(crate) confirmed: bool,
}
#[derive(Debug, Default)]
struct NativeState {
    revision: u64,
    bindings: BTreeMap<u32, NativeBinding>,
}
#[derive(Debug, Default)]
pub(crate) struct CommandHotkeys {
    state: Mutex<NativeState>,
}
impl CommandHotkeys {
    pub(crate) fn revision(&self) -> String {
        self.state.lock().unwrap().revision.to_string()
    }
    pub(crate) fn entries(&self) -> Vec<NativeBinding> {
        self.state
            .lock()
            .unwrap()
            .bindings
            .values()
            .cloned()
            .collect()
    }
    pub(crate) fn candidate(&self, id: u32) -> Option<CommandEntry> {
        self.state
            .lock()
            .unwrap()
            .bindings
            .get(&id)
            .filter(|b| b.confirmed)
            .map(|b| b.entry.clone())
    }
    pub(crate) fn matches(&self, entry: &CommandEntry) -> bool {
        command_chord(&entry.hotkey)
            .ok()
            .and_then(|s| self.candidate(s.id()))
            .as_ref()
            == Some(entry)
    }
    pub(crate) fn status(
        &self,
        plugin: &str,
        command: &str,
        chord: Option<&str>,
        enabled: bool,
    ) -> &'static str {
        let Some(chord) = chord else {
            return "unset";
        };
        let entries = self.entries();
        if entries
            .iter()
            .any(|b| b.entry.plugin_id == plugin && b.entry.command_id == command && !b.confirmed)
        {
            return "unavailable";
        }
        if !enabled {
            return "suspended";
        }
        if entries.iter().any(|b| {
            b.confirmed
                && b.entry.plugin_id == plugin
                && b.entry.command_id == command
                && b.entry.hotkey == chord
        }) {
            "active"
        } else {
            "unavailable"
        }
    }
    // Caller owns the shared operation gate. Never hold the table lock through native or authority work.
    pub(crate) fn edit<R, U, P>(
        &self,
        plugin: &str,
        command: &str,
        replacement: Option<Shortcut>,
        enabled: bool,
        register: &mut R,
        unregister: &mut U,
        persist: P,
    ) -> Result<(), ()>
    where
        R: FnMut(Shortcut) -> Result<(), ()>,
        U: FnMut(Shortcut) -> Result<(), ()>,
        P: FnOnce() -> Result<(), ()>,
    {
        let old = self
            .entries()
            .into_iter()
            .filter(|b| b.entry.plugin_id == plugin && b.entry.command_id == command)
            .collect::<Vec<_>>();
        let revision = self
            .state
            .lock()
            .unwrap()
            .revision
            .checked_add(1)
            .ok_or(())?;
        let desired = replacement.filter(|_| enabled);
        let reused = desired.is_some_and(|s| old.iter().any(|b| b.confirmed && b.shortcut == s));
        let new_binding = desired.map(|shortcut| NativeBinding {
            shortcut,
            confirmed: false,
            entry: CommandEntry {
                plugin_id: plugin.into(),
                command_id: command.into(),
                hotkey: shortcut.to_string(),
                binding_revision: revision.to_string(),
            },
        });
        let mut observed = old
            .iter()
            .cloned()
            .map(|b| (b.shortcut.id(), b))
            .collect::<BTreeMap<_, _>>();
        let mut registered = false;
        let mut removed = Vec::new();
        let mut uncertain = false;
        let result = (|| {
            for binding in old.iter().filter(|b| !b.confirmed) {
                if unregister(binding.shortcut).is_err() {
                    uncertain = true;
                    return Err(());
                }
                observed.remove(&binding.shortcut.id());
            }
            if let Some(shortcut) = desired.filter(|_| !reused) {
                register(shortcut)?;
                registered = true;
                observed.insert(shortcut.id(), new_binding.clone().unwrap());
            }
            for binding in old.iter().filter(|b| b.confirmed) {
                if desired == Some(binding.shortcut) {
                    continue;
                }
                if unregister(binding.shortcut).is_err() {
                    observed.get_mut(&binding.shortcut.id()).unwrap().confirmed = false;
                    uncertain = true;
                    return Err(());
                }
                observed.remove(&binding.shortcut.id());
                removed.push(binding.clone());
            }
            persist()
        })();
        if result.is_err() {
            let mut restored = !uncertain;
            if registered {
                let shortcut = desired.unwrap();
                if unregister(shortcut).is_ok() {
                    observed.remove(&shortcut.id());
                } else {
                    restored = false;
                }
            }
            for binding in removed {
                if register(binding.shortcut).is_ok() {
                    observed.insert(binding.shortcut.id(), binding);
                } else {
                    restored = false;
                }
            }
            // A confirmed removal stays removed, even when another rollback step fails.
            // Only observed or possibly registered IDs remain for retry/exit cleanup.
            if !restored || old.iter().any(|b| !b.confirmed) {
                let mut state = self.state.lock().unwrap();
                state.revision = revision;
                state
                    .bindings
                    .retain(|_, b| b.entry.plugin_id != plugin || b.entry.command_id != command);
                for (_, mut binding) in observed {
                    binding.confirmed = false;
                    state.bindings.insert(binding.shortcut.id(), binding);
                }
            }
            return Err(());
        }
        let mut state = self.state.lock().unwrap();
        state
            .bindings
            .retain(|_, b| b.entry.plugin_id != plugin || b.entry.command_id != command);
        if let Some(shortcut) = desired {
            state.bindings.insert(
                shortcut.id(),
                NativeBinding {
                    shortcut,
                    confirmed: true,
                    entry: CommandEntry {
                        plugin_id: plugin.into(),
                        command_id: command.into(),
                        hotkey: shortcut.to_string(),
                        binding_revision: revision.to_string(),
                    },
                },
            );
        }
        state.revision = revision;
        Ok(())
    }
}

pub(crate) fn saved_matches(settings: &Settings, entry: &CommandEntry) -> bool {
    settings
        .public_plugin_command_hotkeys
        .get(&entry.plugin_id)
        .and_then(|m| m.get(&entry.command_id))
        == Some(&entry.hotkey)
}
pub(crate) fn launcher_conflicts(settings: &Settings, chord: &str) -> bool {
    settings
        .screenshot_hotkey
        .as_deref()
        .is_some_and(|s| HotkeyKind::parse(s).is_ok_and(|h| h.canonical() == chord))
        || settings
            .public_plugin_command_hotkeys
            .values()
            .any(|commands| {
                commands
                    .values()
                    .any(|saved| HotkeyKind::parse(saved).is_ok_and(|h| h.canonical() == chord))
            })
}
pub(crate) fn conflicts(settings: &Settings, plugin: &str, command: &str, chord: &str) -> bool {
    HotkeyKind::parse(&settings.hotkey).is_ok_and(|h| h.canonical() == chord)
        || settings
            .screenshot_hotkey
            .as_deref()
            .is_some_and(|s| HotkeyKind::parse(s).is_ok_and(|h| h.canonical() == chord))
        || settings
            .public_plugin_command_hotkeys
            .iter()
            .any(|(p, commands)| {
                commands.iter().any(|(c, value)| {
                    (p != plugin || c != command)
                        && HotkeyKind::parse(value).is_ok_and(|h| h.canonical() == chord)
                })
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeSet};
    #[test]
    fn screenshot_saved_chord_conflicts_with_launcher_and_plugin() {
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "hotkey": "Alt+Space", "autostart": false, "screenshotHotkey": "Ctrl+P"
        }))
        .unwrap();
        let chord = command_chord("Ctrl+P").unwrap().to_string();
        assert!(launcher_conflicts(&settings, &chord));
        assert!(conflicts(&settings, "com.example.a", "default", &chord));
    }
    #[test]
    fn command_hotkey_uncertain_retry_cleans_real_ids_without_phantom_removed_ids() {
        use std::cell::Cell;
        let registry = CommandHotkeys::default();
        let actual = RefCell::new(BTreeSet::new());
        let fail_restore = Cell::new(false);
        let fail_remove_replacement = Cell::new(false);
        let old = command_chord("Ctrl+N").unwrap();
        let new = command_chord("Ctrl+M").unwrap();
        let mut register = |s: Shortcut| {
            if s == old && fail_restore.replace(false) {
                return Err(());
            }
            if actual.borrow_mut().insert(s.id()) {
                Ok(())
            } else {
                Err(())
            }
        };
        let mut unregister = |s: Shortcut| {
            if s == new && fail_remove_replacement.replace(false) {
                return Err(());
            }
            if actual.borrow_mut().remove(&s.id()) {
                Ok(())
            } else {
                Err(())
            }
        };
        registry
            .edit(
                "com.example.notes",
                "default",
                Some(old),
                true,
                &mut register,
                &mut unregister,
                || Ok(()),
            )
            .unwrap();
        fail_restore.set(true);
        assert!(registry
            .edit(
                "com.example.notes",
                "default",
                Some(new),
                true,
                &mut register,
                &mut unregister,
                || Err(())
            )
            .is_err());
        assert!(actual.borrow().is_empty());
        assert!(
            registry.entries().is_empty(),
            "both confirmed removals must stay absent"
        );
        registry
            .edit(
                "com.example.notes",
                "default",
                Some(old),
                true,
                &mut register,
                &mut unregister,
                || Ok(()),
            )
            .unwrap();
        fail_remove_replacement.set(true);
        assert!(registry
            .edit(
                "com.example.notes",
                "default",
                Some(new),
                true,
                &mut register,
                &mut unregister,
                || Err(())
            )
            .is_err());
        assert_eq!(*actual.borrow(), BTreeSet::from([old.id(), new.id()]));
        assert!(registry.candidate(old.id()).is_none());
        registry
            .edit(
                "com.example.notes",
                "default",
                Some(old),
                true,
                &mut register,
                &mut unregister,
                || Ok(()),
            )
            .unwrap();
        assert_eq!(*actual.borrow(), BTreeSet::from([old.id()]));
        assert!(registry.candidate(old.id()).is_some());
    }

    #[test]
    fn command_hotkey_conflicts_include_launcher_aliases_and_all_suspended_preferences() {
        let mut settings = Settings::default();
        settings.hotkey = "Control+N".into();
        assert!(conflicts(
            &settings,
            "com.example.a",
            "default",
            &command_chord("Ctrl+N").unwrap().to_string()
        ));
        settings.public_plugin_command_hotkeys.insert(
            "com.example.suspended".into(),
            BTreeMap::from([("default".into(), "Control+M".into())]),
        );
        assert!(conflicts(
            &settings,
            "com.example.a",
            "default",
            &command_chord("Ctrl+M").unwrap().to_string()
        ));
        assert!(!conflicts(
            &settings,
            "com.example.suspended",
            "default",
            &command_chord("Ctrl+M").unwrap().to_string()
        ));
    }

    #[test]
    fn command_hotkey_bad_registration_does_not_block_other_chords_or_old_binding() {
        let registry = CommandHotkeys::default();
        let old = command_chord("Ctrl+N").unwrap();
        registry
            .edit(
                "com.example.a",
                "default",
                Some(old),
                true,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
                || Ok(()),
            )
            .unwrap();
        let revision = registry.revision();
        assert!(registry
            .edit(
                "com.example.b",
                "default",
                Some(command_chord("Ctrl+M").unwrap()),
                true,
                &mut |_| Err(()),
                &mut |_| Ok(()),
                || panic!("registration failed before persistence")
            )
            .is_err());
        assert_eq!(registry.revision(), revision);
        assert!(registry.candidate(old.id()).is_some());
        registry
            .edit(
                "com.example.c",
                "default",
                Some(command_chord("Ctrl+K").unwrap()),
                true,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
                || Ok(()),
            )
            .unwrap();
        assert_eq!(registry.entries().len(), 2);
    }

    #[test]
    fn command_hotkey_transaction_restores_old_native_on_failed_persistence() {
        let registry = CommandHotkeys::default();
        let actual = RefCell::new(BTreeSet::new());
        let mut register = |s: Shortcut| {
            actual.borrow_mut().insert(s.id());
            Ok(())
        };
        let mut unregister = |s: Shortcut| {
            actual.borrow_mut().remove(&s.id());
            Ok(())
        };
        let old = command_chord("Ctrl+N").unwrap();
        registry
            .edit(
                "com.example.notes",
                "default",
                Some(old),
                true,
                &mut register,
                &mut unregister,
                || Ok(()),
            )
            .unwrap();
        let before = registry.candidate(old.id()).unwrap();
        assert!(registry
            .edit(
                "com.example.notes",
                "default",
                Some(command_chord("Ctrl+M").unwrap()),
                true,
                &mut register,
                &mut unregister,
                || Err(())
            )
            .is_err());
        assert_eq!(registry.candidate(old.id()), Some(before));
        assert_eq!(*actual.borrow(), BTreeSet::from([old.id()]));
    }
    #[test]
    fn command_hotkey_suspended_edit_clear_and_uncertain_rollback() {
        let registry = CommandHotkeys::default();
        let old = command_chord("Ctrl+N").unwrap();
        registry
            .edit(
                "com.example.notes",
                "default",
                Some(old),
                false,
                &mut |_| panic!("suspended"),
                &mut |_| Ok(()),
                || Ok(()),
            )
            .unwrap();
        assert!(registry.candidate(old.id()).is_none());
        registry
            .edit(
                "com.example.notes",
                "default",
                Some(old),
                true,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
                || Ok(()),
            )
            .unwrap();
        let new = command_chord("Ctrl+M").unwrap();
        assert!(registry
            .edit(
                "com.example.notes",
                "default",
                Some(new),
                true,
                &mut |s| if s == old { Err(()) } else { Ok(()) },
                &mut |_| Ok(()),
                || Err(())
            )
            .is_err());
        assert!(registry.candidate(old.id()).is_none());
        assert!(registry.candidate(new.id()).is_none());
        assert_eq!(
            registry.status("com.example.notes", "default", Some(&old.to_string()), true),
            "unavailable"
        );
        registry
            .edit(
                "com.example.notes",
                "default",
                None,
                true,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
                || Ok(()),
            )
            .unwrap();
        assert!(registry.entries().is_empty());
    }
}
