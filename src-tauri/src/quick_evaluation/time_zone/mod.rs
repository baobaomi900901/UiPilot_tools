mod aliases;

use chrono::{DateTime, Offset, Utc};
use chrono_tz::{OffsetName, Tz};
use time::OffsetDateTime;

use self::aliases::resolve_alias;
use super::normalize_query;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TimeZoneFailureKind {
    InvalidTimeZone,
    ClockUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimeZoneInvalid {
    pub(crate) reason: TimeZoneFailureKind,
    pub(crate) title: String,
    pub(crate) explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TimeZoneClassification {
    NoMatch,
    Invalid(TimeZoneInvalid),
    Ready(TimeZoneIntent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimeZoneIntent {
    zone: Tz,
    display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TimeZoneOutput {
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) copy_text: String,
    pub(crate) refresh_at_utc: Option<OffsetDateTime>,
}

pub(crate) fn classify(source: &str) -> TimeZoneClassification {
    let Some(normalized) = normalize_query(source) else {
        return TimeZoneClassification::NoMatch;
    };
    let Some((zone_source, keyword)) = normalized.rsplit_once(' ') else {
        return TimeZoneClassification::NoMatch;
    };
    if zone_source.is_empty() || !keyword.eq_ignore_ascii_case("time") {
        return TimeZoneClassification::NoMatch;
    }

    if let Some(alias) = resolve_alias(zone_source) {
        return TimeZoneClassification::Ready(TimeZoneIntent {
            zone: alias.zone,
            display_name: alias.display_name.into(),
        });
    }
    if ["CST", "EST", "IST"]
        .iter()
        .any(|ambiguous| zone_source.eq_ignore_ascii_case(ambiguous))
    {
        return TimeZoneClassification::Invalid(invalid_time_zone());
    }
    match Tz::from_str_insensitive(zone_source) {
        Ok(zone) => TimeZoneClassification::Ready(TimeZoneIntent {
            display_name: zone.name().into(),
            zone,
        }),
        Err(_) => TimeZoneClassification::Invalid(invalid_time_zone()),
    }
}

pub(crate) fn evaluate(
    intent: TimeZoneIntent,
    utc_now: OffsetDateTime,
) -> Result<TimeZoneOutput, TimeZoneInvalid> {
    let utc = DateTime::<Utc>::from_timestamp(utc_now.unix_timestamp(), utc_now.nanosecond())
        .ok_or_else(local_clock_unavailable)?;
    let local = utc.with_timezone(&intent.zone);
    let offset_seconds = local.offset().fix().local_minus_utc();
    let offset_sign = if offset_seconds < 0 { '−' } else { '+' };
    let offset_magnitude = offset_seconds.unsigned_abs();
    let offset_hours = offset_magnitude / 3_600;
    let offset_minutes = offset_magnitude % 3_600 / 60;
    let abbreviation = local
        .offset()
        .abbreviation()
        .ok_or_else(local_clock_unavailable)?;
    let title = local.format("%H:%M").to_string();
    let subtitle = format!(
        "{} · {} · UTC{offset_sign}{offset_hours:02}:{offset_minutes:02} ({})",
        local.format("%Y-%m-%d"),
        intent.display_name,
        abbreviation
    );
    let refresh_at_utc = next_utc_minute(utc_now).ok_or_else(local_clock_unavailable)?;

    Ok(TimeZoneOutput {
        subtitle: Some(subtitle),
        copy_text: title.clone(),
        title,
        refresh_at_utc: Some(refresh_at_utc),
    })
}

fn next_utc_minute(utc_now: OffsetDateTime) -> Option<OffsetDateTime> {
    utc_now
        .unix_timestamp()
        .div_euclid(60)
        .checked_add(1)?
        .checked_mul(60)
        .and_then(|timestamp| OffsetDateTime::from_unix_timestamp(timestamp).ok())
}

fn invalid_time_zone() -> TimeZoneInvalid {
    TimeZoneInvalid {
        reason: TimeZoneFailureKind::InvalidTimeZone,
        title: "无法识别时区".into(),
        explanation: "请输入支持的城市别名或完整 IANA 时区名称".into(),
    }
}

pub(crate) fn local_clock_unavailable() -> TimeZoneInvalid {
    TimeZoneInvalid {
        reason: TimeZoneFailureKind::ClockUnavailable,
        title: "无法读取本地时间".into(),
        explanation: "请检查 Windows 日期和时区设置".into(),
    }
}

#[cfg(test)]
mod tests {
    use time::{format_description::well_known::Rfc3339, Date, OffsetDateTime};

    use super::{
        classify, evaluate, local_clock_unavailable, TimeZoneClassification, TimeZoneFailureKind,
    };

    fn instant(source: &str) -> OffsetDateTime {
        OffsetDateTime::parse(source, &Rfc3339).unwrap()
    }

    #[test]
    fn parses_terminal_time_for_aliases_and_case_insensitive_canonical_iana_names() {
        for source in [
            "NewYork time",
            "NEW YORK TIME",
            "new-york time",
            "new_york time",
            "America/New_York time",
            "america/new_york TIME",
            "UTC time",
            "GMT time",
        ] {
            assert!(
                matches!(classify(source), TimeZoneClassification::Ready(_)),
                "source={source}"
            );
        }
    }

    #[test]
    fn complete_unknown_and_ambiguous_time_expressions_are_typed_invalid() {
        for source in [
            "CST time",
            "EST time",
            "IST time",
            "Atlantis time",
            "Mars/Olympus time",
        ] {
            let TimeZoneClassification::Invalid(failure) = classify(source) else {
                panic!("expected typed invalid for {source}");
            };
            assert_eq!(
                failure.reason,
                TimeZoneFailureKind::InvalidTimeZone,
                "source={source}"
            );
            assert_eq!(failure.title, "无法识别时区", "source={source}");
            assert_eq!(
                failure.explanation, "请输入支持的城市别名或完整 IANA 时区名称",
                "source={source}"
            );
        }
        for source in [
            "",
            "New York",
            "time",
            "New York timer",
            "what time in New York",
        ] {
            assert_eq!(
                classify(source),
                TimeZoneClassification::NoMatch,
                "source={source}"
            );
        }
    }

    #[test]
    fn formats_exact_new_york_winter_and_summer_results_from_injected_utc_instants() {
        for (source, now, title, date, offset, abbreviation) in [
            (
                "New York time",
                "2026-01-15T15:35:20.123Z",
                "10:35",
                "2026-01-15",
                "UTC−05:00",
                "EST",
            ),
            (
                "New York time",
                "2026-07-15T14:35:20.123Z",
                "10:35",
                "2026-07-15",
                "UTC−04:00",
                "EDT",
            ),
        ] {
            let TimeZoneClassification::Ready(intent) = classify(source) else {
                panic!("source={source}")
            };
            let output = evaluate(intent, instant(now)).unwrap();
            assert_eq!(output.title, title);
            assert_eq!(
                output.subtitle.as_deref(),
                Some(format!("{date} · New York · {offset} ({abbreviation})").as_str())
            );
            assert_eq!(output.copy_text, title);
            assert_eq!(
                output.refresh_at_utc.unwrap().format(&Rfc3339).unwrap(),
                if now.starts_with("2026-01") {
                    "2026-01-15T15:36:00Z"
                } else {
                    "2026-07-15T14:36:00Z"
                }
            );
        }
    }

    #[test]
    fn observes_both_new_york_dst_transition_boundaries() {
        for (now, expected_title, expected_offset, expected_abbreviation) in [
            ("2026-03-08T06:59:00Z", "01:59", "UTC−05:00", "EST"),
            ("2026-03-08T07:00:00Z", "03:00", "UTC−04:00", "EDT"),
            ("2026-11-01T05:59:00Z", "01:59", "UTC−04:00", "EDT"),
            ("2026-11-01T06:00:00Z", "01:00", "UTC−05:00", "EST"),
        ] {
            let TimeZoneClassification::Ready(intent) = classify("New York time") else {
                unreachable!()
            };
            let output = evaluate(intent, instant(now)).unwrap();
            assert_eq!(output.title, expected_title, "now={now}");
            let subtitle = output.subtitle.unwrap();
            assert!(
                subtitle.contains(expected_offset),
                "now={now}, subtitle={subtitle}"
            );
            assert!(
                subtitle.ends_with(&format!("({expected_abbreviation})")),
                "now={now}, subtitle={subtitle}"
            );
        }
    }

    #[test]
    fn direct_iana_uses_canonical_name_and_utc_gmt_use_zero_offsets() {
        for (source, display, offset, abbreviation) in [
            (
                "america/new_york time",
                "America/New_York",
                "UTC−04:00",
                "EDT",
            ),
            ("UTC time", "UTC", "UTC+00:00", "UTC"),
            ("GMT time", "GMT", "UTC+00:00", "GMT"),
        ] {
            let TimeZoneClassification::Ready(intent) = classify(source) else {
                panic!("source={source}")
            };
            let output = evaluate(intent, instant("2026-09-03T14:35:00Z")).unwrap();
            let subtitle = output.subtitle.unwrap();
            assert!(
                subtitle.contains(&format!("· {display} ·")),
                "source={source}, subtitle={subtitle}"
            );
            assert!(
                subtitle.contains(offset),
                "source={source}, subtitle={subtitle}"
            );
            assert!(
                subtitle.ends_with(&format!("({abbreviation})")),
                "source={source}, subtitle={subtitle}"
            );
        }
    }

    #[test]
    fn next_minute_is_checked_and_crosses_date_and_year_in_strict_utc_form() {
        for (now, expected) in [
            ("2026-09-03T23:59:59.999999999Z", "2026-09-04T00:00:00Z"),
            ("2026-12-31T23:59:59.999999999Z", "2027-01-01T00:00:00Z"),
        ] {
            let TimeZoneClassification::Ready(intent) = classify("UTC time") else {
                unreachable!()
            };
            let output = evaluate(intent, instant(now)).unwrap();
            assert_eq!(
                output.refresh_at_utc.unwrap().format(&Rfc3339).unwrap(),
                expected
            );
        }
        let TimeZoneClassification::Ready(intent) = classify("UTC time") else {
            unreachable!()
        };
        let maximum = Date::MAX
            .with_hms_nano(23, 59, 59, 999_999_999)
            .unwrap()
            .assume_utc();
        assert_eq!(
            evaluate(intent, maximum).unwrap_err(),
            local_clock_unavailable()
        );
        let failure = local_clock_unavailable();
        assert_eq!(failure.reason, TimeZoneFailureKind::ClockUnavailable);
        assert_eq!(failure.title, "无法读取本地时间");
        assert_eq!(failure.explanation, "请检查 Windows 日期和时区设置");
    }
}
