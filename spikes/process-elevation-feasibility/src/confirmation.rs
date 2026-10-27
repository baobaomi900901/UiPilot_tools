#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Pending,
    Cancelled,
    ConfirmedSimulation,
    TimedOut,
}
pub(super) struct Model {
    pub rows: Vec<String>,
    pub outcome: Outcome,
}

pub(super) fn display_text(raw: &str) -> String {
    raw.chars().map(|ch| {
        if ch.is_control() || matches!(ch, '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            format!("\\u{{{:x}}}", ch as u32)
        } else { ch.to_string() }
    }).collect()
}
impl Model {
    pub fn new() -> Self {
        let mut rows: Vec<String> = (1..=256)
            .map(|i| {
                format!(
                    "模拟目标 {i:03} — PID {} — C:\\模拟目录\\fixture-{i:03}.exe",
                    50000 + i
                )
            })
            .collect();
        rows[254].push_str(&display_text(" — 控制字符示例:\n\u{202e}"));
        rows[255].push_str(&format!(
            " — 长路径示例:C:\\{}end.exe",
            "长目录名\\".repeat(70)
        ));
        Self {
            rows,
            outcome: Outcome::Pending,
        }
    }
    pub fn finish(&mut self, outcome: Outcome) {
        if self.outcome == Outcome::Pending && outcome != Outcome::Pending {
            self.outcome = outcome;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_frozen_selection_has_256_rows() {
        let model = Model::new();
        assert_eq!(model.rows.len(), 256);
        assert!(model.rows[0].contains("001"));
        assert!(model.rows[255].contains("256"));
        assert_eq!(model.outcome, Outcome::Pending);
    }
    #[test]
    fn control_and_bidi_characters_are_visible_not_active() {
        assert_eq!(display_text("a\n\0\u{202e}z"), "a\\u{a}\\u{0}\\u{202e}z");
        assert_eq!(display_text("中文.exe"), "中文.exe");
    }
    #[test]
    fn terminal_decision_cannot_be_changed() {
        for first in [
            Outcome::Cancelled,
            Outcome::ConfirmedSimulation,
            Outcome::TimedOut,
        ] {
            let mut model = Model::new();
            model.finish(first);
            model.finish(Outcome::ConfirmedSimulation);
            model.finish(Outcome::Pending);
            assert_eq!(model.outcome, first);
        }
    }
}
