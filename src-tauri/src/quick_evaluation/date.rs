use time::{Date, Duration, Month, Weekday};

use super::{
    normalize_query,
    number::{canonicalize_number, NumberGrammar},
    QuickClassification, QuickFailure, QuickIntent, QuickKind, QuickOutput,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DateIntent {
    base: DateBase,
    operation: DateOperation,
    quantity: u64,
    unit: DateUnit,
    normalized_expression: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DateBase {
    Today,
    Explicit(Date),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DateOperation {
    Add,
    Subtract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DateUnit {
    Day,
    Week,
    Month,
    Year,
}

pub(crate) fn classify(source: &str) -> QuickClassification {
    let Some(normalized) = normalize_query(source) else {
        return QuickClassification::NoMatch;
    };
    let parts = normalized.split(' ').collect::<Vec<_>>();
    let Some(base_source) = parts.first().copied() else {
        return QuickClassification::NoMatch;
    };
    let is_today = base_source.eq_ignore_ascii_case("today");
    if !is_today && !looks_like_date_candidate(base_source) {
        return QuickClassification::NoMatch;
    }
    if parts.len() < 4 {
        return QuickClassification::NoMatch;
    }
    let operation = match parts[1] {
        "+" => DateOperation::Add,
        "-" => DateOperation::Subtract,
        _ => return QuickClassification::NoMatch,
    };
    if parts.len() != 4 {
        return QuickClassification::Invalid(invalid_syntax());
    }

    let base = if is_today {
        DateBase::Today
    } else {
        let Some(date) = parse_iso_date(base_source) else {
            return QuickClassification::Invalid(invalid_date());
        };
        DateBase::Explicit(date)
    };
    let Some(canonical_quantity) = canonicalize_number(parts[2], NumberGrammar::NonNegativeInteger)
    else {
        return QuickClassification::Invalid(invalid_quantity());
    };
    let Ok(quantity) = canonical_quantity.parse::<u64>() else {
        return QuickClassification::Invalid(invalid_quantity());
    };
    let unit_source = parts[3].to_ascii_lowercase();
    let unit = match unit_source.as_str() {
        "day" | "days" => DateUnit::Day,
        "week" | "weeks" => DateUnit::Week,
        "month" | "months" => DateUnit::Month,
        "year" | "years" => DateUnit::Year,
        _ => return QuickClassification::Invalid(invalid_syntax()),
    };
    let normalized_base = if is_today { "today" } else { base_source };

    QuickClassification::Ready(QuickIntent::Date(DateIntent {
        base,
        operation,
        quantity,
        unit,
        normalized_expression: format!(
            "{normalized_base} {} {canonical_quantity} {unit_source}",
            parts[1]
        ),
    }))
}

pub(crate) fn evaluate(intent: DateIntent, today: Date) -> Result<QuickOutput, QuickFailure> {
    let base = match intent.base {
        DateBase::Today => today,
        DateBase::Explicit(date) => date,
    };
    if !(1..=9999).contains(&base.year()) {
        return Err(out_of_range());
    }

    let result = match intent.unit {
        DateUnit::Day => checked_days(base, intent.operation, intent.quantity),
        DateUnit::Week => intent
            .quantity
            .checked_mul(7)
            .and_then(|days| checked_days(base, intent.operation, days)),
        DateUnit::Month => checked_months(base, intent.operation, intent.quantity),
        DateUnit::Year => checked_years(base, intent.operation, intent.quantity),
    }
    .filter(|date| (1..=9999).contains(&date.year()))
    .ok_or_else(out_of_range)?;

    let title = format_iso_date(result);
    Ok(QuickOutput {
        kind: QuickKind::Date,
        subtitle: Some(format!(
            "{} · {}",
            english_weekday(result.weekday()),
            intent.normalized_expression
        )),
        copy_text: title.clone(),
        title,
        refresh_at_utc: None,
    })
}

fn looks_like_date_candidate(source: &str) -> bool {
    let mut parts = source.split('-');
    let candidate = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.as_bytes().iter().all(u8::is_ascii_digit))
    });
    candidate && parts.next().is_none()
}

fn looks_like_iso_date(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn parse_iso_date(source: &str) -> Option<Date> {
    if !looks_like_iso_date(source) {
        return None;
    }
    let year = source[0..4].parse::<i32>().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    let month = Month::try_from(source[5..7].parse::<u8>().ok()?).ok()?;
    let day = source[8..10].parse::<u8>().ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

fn checked_days(base: Date, operation: DateOperation, quantity: u64) -> Option<Date> {
    let seconds = quantity.checked_mul(86_400)?;
    let duration = Duration::seconds(i64::try_from(seconds).ok()?);
    match operation {
        DateOperation::Add => base.checked_add(duration),
        DateOperation::Subtract => base.checked_sub(duration),
    }
}

fn checked_months(base: Date, operation: DateOperation, quantity: u64) -> Option<Date> {
    let base_month = (base.year() as i128 - 1)
        .checked_mul(12)?
        .checked_add(base.month() as i128 - 1)?;
    let quantity = i128::from(quantity);
    let target_month = match operation {
        DateOperation::Add => base_month.checked_add(quantity)?,
        DateOperation::Subtract => base_month.checked_sub(quantity)?,
    };
    const LAST_SUPPORTED_MONTH: i128 = 9999 * 12 - 1;
    if !(0..=LAST_SUPPORTED_MONTH).contains(&target_month) {
        return None;
    }

    let year = i32::try_from(target_month / 12 + 1).ok()?;
    let month = Month::try_from(u8::try_from(target_month % 12 + 1).ok()?).ok()?;
    clamped_date(year, month, base.day())
}

fn checked_years(base: Date, operation: DateOperation, quantity: u64) -> Option<Date> {
    let quantity = i128::from(quantity);
    let target_year = match operation {
        DateOperation::Add => i128::from(base.year()).checked_add(quantity)?,
        DateOperation::Subtract => i128::from(base.year()).checked_sub(quantity)?,
    };
    if !(1..=9999).contains(&target_year) {
        return None;
    }
    clamped_date(i32::try_from(target_year).ok()?, base.month(), base.day())
}

fn clamped_date(year: i32, month: Month, preferred_day: u8) -> Option<Date> {
    Date::from_calendar_date(year, month, preferred_day.min(month.length(year))).ok()
}

fn format_iso_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

fn english_weekday(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Monday => "Monday",
        Weekday::Tuesday => "Tuesday",
        Weekday::Wednesday => "Wednesday",
        Weekday::Thursday => "Thursday",
        Weekday::Friday => "Friday",
        Weekday::Saturday => "Saturday",
        Weekday::Sunday => "Sunday",
    }
}

fn invalid_syntax() -> QuickFailure {
    QuickFailure {
        kind: QuickKind::Date,
        title: "日期表达式无效".into(),
        explanation: "请使用 today 或 YYYY-MM-DD 加减非负整数日期单位".into(),
    }
}

fn invalid_date() -> QuickFailure {
    QuickFailure {
        kind: QuickKind::Date,
        title: "日期无效".into(),
        explanation: "请输入 0001-01-01 至 9999-12-31 之间存在的日期".into(),
    }
}

fn invalid_quantity() -> QuickFailure {
    QuickFailure {
        kind: QuickKind::Date,
        title: "日期数量无效".into(),
        explanation: "日期数量必须是不带正负号或小数的非负整数".into(),
    }
}

fn out_of_range() -> QuickFailure {
    QuickFailure {
        kind: QuickKind::Date,
        title: "日期结果超出范围".into(),
        explanation: "结果必须位于 0001-01-01 至 9999-12-31 之间".into(),
    }
}

#[cfg(test)]
mod tests {
    use time::{Date, Month};

    use super::{classify, evaluate};
    use crate::quick_evaluation::{QuickClassification, QuickIntent, QuickKind};

    fn date(year: i32, month: Month, day: u8) -> Date {
        Date::from_calendar_date(year, month, day).unwrap()
    }

    fn evaluate_ready(source: &str, today: Date) -> crate::quick_evaluation::QuickOutput {
        let QuickClassification::Ready(QuickIntent::Date(intent)) = classify(source) else {
            panic!("expected ready date intent for {source}");
        };
        evaluate(intent, today).unwrap()
    }

    fn assert_invalid(source: &str) {
        let QuickClassification::Invalid(failure) = classify(source) else {
            panic!("expected invalid date classification for {source}");
        };
        assert_eq!(failure.kind, QuickKind::Date, "source={source}");
        assert!(!failure.title.is_empty(), "source={source}");
        assert!(!failure.explanation.is_empty(), "source={source}");
    }

    #[test]
    fn incomplete_or_unowned_date_shapes_do_not_match() {
        for source in [
            "",
            "today",
            "today +",
            "today + 1",
            "2026-02-30 + 1",
            "tomorrow + 1 day",
            "today+1 day",
            "today * 1 day",
            "today top news from london",
            "2026-09-03 release notes from team",
        ] {
            assert!(
                matches!(classify(source), QuickClassification::NoMatch),
                "source={source}"
            );
        }
        assert!(matches!(
            classify(&format!("today + 1 day {}", "x".repeat(401))),
            QuickClassification::NoMatch
        ));
    }

    #[test]
    fn complete_date_signatures_report_typed_invalid_failures() {
        for source in [
            "today + 1.5 days",
            "today + -1 days",
            "today + +1 days",
            "today + 1 fortnight",
            "today + 1 day extra",
            "2026-9-03 + 1 day",
            "2026-02-30 + 1 day",
            "2025-02-29 + 1 year",
            "0000-01-01 + 1 day",
            "today + 18446744073709551616 days",
        ] {
            assert_invalid(source);
        }
    }

    #[test]
    fn recognizes_every_approved_date_unit_form() {
        let today = date(2026, Month::September, 3);
        for source in [
            "today + 1 day",
            "today + 2 days",
            "today + 1 week",
            "today + 2 weeks",
            "today + 1 month",
            "today + 2 months",
            "today + 1 year",
            "today + 2 years",
        ] {
            let output = evaluate_ready(source, today);
            assert_eq!(output.kind, QuickKind::Date, "source={source}");
            assert_eq!(output.copy_text, output.title, "source={source}");
        }
    }

    #[test]
    fn formats_exact_iso_weekday_normalized_expression_and_copy_text() {
        let output = evaluate_ready("  TODAY   +   7   DAYS  ", date(2026, Month::September, 3));

        assert_eq!(output.kind, QuickKind::Date);
        assert_eq!(output.title, "2026-09-10");
        assert_eq!(
            output.subtitle.as_deref(),
            Some("Thursday · today + 7 days")
        );
        assert_eq!(output.copy_text, "2026-09-10");
        assert_eq!(output.refresh_at_utc, None);
    }

    #[test]
    fn uses_injected_today_and_canonicalizes_grouped_quantities() {
        let output = evaluate_ready("today + 1,000 days", date(2024, Month::January, 1));

        assert_eq!(output.title, "2026-09-27");
        assert_eq!(
            output.subtitle.as_deref(),
            Some("Sunday · today + 1000 days")
        );
    }

    #[test]
    fn checked_day_and_week_arithmetic_crosses_month_and_year() {
        assert_eq!(
            evaluate_ready("2026-01-31 + 1 day", date(2000, Month::January, 1)).title,
            "2026-02-01"
        );
        assert_eq!(
            evaluate_ready("2025-12-29 + 1 week", date(2000, Month::January, 1)).title,
            "2026-01-05"
        );
        assert_eq!(
            evaluate_ready("2026-01-01 - 1 day", date(2000, Month::January, 1)).title,
            "2025-12-31"
        );
    }

    #[test]
    fn month_and_year_arithmetic_clamps_to_the_target_month_end() {
        for (source, expected) in [
            ("2026-01-31 + 1 month", "2026-02-28"),
            ("2024-01-31 + 1 month", "2024-02-29"),
            ("2026-03-31 - 1 month", "2026-02-28"),
            ("2024-02-29 + 1 year", "2025-02-28"),
            ("2024-02-29 - 1 year", "2023-02-28"),
        ] {
            assert_eq!(
                evaluate_ready(source, date(2000, Month::January, 1)).title,
                expected,
                "source={source}"
            );
        }
    }

    #[test]
    fn checked_arithmetic_rejects_overflow_and_out_of_range_results() {
        for source in [
            "9999-12-31 + 1 day",
            "0001-01-01 - 1 day",
            "9999-12-31 + 1 month",
            "0001-01-01 - 1 year",
            "today + 18446744073709551615 weeks",
        ] {
            let QuickClassification::Ready(QuickIntent::Date(intent)) = classify(source) else {
                panic!("expected ready date intent for {source}");
            };
            let failure = evaluate(intent, date(2026, Month::September, 3)).unwrap_err();
            assert_eq!(failure.kind, QuickKind::Date, "source={source}");
            assert!(!failure.explanation.is_empty(), "source={source}");
        }
    }
}
