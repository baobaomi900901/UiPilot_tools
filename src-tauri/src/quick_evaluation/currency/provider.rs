use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;
use time::{Date, Month};

use super::{ExchangeRateError, ExchangeRateRequest, ExchangeRateResult};

#[derive(Deserialize)]
struct FrankfurterRate<'a> {
    date: &'a str,
    base: &'a str,
    quote: &'a str,
    rate: &'a RawValue,
}

pub(crate) fn map_status(status: u16) -> Result<(), ExchangeRateError> {
    match status {
        200 => Ok(()),
        422 => Err(ExchangeRateError::UnsupportedCode),
        _ => Err(ExchangeRateError::Unavailable),
    }
}

pub(crate) fn parse_response(
    request: &ExchangeRateRequest,
    body: &[u8],
) -> Result<ExchangeRateResult, ExchangeRateError> {
    let response: FrankfurterRate<'_> =
        serde_json::from_slice(body).map_err(|_| ExchangeRateError::Unavailable)?;
    if response.base != request.base.as_str() || response.quote != request.quote.as_str() {
        return Err(ExchangeRateError::Unavailable);
    }
    let effective_date = parse_iso_date(response.date).ok_or(ExchangeRateError::Unavailable)?;
    let rate = parse_exact_decimal(response.rate.get())
        .filter(|rate| *rate > Decimal::ZERO)
        .ok_or(ExchangeRateError::Unavailable)?;

    Ok(ExchangeRateResult {
        base: request.base.clone(),
        quote: request.quote.clone(),
        rate,
        effective_date,
    })
}

fn parse_exact_decimal(source: &str) -> Option<Decimal> {
    const MAX_PROVIDER_NUMBER_BYTES: usize = 64 * 1024;
    if source.len() > MAX_PROVIDER_NUMBER_BYTES {
        return None;
    }
    let Some(exponent_index) = source.bytes().position(|byte| matches!(byte, b'e' | b'E')) else {
        return Decimal::from_str_exact(source).ok();
    };

    let mantissa = &source[..exponent_index];
    let exponent = parse_exponent(&source[exponent_index + 1..])?;
    let (negative, mantissa) = mantissa
        .strip_prefix('-')
        .map_or((false, mantissa), |value| (true, value));
    let (integer, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, ""), |(integer, fraction)| (integer, fraction));
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || (mantissa.contains('.') && fraction.is_empty())
    {
        return None;
    }

    let mut digits = String::with_capacity(integer.len().checked_add(fraction.len())?);
    digits.push_str(integer);
    digits.push_str(fraction);
    let first_nonzero = digits.bytes().position(|byte| byte != b'0');
    let Some(first_nonzero) = first_nonzero else {
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

fn parse_iso_date(source: &str) -> Option<Date> {
    let bytes = source.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return None;
    }
    let year = source[0..4].parse().ok()?;
    let month = Month::try_from(source[5..7].parse::<u8>().ok()?).ok()?;
    let day = source[8..10].parse().ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use time::{Date, Month};

    use super::{map_status, parse_response};
    use crate::quick_evaluation::currency::{CurrencyCode, ExchangeRateError, ExchangeRateRequest};

    fn request() -> ExchangeRateRequest {
        ExchangeRateRequest {
            base: CurrencyCode::new("USD").unwrap(),
            quote: CurrencyCode::new("CNY").unwrap(),
        }
    }

    #[test]
    fn parses_the_frankfurter_v2_single_pair_object_as_exact_decimal_data() {
        let parsed = parse_response(
            &request(),
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":7.1324500000000001,"provider":"ECB","metadata":{"revision":1}}"#,
        )
        .unwrap();

        assert_eq!(parsed.base.as_str(), "USD");
        assert_eq!(parsed.quote.as_str(), "CNY");
        assert_eq!(
            parsed.rate,
            "7.1324500000000001".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            parsed.effective_date,
            Date::from_calendar_date(2026, Month::September, 3).unwrap()
        );

        for (rate, expected) in [
            ("1e3", "1000"),
            ("7.13245e0", "7.13245"),
            ("713245e-5", "7.13245"),
            ("1e-28", "0.0000000000000000000000000001"),
            ("10e-29", "0.0000000000000000000000000001"),
        ] {
            let body =
                format!(r#"{{"date":"2026-09-03","base":"USD","quote":"CNY","rate":{rate}}}"#);
            let parsed = parse_response(&request(), body.as_bytes()).unwrap();
            assert_eq!(
                parsed.rate,
                Decimal::from_str_exact(expected).unwrap(),
                "rate={rate}"
            );
        }
    }

    #[test]
    fn rejects_mismatched_pair_date_rate_and_required_object_shape() {
        for body in [
            br#"{"date":"2026-09-03","base":"EUR","quote":"CNY","rate":7.1}"#.as_slice(),
            br#"{"date":"2026-09-03","base":"USD","quote":"JPY","rate":7.1}"#,
            br#"{"date":"2026-02-30","base":"USD","quote":"CNY","rate":7.1}"#,
            br#"{"date":"2026-9-3","base":"USD","quote":"CNY","rate":7.1}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":0}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":-1}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":"7.1"}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":true}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":null}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":[]}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":{}}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":1e100}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":1e-29}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":0.12345678901234567890123456789}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":79228162514264337593543950336e0}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":1e999999999999999999999}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY"}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":7.1} trailing"#,
            br#"[{"date":"2026-09-03","base":"USD","quote":"CNY","rate":7.1}]"#,
        ] {
            assert_eq!(
                parse_response(&request(), body),
                Err(ExchangeRateError::Unavailable)
            );
        }
    }

    #[test]
    fn maps_only_documented_unsupported_statuses_away_from_unavailable() {
        assert_eq!(map_status(404), Err(ExchangeRateError::Unavailable));
        assert_eq!(map_status(422), Err(ExchangeRateError::UnsupportedCode));
        assert_eq!(map_status(503), Err(ExchangeRateError::Unavailable));
        assert_eq!(map_status(500), Err(ExchangeRateError::Unavailable));
        assert_eq!(map_status(200), Ok(()));
    }
}
