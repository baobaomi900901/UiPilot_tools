mod arithmetic;
#[allow(dead_code)] // Currency is intentionally registered with the async dispatcher in task C5.
mod currency;
mod date;
mod number;
mod time_zone;
mod units;

use time::OffsetDateTime;

const MAX_QUERY_BYTES: usize = 400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickKind {
    Unit,
    Arithmetic,
    Date,
    TimeZone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickClassification {
    NoMatch,
    Invalid(QuickFailure),
    Ready(QuickIntent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickIntent {
    Unit(units::UnitIntent),
    Arithmetic(arithmetic::ArithmeticIntent),
    Date(date::DateIntent),
    TimeZone(time_zone::TimeZoneIntent),
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

pub(crate) fn classify(source: &str) -> QuickClassification {
    let date = date::classify(source);
    if !matches!(date, QuickClassification::NoMatch) {
        return date;
    }
    match time_zone::classify(source) {
        time_zone::TimeZoneClassification::NoMatch => {}
        time_zone::TimeZoneClassification::Invalid(invalid) => {
            let time_zone::TimeZoneInvalid {
                reason: _reason,
                title,
                explanation,
            } = invalid;
            return QuickClassification::Invalid(QuickFailure {
                kind: QuickKind::TimeZone,
                title,
                explanation,
            });
        }
        time_zone::TimeZoneClassification::Ready(intent) => {
            return QuickClassification::Ready(QuickIntent::TimeZone(intent));
        }
    }
    match units::classify(source) {
        units::UnitClassification::NoMatch => {}
        units::UnitClassification::Invalid(reason) => {
            let invalid = units::failure(reason);
            return QuickClassification::Invalid(QuickFailure {
                kind: QuickKind::Unit,
                title: invalid.title,
                explanation: invalid.explanation,
            });
        }
        units::UnitClassification::Ready(intent) => {
            return QuickClassification::Ready(QuickIntent::Unit(intent));
        }
    }
    arithmetic::classify(source).map_or(QuickClassification::NoMatch, |intent| {
        QuickClassification::Ready(QuickIntent::Arithmetic(intent))
    })
}

pub(crate) fn evaluate<L, U>(
    intent: QuickIntent,
    local_today: L,
    utc_now: U,
) -> Result<QuickOutput, QuickFailure>
where
    L: FnOnce() -> Result<time::Date, ()>,
    U: FnOnce() -> Result<OffsetDateTime, ()>,
{
    match intent {
        QuickIntent::Unit(intent) => units::evaluate(intent)
            .map(|output| QuickOutput {
                kind: QuickKind::Unit,
                title: output.title,
                subtitle: output.subtitle,
                copy_text: output.copy_text,
                refresh_at_utc: output.refresh_at_utc,
            })
            .map_err(|invalid| QuickFailure {
                kind: QuickKind::Unit,
                title: invalid.title,
                explanation: invalid.explanation,
            }),
        QuickIntent::Arithmetic(intent) => Ok(arithmetic::output(intent)),
        QuickIntent::Date(intent) => {
            let today = local_today().map_err(|()| local_date_unavailable())?;
            date::evaluate(intent, today)
        }
        QuickIntent::TimeZone(intent) => {
            let utc_now = utc_now().map_err(|()| {
                let time_zone::TimeZoneInvalid {
                    reason: _reason,
                    title,
                    explanation,
                } = time_zone::local_clock_unavailable();
                QuickFailure {
                    kind: QuickKind::TimeZone,
                    title,
                    explanation,
                }
            })?;
            time_zone::evaluate(intent, utc_now)
                .map(|output| QuickOutput {
                    kind: QuickKind::TimeZone,
                    title: output.title,
                    subtitle: output.subtitle,
                    copy_text: output.copy_text,
                    refresh_at_utc: output.refresh_at_utc,
                })
                .map_err(|invalid| {
                    let time_zone::TimeZoneInvalid {
                        reason: _reason,
                        title,
                        explanation,
                    } = invalid;
                    QuickFailure {
                        kind: QuickKind::TimeZone,
                        title,
                        explanation,
                    }
                })
        }
    }
}

fn local_date_unavailable() -> QuickFailure {
    QuickFailure {
        kind: QuickKind::Date,
        title: "无法读取本地日期".into(),
        explanation: "请检查 Windows 日期和时区设置".into(),
    }
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
    use std::cell::Cell;

    use time::{Date, Month};

    use super::{
        classify, evaluate, normalize_query, QuickClassification, QuickFailure, QuickIntent,
        QuickKind, QuickOutput,
    };

    #[test]
    fn dispatcher_keeps_date_ahead_of_legacy_arithmetic() {
        assert!(matches!(
            classify("2026-08-16 + 1 day"),
            QuickClassification::Ready(QuickIntent::Date(_))
        ));
        assert!(matches!(
            classify("2026-08-16"),
            QuickClassification::Ready(QuickIntent::Arithmetic(_))
        ));
        assert!(matches!(classify("1+"), QuickClassification::NoMatch));
    }

    #[test]
    fn dispatcher_orders_date_then_time_zone_then_unit_then_arithmetic() {
        assert!(matches!(
            classify("2026-08-16 + 1 day"),
            QuickClassification::Ready(QuickIntent::Date(_))
        ));
        assert!(matches!(
            classify("New York time"),
            QuickClassification::Ready(QuickIntent::TimeZone(_))
        ));
        assert!(matches!(
            classify("1000 mm to in"),
            QuickClassification::Ready(QuickIntent::Unit(_))
        ));
        assert!(matches!(
            classify(".5*8"),
            QuickClassification::Ready(QuickIntent::Arithmetic(_))
        ));
    }

    #[test]
    fn time_zone_evaluation_reads_one_injected_utc_instant_and_not_the_local_date() {
        let calls = Cell::new(0);
        let QuickClassification::Ready(intent) = classify("Mumbai time") else {
            panic!("expected time-zone intent");
        };
        let utc_now = Date::from_calendar_date(2026, Month::January, 15)
            .unwrap()
            .with_hms(5, 5, 20)
            .unwrap()
            .assume_utc();

        let output = evaluate(
            intent,
            || panic!("time-zone evaluation must not read the local date"),
            || {
                calls.set(calls.get() + 1);
                Ok(utc_now)
            },
        )
        .unwrap();

        assert_eq!(calls.get(), 1);
        assert_eq!(output.kind, QuickKind::TimeZone);
        assert_eq!(output.title, "10:35");
        assert_eq!(output.copy_text, output.title);
        assert_eq!(
            output.subtitle.as_deref(),
            Some("2026-01-15 · Mumbai · UTC+05:30 (IST)")
        );
        assert_eq!(
            output.refresh_at_utc,
            Some(
                Date::from_calendar_date(2026, Month::January, 15)
                    .unwrap()
                    .with_hms(5, 6, 0)
                    .unwrap()
                    .assume_utc()
            )
        );
    }

    #[test]
    fn dispatcher_routes_units_immediately_before_legacy_arithmetic() {
        let QuickClassification::Ready(intent) = classify("1000 mm to in") else {
            panic!("expected unit intent");
        };
        let output = evaluate(
            intent,
            || panic!("units must not read the local date"),
            || panic!("units must not read the UTC clock"),
        )
        .unwrap();

        assert_eq!(output.kind, QuickKind::Unit);
        assert_eq!(output.title, "39.3701 in");
        assert_eq!(output.copy_text, "39.3701 in");
    }

    #[test]
    fn dispatcher_reads_local_date_once_only_for_date_intents() {
        let arithmetic_calls = Cell::new(0);
        let QuickClassification::Ready(arithmetic) = classify(".5*8") else {
            panic!("expected arithmetic intent");
        };
        let arithmetic_output = evaluate(
            arithmetic,
            || {
                arithmetic_calls.set(arithmetic_calls.get() + 1);
                Err(())
            },
            || panic!("arithmetic must not read the UTC clock"),
        )
        .unwrap();
        assert_eq!(arithmetic_output.kind, QuickKind::Arithmetic);
        assert_eq!(arithmetic_output.title, "4");
        assert_eq!(arithmetic_calls.get(), 0);

        let date_calls = Cell::new(0);
        let QuickClassification::Ready(date) = classify("today + 1 day") else {
            panic!("expected date intent");
        };
        let date_output = evaluate(
            date,
            || {
                date_calls.set(date_calls.get() + 1);
                Date::from_calendar_date(2026, Month::September, 3).map_err(|_| ())
            },
            || panic!("date arithmetic must not read the UTC clock"),
        )
        .unwrap();
        assert_eq!(date_output.title, "2026-09-04");
        assert_eq!(date_calls.get(), 1);
    }

    #[test]
    fn dispatcher_reports_local_date_failure_without_a_utc_fallback() {
        let QuickClassification::Ready(intent) = classify("today + 1 day") else {
            panic!("expected date intent");
        };
        let failure = evaluate(
            intent,
            || Err(()),
            || panic!("date arithmetic must not read the UTC clock"),
        )
        .unwrap_err();

        assert_eq!(failure.kind, QuickKind::Date);
        assert_eq!(failure.title, "无法读取本地日期");
        assert_eq!(failure.explanation, "请检查 Windows 日期和时区设置");
    }

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
