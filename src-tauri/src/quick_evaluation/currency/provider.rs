use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;
use time::{Date, Month};

use super::{ExchangeRateError, ExchangeRateRequest, ExchangeRateResult};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrankfurterRate<'a> {
    date: &'a str,
    base: &'a str,
    quote: &'a str,
    rate: &'a RawValue,
}

pub(crate) fn map_status(status: u16) -> Result<(), ExchangeRateError> {
    match status {
        200 => Ok(()),
        404 | 422 => Err(ExchangeRateError::UnsupportedCode),
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
    let rate = response
        .rate
        .get()
        .parse::<Decimal>()
        .ok()
        .filter(|rate| *rate > Decimal::ZERO)
        .ok_or(ExchangeRateError::Unavailable)?;

    Ok(ExchangeRateResult {
        base: request.base.clone(),
        quote: request.quote.clone(),
        rate,
        effective_date,
    })
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
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":7.1324500000000001}"#,
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

        let exponent = parse_response(
            &request(),
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":1e3}"#,
        )
        .unwrap();
        assert_eq!(exponent.rate, Decimal::from(1000));
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
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY"}"#,
            br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":7.1,"extra":1}"#,
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
        assert_eq!(map_status(404), Err(ExchangeRateError::UnsupportedCode));
        assert_eq!(map_status(422), Err(ExchangeRateError::UnsupportedCode));
        assert_eq!(map_status(503), Err(ExchangeRateError::Unavailable));
        assert_eq!(map_status(500), Err(ExchangeRateError::Unavailable));
        assert_eq!(map_status(200), Ok(()));
    }
}
