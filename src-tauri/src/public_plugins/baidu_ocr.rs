use super::{
    network::{
        broker::{PluginNetworkRequestBody, PluginNetworkRequestMethod},
        PluginNetworkRequest,
    },
    PluginRequestContext,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const OCR_URL: &str = "https://aip.baidubce.com/rest/2.0/ocr/v1/general_basic";
pub(crate) const TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BaiduCommandInput {
    pub(crate) context: PluginRequestContext,
    pub(crate) input: BaiduInput,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BaiduInput {
    pub(crate) image_id: String,
}
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct BaiduResponse {
    pub(crate) text: String,
    pub(crate) lines: Vec<String>,
}
pub(crate) fn form_request(url: &str, value: BTreeMap<String, String>) -> PluginNetworkRequest {
    PluginNetworkRequest {
        url: url.into(),
        method: PluginNetworkRequestMethod::Post,
        headers: None,
        body: Some(PluginNetworkRequestBody::Form { value }),
        credential: None,
    }
}
pub(crate) fn authentication(
    values: &BTreeMap<String, String>,
) -> Result<PluginNetworkRequest, &'static str> {
    Ok(form_request(
        TOKEN_URL,
        BTreeMap::from([
            ("grant_type".into(), "client_credentials".into()),
            (
                "client_id".into(),
                values.get("apiKey").ok_or("credentialUnavailable")?.clone(),
            ),
            (
                "client_secret".into(),
                values
                    .get("secretKey")
                    .ok_or("credentialUnavailable")?
                    .clone(),
            ),
        ]),
    ))
}
fn response(status: u16, body: &str) -> Result<serde_json::Value, &'static str> {
    if matches!(status, 401 | 403) {
        return Err("providerAuthentication");
    }
    if status == 429 {
        return Err("providerQuota");
    }
    if !(200..300).contains(&status) {
        return Err("providerFailure");
    }
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "networkResponseInvalid")?;
    if let Some(error) = value.get("error_code") {
        return Err(match error.as_u64() {
            Some(6 | 14 | 15 | 100 | 110 | 111) => "providerAuthentication",
            Some(4 | 17 | 18 | 19) => "providerQuota",
            _ => "providerFailure",
        });
    }
    if value.get("error").is_some() {
        return Err("providerAuthentication");
    }
    Ok(value)
}
pub(crate) fn access_token(status: u16, body: &str) -> Result<String, &'static str> {
    let value = response(status, body)?;
    if !value["expires_in"]
        .as_u64()
        .is_some_and(|seconds| seconds > 0)
    {
        return Err("providerAuthentication");
    }
    let token = value["access_token"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.chars().any(char::is_control))
        .ok_or("providerAuthentication")?;
    if !value["scope"].as_str().is_some_and(|scope| {
        scope.split_ascii_whitespace().any(|entry| {
            entry == "brain_ocr"
                || entry == "vis-ocr"
                || entry.starts_with("brain_ocr_")
                || entry.starts_with("vis-ocr_")
        })
    }) {
        return Err("providerAuthentication");
    }
    Ok(token.into())
}
pub(crate) fn parse_response(status: u16, body: &str) -> Result<BaiduResponse, &'static str> {
    let value = response(status, body)?;
    let results = value["words_result"]
        .as_array()
        .ok_or("networkResponseInvalid")?;
    if results.len() > 4096 {
        return Err("networkResponseTooLarge");
    }
    let lines: Vec<String> = results
        .iter()
        .map(|row| {
            row["words"]
                .as_str()
                .filter(|s| !s.contains('\0'))
                .map(str::to_owned)
                .ok_or("networkResponseInvalid")
        })
        .collect::<Result<_, _>>()?;
    let text = lines.join("\n");
    // Main result detail and copyText each have a 16KiB contract.
    if text.len() > 16384 {
        return Err("networkResponseTooLarge");
    }
    Ok(BaiduResponse { text, lines })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_order_empty_result_and_errors_are_safe() {
        assert_eq!(
            parse_response(
                200,
                r#"{"words_result":[{"words":"first"},{"words":"second"}]}"#
            )
            .unwrap()
            .text,
            "first\nsecond"
        );
        assert_eq!(
            parse_response(200, r#"{"words_result":[]}"#).unwrap().text,
            ""
        );
        for code in [4, 17, 18, 19] {
            assert_eq!(
                parse_response(
                    200,
                    &format!(r#"{{"error_code":{code},"error_msg":"secret"}}"#)
                ),
                Err("providerQuota")
            );
        }
        assert_eq!(parse_response(200, "secret"), Err("networkResponseInvalid"));
        assert_eq!(
            parse_response(
                200,
                &serde_json::json!({"words_result":[{"words":"a".repeat(16385)}]}).to_string()
            ),
            Err("networkResponseTooLarge")
        );
    }
    #[test]
    fn authentication_is_fixed_and_scope_checked() {
        let request = authentication(&BTreeMap::from([
            ("apiKey".into(), "synthetic".into()),
            ("secretKey".into(), "synthetic-secret".into()),
        ]))
        .unwrap();
        assert_eq!(request.url, TOKEN_URL);
        assert_eq!(
            access_token(
                200,
                r#"{"access_token":"synthetic-token","scope":"brain_ocr","expires_in":3600}"#
            ),
            Ok("synthetic-token".into())
        );
        assert_eq!(
            access_token(
                200,
                r#"{"access_token":"synthetic-token","scope":"brain_ocr"}"#
            ),
            Err("providerAuthentication")
        );
        assert_eq!(
            access_token(
                200,
                r#"{"access_token":"synthetic-token","scope":"brain_ocr","expires_in":0}"#
            ),
            Err("providerAuthentication")
        );
        assert_eq!(
            access_token(200, r#"{"access_token":"secret","scope":"profile"}"#),
            Err("providerAuthentication")
        );
    }
}
