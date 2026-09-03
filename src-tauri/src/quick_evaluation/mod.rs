mod date;
mod number;

use time::OffsetDateTime;

const MAX_QUERY_BYTES: usize = 400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickKind {
    Date,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickClassification {
    NoMatch,
    Invalid(QuickFailure),
    Ready(QuickIntent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickIntent {
    Date(date::DateIntent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuickFailure {
    pub(crate) kind: QuickKind,
    pub(crate) title: String,
    pub(crate) explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuickOutput {
    pub(crate) kind: QuickKind,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) copy_text: String,
    pub(crate) refresh_at_utc: Option<OffsetDateTime>,
}

fn normalize_query(source: &str) -> Option<String> {
    let source = source.trim();
    if source.len() > MAX_QUERY_BYTES {
        return None;
    }

    let mut normalized = String::with_capacity(source.len());
    let mut previous_was_space = false;
    for character in source.chars() {
        if character == ' ' {
            if !previous_was_space {
                normalized.push(character);
            }
            previous_was_space = true;
        } else {
            normalized.push(character);
            previous_was_space = false;
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::{normalize_query, QuickClassification, QuickFailure, QuickKind, QuickOutput};

    #[test]
    fn shared_contract_keeps_presentation_data_independent_of_launcher_dtos() {
        let output = QuickOutput {
            kind: QuickKind::Date,
            title: "2026-09-10".into(),
            subtitle: Some("Thursday · today + 7 days".into()),
            copy_text: "2026-09-10".into(),
            refresh_at_utc: None,
        };
        assert_eq!(output.kind, QuickKind::Date);
        assert_eq!(output.copy_text, output.title);
        assert_eq!(output.refresh_at_utc, None);

        let failure = QuickFailure {
            kind: QuickKind::Date,
            title: "日期表达式无效".into(),
            explanation: "请检查日期和数量".into(),
        };
        assert!(matches!(
            QuickClassification::Invalid(failure.clone()),
            QuickClassification::Invalid(actual) if actual == failure
        ));
    }

    #[test]
    fn normalization_outer_trims_and_collapses_only_ascii_spaces() {
        assert_eq!(
            normalize_query("  TODAY   +   7   DAYS  ").as_deref(),
            Some("TODAY + 7 DAYS")
        );
        assert_eq!(
            normalize_query("today\t+\t7\tdays").as_deref(),
            Some("today\t+\t7\tdays")
        );
        assert_eq!(
            normalize_query(&format!(
                "{}today + 7 days{}",
                " ".repeat(401),
                " ".repeat(401)
            ))
            .as_deref(),
            Some("today + 7 days")
        );
        assert_eq!(normalize_query(&"0".repeat(401)), None);
    }
}
