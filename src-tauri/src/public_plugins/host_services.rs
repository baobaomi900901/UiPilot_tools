use super::{
    network::broker::{PluginNetworkRequestBody, PluginNetworkRequestMethod},
    network::{PluginNetworkErrorCode, PluginNetworkRequest},
    PluginRequestContext,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct YoudaoCommandInput {
    pub(crate) context: PluginRequestContext,
    pub(crate) input: YoudaoInput,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YoudaoInput {
    text: String,
    from: String,
    to: String,
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct YoudaoResponse {
    pub(crate) translations: Vec<String>,
    pub(crate) source_language: Option<String>,
    pub(crate) target_language: String,
    pub(crate) unchanged: bool,
}
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ServiceError {
    pub(crate) code: &'static str,
}
impl From<&'static str> for ServiceError {
    fn from(code: &'static str) -> Self {
        Self { code }
    }
}
pub(crate) fn network_code(error: PluginNetworkErrorCode) -> &'static str {
    match error {
        PluginNetworkErrorCode::InvalidNetworkRequest => "invalidServiceRequest",
        PluginNetworkErrorCode::PermissionDenied => "permissionDenied",
        PluginNetworkErrorCode::NetworkTargetDenied => "networkTargetDenied",
        PluginNetworkErrorCode::NetworkTimeout => "networkTimeout",
        PluginNetworkErrorCode::NetworkFailure => "networkFailure",
        PluginNetworkErrorCode::CredentialUnavailable => "credentialUnavailable",
        PluginNetworkErrorCode::NetworkResponseTooLarge => "networkResponseTooLarge",
        PluginNetworkErrorCode::NetworkResponseInvalid => "networkResponseInvalid",
        PluginNetworkErrorCode::NetworkLimitExceeded => "networkLimitExceeded",
        PluginNetworkErrorCode::ExpiredRequest => "expiredRequest",
    }
}
impl YoudaoInput {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.text.trim().is_empty()
            || self.text.contains('\0')
            || self.text.len() > 8192
            || !matches!(self.from.as_str(), "auto" | "en" | "zh-CHS")
            || !matches!(self.to.as_str(), "en" | "zh-CHS")
        {
            Err("invalidServiceRequest")
        } else {
            Ok(())
        }
    }
    pub(crate) fn extended_input(&self) -> bool {
        self.from == "auto" || self.from == self.to
    }
    pub(crate) fn unchanged_response(&self) -> Option<YoudaoResponse> {
        (self.from == self.to).then(|| YoudaoResponse {
            translations: vec![self.text.clone()],
            source_language: Some(self.from.clone()),
            target_language: self.to.clone(),
            unchanged: true,
        })
    }
    pub(crate) fn request(
        &self,
        values: &BTreeMap<String, String>,
        salt: &str,
        timestamp: &str,
    ) -> Result<PluginNetworkRequest, &'static str> {
        self.validate()?;
        let app_id = values.get("appId").ok_or("credentialUnavailable")?;
        let app_key = values.get("appKey").ok_or("credentialUnavailable")?;
        let chars: Vec<char> = self.text.chars().collect();
        let input = if chars.len() <= 20 {
            self.text.clone()
        } else {
            format!(
                "{}{}{}",
                chars[..10].iter().collect::<String>(),
                chars.len(),
                chars[chars.len() - 10..].iter().collect::<String>()
            )
        };
        let sign = format!(
            "{:x}",
            Sha256::digest(format!("{app_id}{input}{salt}{timestamp}{app_key}"))
        );
        let body = BTreeMap::from([
            ("q".into(), self.text.clone()),
            ("from".into(), self.from.clone()),
            ("to".into(), self.to.clone()),
            ("appKey".into(), app_id.clone()),
            ("salt".into(), salt.into()),
            ("signType".into(), "v3".into()),
            ("strict".into(), "true".into()),
            ("curtime".into(), timestamp.into()),
            ("sign".into(), sign),
        ]);
        Ok(PluginNetworkRequest {
            url: "https://openapi.youdao.com/api".into(),
            method: PluginNetworkRequestMethod::Post,
            headers: None,
            body: Some(PluginNetworkRequestBody::Form { value: body }),
            credential: None,
        })
    }
}
pub(crate) fn parse_response(
    status: u16,
    body: &str,
    input: &YoudaoInput,
) -> Result<YoudaoResponse, &'static str> {
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
    match value
        .get("errorCode")
        .and_then(serde_json::Value::as_str)
        .ok_or("networkResponseInvalid")?
    {
        "0" => {}
        "108" | "110" | "111" | "112" | "202" | "203" => return Err("providerAuthentication"),
        "401" | "411" | "412" | "1411" | "2411" | "9411" => return Err("providerQuota"),
        _ => return Err("providerFailure"),
    }
    let translations: Vec<String> = serde_json::from_value(
        value
            .get("translation")
            .cloned()
            .ok_or("networkResponseInvalid")?,
    )
    .map_err(|_| "networkResponseInvalid")?;
    if translations.is_empty()
        || translations
            .iter()
            .any(|s| s.is_empty() || s.contains('\0'))
    {
        return Err("networkResponseInvalid");
    }
    if translations
        .iter()
        .map(|s| s.len())
        .sum::<usize>()
        .saturating_add(translations.len().saturating_sub(1))
        > 16384
    {
        return Err("networkResponseTooLarge");
    }
    let language = |value: &str| {
        if value.eq_ignore_ascii_case("en") {
            Some("en".to_string())
        } else if value.eq_ignore_ascii_case("zh-CHS") {
            Some("zh-CHS".to_string())
        } else {
            None
        }
    };
    let source_language = match value.get("l") {
        None => None,
        Some(pair) => {
            let (from, to) = pair
                .as_str()
                .and_then(|pair| pair.split_once('2'))
                .ok_or("networkResponseInvalid")?;
            if language(to).as_deref() != Some(input.to.as_str()) {
                return Err("networkResponseInvalid");
            }
            language(from)
        }
    };
    Ok(YoudaoResponse {
        unchanged: source_language.as_deref() == Some(input.to.as_str()),
        translations,
        source_language,
        target_language: input.to.clone(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_languages_and_detection_are_normalized_without_guessing() {
        for from in ["auto", "en", "zh-CHS"] {
            for to in ["en", "zh-CHS"] {
                let input: YoudaoInput = serde_json::from_value(
                    serde_json::json!({"text":"hello", "from":from, "to":to}),
                )
                .unwrap();
                assert_eq!(input.validate(), Ok(()));
            }
        }
    }
    #[test]
    fn detection_metadata_comes_only_from_provider_language_pair() {
        let input: YoudaoInput = serde_json::from_value(
            serde_json::json!({"text":"hello", "from":"auto", "to":"zh-CHS"}),
        )
        .unwrap();
        let response = parse_response(
            200,
            r#"{"errorCode":"0","translation":["你好"],"l":"EN2zh-CHS"}"#,
            &input,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({"translations":["你好"],"sourceLanguage":"en","targetLanguage":"zh-CHS","unchanged":false})
        );
        let unknown = parse_response(
            200,
            r#"{"errorCode":"0","translation":["你好"],"l":"fr2zh-CHS"}"#,
            &input,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(unknown).unwrap()["sourceLanguage"],
            serde_json::Value::Null
        );
        assert_eq!(
            parse_response(
                200,
                r#"{"errorCode":"0","translation":["hi"],"l":"zh-CHS2EN"}"#,
                &input
            ),
            Err("networkResponseInvalid")
        );
    }
    #[test]
    fn input_is_bounded_and_direction_is_fixed() {
        for (text, from, to) in [
            ("".into(), "en", "zh-CHS"),
            ("x".repeat(8193), "en", "zh-CHS"),
            ("hello".into(), "fr", "en"),
            ("你好".into(), "auto", "auto"),
        ] {
            assert_eq!(
                YoudaoInput {
                    text,
                    from: from.into(),
                    to: to.into()
                }
                .validate(),
                Err("invalidServiceRequest")
            );
        }
    }
    #[test]
    fn provider_errors_are_static_and_responses_are_normalized() {
        let input: YoudaoInput =
            serde_json::from_value(serde_json::json!({"text":"hello","from":"en","to":"zh-CHS"}))
                .unwrap();
        let parse_response = |status, body| super::parse_response(status, body, &input);

        assert_eq!(
            parse_response(200, r#"{"errorCode":"202","secret":"never returned"}"#),
            Err("providerAuthentication")
        );
        assert_eq!(
            parse_response(200, r#"{"errorCode":"401"}"#),
            Err("providerQuota")
        );
        assert_eq!(
            parse_response(
                200,
                r#"{"errorCode":"0","translation":["你好"],"raw":"discard"}"#
            )
            .unwrap()
            .translations,
            vec!["你好"]
        );
        assert_eq!(parse_response(302, ""), Err("providerFailure"));
    }
    #[test]
    fn signing_uses_character_truncation_and_fixed_target() {
        let input = YoudaoInput {
            text: "你".repeat(21),
            from: "zh-CHS".into(),
            to: "en".into(),
        };
        let values = BTreeMap::from([
            ("appId".into(), "synthetic-id".into()),
            ("appKey".into(), "synthetic-secret".into()),
        ]);
        let request = input.request(&values, "salt", "123").unwrap();
        assert_eq!(request.url, "https://openapi.youdao.com/api");
        let Some(PluginNetworkRequestBody::Form { value }) = request.body else {
            panic!()
        };
        assert_eq!(value["strict"], "true");
        assert_eq!(
            value["sign"],
            format!(
                "{:x}",
                Sha256::digest(format!(
                    "synthetic-id{}21{}salt123synthetic-secret",
                    "你".repeat(10),
                    "你".repeat(10)
                ))
            )
        );
        assert!(!value.values().any(|v| v.contains("synthetic-secret")));
    }
}
