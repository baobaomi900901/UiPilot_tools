use std::time::Duration;

use tauri_plugin_global_shortcut::Shortcut;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DoubleTapModifier {
    Ctrl,
    Alt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HotkeyKind {
    Chord(Shortcut),
    DoubleTap(DoubleTapModifier),
}

pub(crate) const DOUBLE_CTRL: &str = "DoubleCtrl";
pub(crate) const DOUBLE_ALT: &str = "DoubleAlt";
pub(crate) const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);

impl HotkeyKind {
    pub(crate) fn parse(raw: &str) -> Result<Self, ()> {
        match raw {
            DOUBLE_CTRL => Ok(HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl)),
            DOUBLE_ALT => Ok(HotkeyKind::DoubleTap(DoubleTapModifier::Alt)),
            _ => raw
                .parse::<Shortcut>()
                .map(HotkeyKind::Chord)
                .map_err(|_| ()),
        }
    }

    pub(crate) fn canonical(&self) -> String {
        match self {
            HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl) => DOUBLE_CTRL.to_string(),
            HotkeyKind::DoubleTap(DoubleTapModifier::Alt) => DOUBLE_ALT.to_string(),
            HotkeyKind::Chord(shortcut) => shortcut.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DoubleTapModifier, HotkeyKind, DOUBLE_ALT, DOUBLE_CTRL};

    #[test]
    fn parses_double_tap_exact_and_rejects_aliases() {
        assert_eq!(
            HotkeyKind::parse(DOUBLE_CTRL),
            Ok(HotkeyKind::DoubleTap(DoubleTapModifier::Ctrl))
        );
        assert_eq!(
            HotkeyKind::parse(DOUBLE_ALT),
            Ok(HotkeyKind::DoubleTap(DoubleTapModifier::Alt))
        );
        for rejected in ["doublectrl", "Double Ctrl", "double-ctrl", "DOUBLECTRL", ""] {
            assert_eq!(HotkeyKind::parse(rejected), Err(()));
        }
    }

    #[test]
    fn parses_chord_and_canonicalizes_via_shortcut() {
        let kind = HotkeyKind::parse("Ctrl+Space").unwrap();
        match &kind {
            HotkeyKind::Chord(shortcut) => {
                assert_eq!(kind.canonical(), shortcut.to_string());
            }
            _ => panic!("expected chord"),
        }
        assert!(HotkeyKind::parse("not a shortcut").is_err());
    }
}
