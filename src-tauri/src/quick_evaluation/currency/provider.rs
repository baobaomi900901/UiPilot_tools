use std::{future::Future, pin::Pin, sync::Arc};

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;
use time::{Date, Month};

use super::{
    parse_decimal_exact,
    transport::{NativeProviderTransport, ProviderHttpRequest, ProviderTransport},
    ExchangeRateError, ExchangeRateRequest, ExchangeRateResult,
};

pub(crate) type ExchangeRateProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ExchangeRateResult, ExchangeRateError>> + Send + 'a>>;

pub(crate) trait ExchangeRateProvider: Send + Sync {
    fn fetch_rate<'a>(&'a self, request: &'a ExchangeRateRequest)
        -> ExchangeRateProviderFuture<'a>;
}

pub(crate) struct FrankfurterProvider {
    transport: Arc<dyn ProviderTransport>,
}

impl FrankfurterProvider {
    pub(crate) fn new() -> Result<Self, ExchangeRateError> {
        NativeProviderTransport::new()
            .map(|transport| Self::with_transport(Arc::new(transport)))
            .map_err(|_| ExchangeRateError::Unavailable)
    }

    pub(crate) fn with_transport(transport: Arc<dyn ProviderTransport>) -> Self {
        Self { transport }
    }

    async fn fetch(
        &self,
        request: &ExchangeRateRequest,
    ) -> Result<ExchangeRateResult, ExchangeRateError> {
        let outgoing =
            ProviderHttpRequest::for_exchange(request).ok_or(ExchangeRateError::Unavailable)?;
        let response = self
            .transport
            .execute(outgoing)
            .await
            .map_err(|_| ExchangeRateError::Unavailable)?;
        map_status(response.status())?;
        parse_response(request, response.body())
    }
}

impl ExchangeRateProvider for FrankfurterProvider {
    fn fetch_rate<'a>(
        &'a self,
        request: &'a ExchangeRateRequest,
    ) -> ExchangeRateProviderFuture<'a> {
        Box::pin(self.fetch(request))
    }
}

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
    let rate = parse_decimal_exact(response.rate.get())
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
    if !(1..=9999).contains(&year) {
        return None;
    }
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
            Decimal::from_str_exact("7.1324500000000001").unwrap()
        );
        assert_eq!(
            parsed.effective_date,
            Date::from_calendar_date(2026, Month::September, 3).unwrap()
        );

        for (rate, expected) in [
            ("1.00000000000000000000000000000", "1"),
            (
                "0.00000000000000000000000000010",
                "0.0000000000000000000000000001",
            ),
            (
                "79228162514264337593543950335.0000",
                "79228162514264337593543950335",
            ),
            ("1e3", "1000"),
            ("7.13245e0", "7.13245"),
            ("713245e-5", "7.13245"),
            ("1e-28", "0.0000000000000000000000000001"),
            ("10e-29", "0.0000000000000000000000000001"),
            (
                "0.00000000000000000000000000010e0",
                "0.0000000000000000000000000001",
            ),
            (
                "792281625142643375935439503350000e-4",
                "79228162514264337593543950335",
            ),
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
            br#"{"date":"0000-09-03","base":"USD","quote":"CNY","rate":7.1}"#,
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
