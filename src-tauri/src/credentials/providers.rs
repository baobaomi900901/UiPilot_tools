use super::{Error, Record, Service};
use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    Client, Request,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::Duration;
use windows::{
    core::PCWSTR,
    Win32::Security::Cryptography::{
        BCryptCloseAlgorithmProvider, BCryptHash, BCryptOpenAlgorithmProvider, BCRYPT_ALG_HANDLE,
        BCRYPT_MD5_ALGORITHM, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
    },
};

const MAX_BODY: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(12);

fn validation_client() -> Result<Client, Error> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| Error::network())
}
pub(super) async fn verify(record: Record) -> Result<(), Error> {
    let client = validation_client()?;
    let salt = format!(
        "{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    );
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let request = build_request(&client, &record, &salt, &timestamp)?;
    let mut response = client
        .execute(request)
        .await
        .map_err(|_| Error::network())?;
    let status = response.status().as_u16();
    if status == 401 || status == 403 {
        return Err(Error::invalid_credentials());
    }
    if !(200..300).contains(&status) && status != 400 {
        return Err(Error::validation());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err(Error::validation());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::network())? {
        append_chunk(&mut body, &chunk)?;
    }
    validate_response(record.service, status, &body)
}
fn append_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), Error> {
    if chunk.len() > MAX_BODY.saturating_sub(body.len()) {
        return Err(Error::validation());
    }
    body.extend_from_slice(chunk);
    Ok(())
}
fn build_request(
    client: &Client,
    record: &Record,
    salt: &str,
    timestamp: &str,
) -> Result<Request, Error> {
    let field = |name: &str| {
        record
            .values
            .get(name)
            .map(String::as_str)
            .ok_or_else(Error::invalid_input)
    };
    let query = "uipilot";
    let request = match record.service {
        Service::YoudaoTranslation => {
            let sign = format!(
                "{:x}",
                Sha256::digest(format!(
                    "{}{query}{salt}{timestamp}{}",
                    field("appId")?,
                    field("appKey")?
                ))
            );
            client.post("https://openapi.youdao.com/api").form(&[
                ("q", query),
                ("from", "auto"),
                ("to", "zh"),
                ("appKey", field("appId")?),
                ("salt", salt),
                ("signType", "v3"),
                ("curtime", timestamp),
                ("sign", sign.as_str()),
            ])
        }
        Service::BaiduTranslation => {
            let sign = md5_hex(
                format!("{}{query}{salt}{}", field("appId")?, field("appKey")?).as_bytes(),
            )?;
            client
                .post("https://fanyi-api.baidu.com/api/trans/vip/translate")
                .form(&[
                    ("q", query),
                    ("from", "auto"),
                    ("to", "zh"),
                    ("appid", field("appId")?),
                    ("salt", salt),
                    ("sign", sign.as_str()),
                ])
        }
        Service::BaiduOcr => client
            .post("https://aip.baidubce.com/oauth/2.0/token")
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", field("apiKey")?),
                ("client_secret", field("secretKey")?),
            ]),
        Service::Deepseek => {
            let mut authorization = HeaderValue::from_str(&format!("Bearer {}", field("apiKey")?))
                .map_err(|_| Error::invalid_input())?;
            authorization.set_sensitive(true);
            client
                .get("https://api.deepseek.com/models")
                .header(AUTHORIZATION, authorization)
        }
    };
    request
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| Error::invalid_input())
}
fn md5_hex(bytes: &[u8]) -> Result<String, Error> {
    let mut algorithm = BCRYPT_ALG_HANDLE::default();
    unsafe {
        BCryptOpenAlgorithmProvider(
            &mut algorithm,
            BCRYPT_MD5_ALGORITHM,
            PCWSTR::null(),
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        )
    }
    .ok()
    .map_err(|_| Error::validation())?;
    let mut digest = [0u8; 16];
    let result = unsafe { BCryptHash(algorithm, None, bytes, &mut digest) }.ok();
    let closed = unsafe { BCryptCloseAlgorithmProvider(algorithm, 0) }.ok();
    result.and(closed).map_err(|_| Error::validation())?;
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn validate_response(service: Service, status: u16, bytes: &[u8]) -> Result<(), Error> {
    if status == 401 || status == 403 {
        return Err(Error::invalid_credentials());
    }
    if (!(200..300).contains(&status) && status != 400) || bytes.len() > MAX_BODY {
        return Err(Error::validation());
    }
    let body: Value = serde_json::from_slice(bytes).map_err(|_| Error::validation())?;
    let valid = match service {
        Service::YoudaoTranslation => {
            let code = body.get("errorCode").and_then(Value::as_str);
            if matches!(
                code,
                Some("101" | "108" | "110" | "111" | "112" | "202" | "203")
            ) {
                return Err(Error::invalid_credentials());
            }
            code == Some("0")
                && nonempty_array(&body["translation"], |value| {
                    value.as_str().is_some_and(|text| !text.is_empty())
                })
        }
        Service::BaiduTranslation => {
            let code = body.get("error_code").map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            });
            if matches!(code.as_deref(), Some("52003" | "54001")) {
                return Err(Error::invalid_credentials());
            }
            code.is_none()
                && nonempty_array(&body["trans_result"], |value| {
                    value["src"].is_string()
                        && value["dst"].as_str().is_some_and(|text| !text.is_empty())
                })
        }
        Service::BaiduOcr => {
            if matches!(
                body.get("error").and_then(Value::as_str),
                Some("invalid_client" | "invalid_grant")
            ) {
                return Err(Error::invalid_credentials());
            }
            body.get("error").is_none()
                && body["access_token"]
                    .as_str()
                    .is_some_and(|token| !token.is_empty() && token.len() <= 8192)
                && body["expires_in"].as_u64().is_some_and(|value| value > 0)
                && body["scope"].as_str().is_some_and(|scope| {
                    scope.split_ascii_whitespace().any(|entry| {
                        entry == "brain_ocr"
                            || entry == "vis-ocr"
                            || entry.starts_with("brain_ocr_")
                            || entry.starts_with("vis-ocr_")
                    })
                })
        }
        Service::Deepseek => {
            body["object"].as_str() == Some("list")
                && body["data"].as_array().is_some_and(|data| {
                    data.iter().all(|model| {
                        model["object"].as_str() == Some("model")
                            && model["id"].as_str().is_some_and(|id| !id.is_empty())
                    })
                })
        }
    };
    if valid && (200..300).contains(&status) {
        Ok(())
    } else {
        Err(Error::validation())
    }
}
fn nonempty_array(value: &Value, predicate: impl Fn(&Value) -> bool) -> bool {
    value
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(predicate))
}
#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
