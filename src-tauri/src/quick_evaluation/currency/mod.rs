pub(crate) mod cache;
pub(crate) mod provider;
pub(crate) mod service;
pub(crate) mod transport;

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod transport_tests;

use rust_decimal::{Decimal, RoundingStrategy};
use time::Date;

use super::{
    normalize_query,
    number::{canonicalize_number, NumberGrammar},
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CurrencyCode(String);

impl CurrencyCode {
    pub(crate) fn new(source: &str) -> Option<Self> {
        (source.len() == 3 && source.bytes().all(|byte| byte.is_ascii_alphabetic()))
            .then(|| Self(source.to_ascii_uppercase()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CurrencyIntent {
    pub(crate) amount: Decimal,
    pub(crate) base: CurrencyCode,
    pub(crate) quote: CurrencyCode,
}

impl CurrencyIntent {
    pub(crate) fn exchange_request(&self) -> Option<ExchangeRateRequest> {
        (self.base != self.quote).then(|| ExchangeRateRequest {
            base: self.base.clone(),
            quote: self.quote.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CurrencyFailure {
    InvalidNumber,
    InvalidCode,
    CalculationOverflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CurrencyClassification {
    NoMatch,
    Invalid(CurrencyFailure),
    Ready(CurrencyIntent),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ExchangeRateRequest {
    pub(crate) base: CurrencyCode,
    pub(crate) quote: CurrencyCode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExchangeRateResult {
    pub(crate) base: CurrencyCode,
    pub(crate) quote: CurrencyCode,
    pub(crate) rate: Decimal,
    pub(crate) effective_date: Date,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExchangeRateError {
    UnsupportedCode,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CurrencyOutput {
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) copy_text: String,
}

pub(crate) fn evaluate_provider_result(
    intent: &CurrencyIntent,
    result: ExchangeRateResult,
) -> Result<CurrencyOutput, CurrencyFailure> {
    debug_assert_eq!(result.base, intent.base);
    debug_assert_eq!(result.quote, intent.quote);
    let converted = intent
        .amount
        .checked_mul(result.rate)
        .ok_or(CurrencyFailure::CalculationOverflow)?;
    let title = format!(
        "{} {}",
        format_converted_amount(converted),
        intent.quote.as_str()
    );
    let subtitle = format!(
        "{} {} · 1 {} = {} {} · Frankfurter · 汇率日期 {}",
        format_decimal(intent.amount),
        intent.base.as_str(),
        intent.base.as_str(),
        format_decimal(result.rate),
        intent.quote.as_str(),
        result.effective_date
    );

    Ok(CurrencyOutput {
        copy_text: title.clone(),
        title,
        subtitle,
    })
}

pub(crate) fn evaluate_same_code(
    intent: &CurrencyIntent,
) -> Option<Result<CurrencyOutput, CurrencyFailure>> {
    if intent.base != intent.quote {
        return None;
    }
    Some(
        intent
            .amount
            .checked_mul(Decimal::ONE)
            .ok_or(CurrencyFailure::CalculationOverflow)
            .map(|converted| {
                let title = format!(
                    "{} {}",
                    format_converted_amount(converted),
                    intent.quote.as_str()
                );
                CurrencyOutput {
                    copy_text: title.clone(),
                    title,
                    subtitle: format!(
                        "{} {} · 1 {} = 1 {}",
                        format_decimal(intent.amount),
                        intent.base.as_str(),
                        intent.base.as_str(),
                        intent.quote.as_str()
                    ),
                }
            }),
    )
}

fn format_converted_amount(value: Decimal) -> String {
    format_decimal(value.round_dp_with_strategy(3, RoundingStrategy::MidpointAwayFromZero))
}

fn format_decimal(value: Decimal) -> String {
    if value.is_zero() {
        "0".into()
    } else {
        value.normalize().to_string()
    }
}

pub(super) fn parse_decimal_exact(source: &str) -> Option<Decimal> {
    const MAX_DECIMAL_SOURCE_BYTES: usize = 64 * 1024;
    if source.is_empty() || source.len() > MAX_DECIMAL_SOURCE_BYTES {
        return None;
    }

    let (mantissa, exponent) =
        if let Some(index) = source.bytes().position(|byte| matches!(byte, b'e' | b'E')) {
            (&source[..index], parse_exponent(&source[index + 1..])?)
        } else {
            (source, 0)
        };
    let (negative, mantissa) = mantissa.strip_prefix('-').map_or_else(
        || {
            mantissa
                .strip_prefix('+')
                .map_or((false, mantissa), |value| (false, value))
        },
        |value| (true, value),
    );
    let (integer, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, ""), |(integer, fraction)| (integer, fraction));
    if integer.len().checked_add(fraction.len())? == 0
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }

    let mut digits = String::with_capacity(integer.len().checked_add(fraction.len())?);
    digits.push_str(integer);
    digits.push_str(fraction);
    let Some(first_nonzero) = digits.bytes().position(|byte| byte != b'0') else {
        return Some(Decimal::ZERO);
    };
    let last_nonzero = digits
        .bytes()
        .rposition(|byte| byte != b'0')
        .expect("nonzero digit exists");
    let significant = &digits[first_nonzero..=last_nonzero];
    let decimal_position = i64::try_from(integer.len())
        .ok()?
        .checked_add(exponent)?
        .checked_sub(i64::try_from(first_nonzero).ok()?)?;

    let mut fixed = String::with_capacity(32);
    if negative {
        fixed.push('-');
    }
    if decimal_position <= 0 {
        let leading_fractional_zeroes = usize::try_from(decimal_position.checked_neg()?).ok()?;
        let scale = leading_fractional_zeroes.checked_add(significant.len())?;
        if scale > 28 {
            return None;
        }
        fixed.push_str("0.");
        fixed.extend(std::iter::repeat_n('0', leading_fractional_zeroes));
        fixed.push_str(significant);
    } else {
        let decimal_position = usize::try_from(decimal_position).ok()?;
        if decimal_position > 29 {
            return None;
        }
        if decimal_position >= significant.len() {
            fixed.push_str(significant);
            fixed.extend(std::iter::repeat_n(
                '0',
                decimal_position - significant.len(),
            ));
        } else {
            let scale = significant.len() - decimal_position;
            if scale > 28 {
                return None;
            }
            fixed.push_str(&significant[..decimal_position]);
            fixed.push('.');
            fixed.push_str(&significant[decimal_position..]);
        }
    }

    Decimal::from_str_exact(&fixed).ok()
}

fn parse_exponent(source: &str) -> Option<i64> {
    let (negative, digits) = source.strip_prefix('-').map_or_else(
        || {
            source
                .strip_prefix('+')
                .map_or((false, source), |value| (false, value))
        },
        |value| (true, value),
    );
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.bytes().try_fold(0_i64, |value, byte| {
        value.checked_mul(10)?.checked_add(i64::from(byte - b'0'))
    })?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

pub(crate) fn classify(source: &str) -> CurrencyClassification {
    let Some(normalized) = normalize_query(source) else {
        return CurrencyClassification::NoMatch;
    };
    let parts = normalized.split(' ').collect::<Vec<_>>();
    if parts.len() != 4 || !parts[2].eq_ignore_ascii_case("to") {
        return CurrencyClassification::NoMatch;
    }

    let (amount_source, base_source, quote_source) = (parts[0], parts[1], parts[3]);
    if super::units::owns_both_unit_operands(&normalized) {
        return CurrencyClassification::NoMatch;
    }

    let Some(canonical_amount) = canonicalize_number(amount_source, NumberGrammar::Number) else {
        return if looks_like_invalid_number(amount_source)
            && CurrencyCode::new(base_source).is_some()
            && CurrencyCode::new(quote_source).is_some()
        {
            CurrencyClassification::Invalid(CurrencyFailure::InvalidNumber)
        } else {
            CurrencyClassification::NoMatch
        };
    };
    let Some(amount) = parse_decimal_exact(&canonical_amount) else {
        return CurrencyClassification::Invalid(CurrencyFailure::InvalidNumber);
    };

    let base = CurrencyCode::new(base_source);
    let quote = CurrencyCode::new(quote_source);
    if base.is_none() || quote.is_none() {
        return if (base.is_some() && malformed_code_like(quote_source))
            || (quote.is_some() && malformed_code_like(base_source))
        {
            CurrencyClassification::Invalid(CurrencyFailure::InvalidCode)
        } else {
            CurrencyClassification::NoMatch
        };
    }

    CurrencyClassification::Ready(CurrencyIntent {
        amount,
        base: base.expect("validated currency code"),
        quote: quote.expect("validated currency code"),
    })
}

fn malformed_code_like(source: &str) -> bool {
    let bytes = source.as_bytes();
    ((1..=2).contains(&bytes.len()) && bytes.iter().all(u8::is_ascii_alphabetic))
        || ((1..=4).contains(&bytes.len())
            && bytes.iter().all(u8::is_ascii_alphanumeric)
            && bytes.iter().any(u8::is_ascii_digit))
        || (bytes.len() == 4 && bytes.iter().all(u8::is_ascii_uppercase))
}

fn looks_like_invalid_number(source: &str) -> bool {
    source.eq_ignore_ascii_case("nan")
        || source.eq_ignore_ascii_case("inf")
        || source.eq_ignore_ascii_case("infinity")
        || (source
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b'+' | b'-' | b'.') || byte.is_ascii_digit())
            && source.bytes().all(|byte| {
                byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b',' | b'e' | b'E')
            }))
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use time::{Date, Month};

    use crate::quick_evaluation::units::UnitClassification;

    use super::{
        classify, evaluate_provider_result, evaluate_same_code, CurrencyClassification,
        CurrencyFailure, ExchangeRateResult,
    };

    fn ready(source: &str) -> super::CurrencyIntent {
        let CurrencyClassification::Ready(intent) = classify(source) else {
            panic!("expected ready currency intent for {source:?}");
        };
        intent
    }

    #[test]
    fn parses_approved_numbers_and_normalizes_mixed_case_codes() {
        for (source, amount, base, quote) in [
            ("100 USD to CNY", "100", "USD", "CNY"),
            ("1,000.5 eur TO jPy", "1000.5", "EUR", "JPY"),
            ("+.5 gbp to usd", "0.5", "GBP", "USD"),
            ("-1. cad to MXN", "-1", "CAD", "MXN"),
        ] {
            let intent = ready(source);
            assert_eq!(intent.amount, Decimal::from_str_exact(amount).unwrap());
            assert_eq!(intent.base.as_str(), base);
            assert_eq!(intent.quote.as_str(), quote);
        }
    }

    #[test]
    fn currency_amounts_require_exact_decimal_representation() {
        for source in [
            "0.12345678901234567890123456789 USD to CNY",
            "79228162514264337593543950336 USD to CNY",
        ] {
            assert_eq!(
                classify(source),
                CurrencyClassification::Invalid(CurrencyFailure::InvalidNumber),
                "{source}"
            );
        }

        assert_eq!(
            ready("0.1234567890123456789012345678 USD to CNY").amount,
            Decimal::from_str_exact("0.1234567890123456789012345678").unwrap()
        );
        assert_eq!(
            ready("79228162514264337593543950335 USD to CNY").amount,
            Decimal::MAX
        );

        for (source, expected) in [
            ("1.00000000000000000000000000000 USD to CNY", Decimal::ONE),
            (
                "0.00000000000000000000000000010 USD to CNY",
                Decimal::from_str_exact("0.0000000000000000000000000001").unwrap(),
            ),
            (
                "00079228162514264337593543950335.0000 USD to CNY",
                Decimal::MAX,
            ),
            ("+.5000 USD to CNY", Decimal::new(5, 1)),
            ("-1. USD to CNY", Decimal::new(-1, 0)),
            ("-.5000 USD to CNY", Decimal::new(-5, 1)),
        ] {
            assert_eq!(ready(source).amount, expected, "{source}");
        }
    }

    #[test]
    fn incomplete_or_non_currency_forms_fall_through() {
        for source in [
            "100 USD",
            "100 USD to",
            "100 dollars to CNY",
            "100 USD to yuan",
            "100 USD to cnyy",
            "add pdf to zip",
            "add US to CNY",
            "100 US to CN",
            "$100 USD to CNY",
            "¥100 JPY to USD",
            "元100 CNY to USD",
            "美元100 USD to CNY",
            "100 USD into CNY",
            "100 USD to CNY now",
        ] {
            assert_eq!(
                classify(source),
                CurrencyClassification::NoMatch,
                "{source}"
            );
        }
    }

    #[test]
    fn complete_currency_signatures_report_invalid_operands() {
        for source in ["1,5 USD to CNY", "1e3 USD to CNY", "NaN USD to CNY"] {
            assert_eq!(
                classify(source),
                CurrencyClassification::Invalid(CurrencyFailure::InvalidNumber),
                "{source}"
            );
        }
        for source in [
            "100 US to CNY",
            "100 USD to C",
            "100 USD to CNYY",
            "100 CNYY to USD",
            "100 USD to CN1",
            "100 CN1 to USD",
        ] {
            assert_eq!(
                classify(source),
                CurrencyClassification::Invalid(CurrencyFailure::InvalidCode),
                "{source}"
            );
        }
    }

    #[test]
    fn two_known_unit_operands_defer_before_currency_code_shapes() {
        for source in ["1 kg to lb", "1 tsp to cup", "1 kg to m", "1 CUP to CUP"] {
            assert_eq!(
                classify(source),
                CurrencyClassification::NoMatch,
                "{source}"
            );
            assert!(
                !matches!(
                    crate::quick_evaluation::units::classify(source),
                    UnitClassification::NoMatch
                ),
                "unit recognizer must own {source}"
            );
        }

        let currency = ready("1 USD to CUP");
        assert_eq!(currency.base.as_str(), "USD");
        assert_eq!(currency.quote.as_str(), "CUP");
    }

    #[test]
    fn same_code_intent_requires_no_exchange_rate_request() {
        let same = ready("100 usd to USD");
        assert_eq!(same.exchange_request(), None);

        let different = ready("100 usd to CNY");
        let request = different.exchange_request().expect("different pair");
        assert_eq!(request.base.as_str(), "USD");
        assert_eq!(request.quote.as_str(), "CNY");
    }

    #[test]
    fn provider_result_uses_checked_decimal_math_and_exact_approved_text() {
        let intent = ready("100 USD to CNY");
        let output = evaluate_provider_result(
            &intent,
            ExchangeRateResult {
                base: intent.base.clone(),
                quote: intent.quote.clone(),
                rate: "7.1324500000000001".parse().unwrap(),
                effective_date: Date::from_calendar_date(2026, Month::September, 3).unwrap(),
            },
        )
        .unwrap();

        assert_eq!(output.title, "713.245 CNY");
        assert_eq!(
            output.subtitle,
            "100 USD · 1 USD = 7.1324500000000001 CNY · Frankfurter · 汇率日期 2026-09-03"
        );
        assert_eq!(output.copy_text, output.title);
    }

    #[test]
    fn converted_amount_has_at_most_three_fractional_digits_and_no_negative_zero() {
        for (query, rate, expected) in [
            ("1.2344 USD to CNY", "1", "1.234 CNY"),
            ("1.2345 USD to CNY", "1", "1.235 CNY"),
            ("1.2300 USD to CNY", "1", "1.23 CNY"),
            ("-0.0004 USD to CNY", "1", "0 CNY"),
        ] {
            let intent = ready(query);
            let output = evaluate_provider_result(
                &intent,
                ExchangeRateResult {
                    base: intent.base.clone(),
                    quote: intent.quote.clone(),
                    rate: rate.parse().unwrap(),
                    effective_date: Date::from_calendar_date(2026, Month::September, 3).unwrap(),
                },
            )
            .unwrap();
            assert_eq!(output.title, expected, "query={query}");
            assert_eq!(output.copy_text, expected, "query={query}");
        }
    }

    #[test]
    fn checked_decimal_overflow_returns_a_typed_failure() {
        let intent = ready("2 USD to CNY");
        let result = evaluate_provider_result(
            &intent,
            ExchangeRateResult {
                base: intent.base.clone(),
                quote: intent.quote.clone(),
                rate: Decimal::MAX,
                effective_date: Date::from_calendar_date(2026, Month::September, 3).unwrap(),
            },
        );

        assert_eq!(result, Err(CurrencyFailure::CalculationOverflow));
    }

    #[test]
    fn same_code_is_evaluated_locally_at_rate_one() {
        let intent = ready("100.000 usd to USD");
        let output = evaluate_same_code(&intent)
            .expect("same-code conversion should be local")
            .expect("multiplication by one should succeed");

        assert_eq!(output.title, "100 USD");
        assert_eq!(output.subtitle, "100 USD · 1 USD = 1 USD");
        assert_eq!(output.copy_text, output.title);
        assert!(evaluate_same_code(&ready("100 USD to CNY")).is_none());
    }
}
