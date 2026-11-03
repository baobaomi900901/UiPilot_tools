use super::*;
use serde_json::json;
use std::collections::BTreeMap;
fn record(service: Service) -> Record {
    Record {
        service,
        name: "Synthetic".into(),
        enabled: false,
        values: service
            .fields()
            .iter()
            .map(|field| ((*field).into(), format!("synthetic-{field}")))
            .collect(),
    }
}
#[test]
fn success_shapes_cover_all_four_services_and_reject_malformed_payloads() {
    let success = [
        (
            Service::YoudaoTranslation,
            json!({"errorCode":"0", "translation":["测试"]}),
        ),
        (
            Service::BaiduTranslation,
            json!({"trans_result":[{"src":"uipilot", "dst":"测试"}]}),
        ),
        (
            Service::BaiduOcr,
            json!({"access_token":"synthetic-token", "expires_in":3600,"scope":"brain_ocr_general basic"}),
        ),
        (
            Service::Deepseek,
            json!({"object":"list", "data":[{"id":"synthetic-model", "object":"model"}]}),
        ),
    ];
    for (service, body) in success {
        assert!(validate_response(service, 200, &serde_json::to_vec(&body).unwrap()).is_ok());
        assert_eq!(
            validate_response(service, 200, b"{}").unwrap_err().code,
            "validationFailed"
        );
        assert_eq!(
            validate_response(service, 302, b"{}").unwrap_err().code,
            "validationFailed"
        );
        assert_eq!(
            validate_response(service, 401, b"synthetic-secret-in-response")
                .unwrap_err()
                .code,
            "invalidCredentials"
        );
        assert_eq!(
            validate_response(service, 429, b"synthetic-secret-in-response")
                .unwrap_err()
                .code,
            "validationFailed"
        );
        assert!(validate_response(service, 200, &vec![b'x'; MAX_BODY + 1]).is_err());
    }
}
#[test]
fn auth_errors_are_distinct_from_quota_and_ocr_scope_failure() {
    for (service, invalid, quota) in [
        (
            Service::YoudaoTranslation,
            json!({"errorCode":"202"}),
            json!({"errorCode":"411"}),
        ),
        (
            Service::BaiduTranslation,
            json!({"error_code":"54001"}),
            json!({"error_code":"54004"}),
        ),
        (
            Service::BaiduOcr,
            json!({"error":"invalid_client"}),
            json!({"access_token":"synthetic","expires_in":3600,"scope":"basic"}),
        ),
    ] {
        assert_eq!(
            validate_response(service, 200, &serde_json::to_vec(&invalid).unwrap())
                .unwrap_err()
                .code,
            "invalidCredentials"
        );
        assert_eq!(
            validate_response(service, 200, &serde_json::to_vec(&quota).unwrap())
                .unwrap_err()
                .code,
            "validationFailed"
        );
    }
}
#[test]
fn fixed_endpoints_keep_keys_out_of_urls_and_use_correct_signatures() {
    let client = validation_client().unwrap();
    for (service, url) in [
        (Service::YoudaoTranslation, "https://openapi.youdao.com/api"),
        (
            Service::BaiduTranslation,
            "https://fanyi-api.baidu.com/api/trans/vip/translate",
        ),
        (
            Service::BaiduOcr,
            "https://aip.baidubce.com/oauth/2.0/token",
        ),
        (Service::Deepseek, "https://api.deepseek.com/models"),
    ] {
        let request = build_request(&client, &record(service), "salt", "100").unwrap();
        assert_eq!(request.url().as_str(), url);
        assert!(request.timeout().is_some());
        if service == Service::Deepseek {
            assert_eq!(request.method(), reqwest::Method::GET);
            assert_eq!(
                request.headers()[reqwest::header::AUTHORIZATION],
                "Bearer synthetic-apiKey"
            );
            assert!(request.headers()[reqwest::header::AUTHORIZATION].is_sensitive());
        } else {
            assert_eq!(request.method(), reqwest::Method::POST);
            let fields: BTreeMap<_, _> =
                url::form_urlencoded::parse(request.body().unwrap().as_bytes().unwrap())
                    .into_owned()
                    .collect();
            if service == Service::BaiduTranslation {
                assert_eq!(
                    fields["sign"],
                    md5_hex(b"synthetic-appIduipilotsaltsynthetic-appKey").unwrap()
                );
            }
            if service == Service::YoudaoTranslation {
                assert_eq!(
                    fields["sign"],
                    format!(
                        "{:x}",
                        Sha256::digest(b"synthetic-appIduipilotsalt100synthetic-appKey")
                    )
                );
            }
            if service == Service::BaiduOcr {
                assert_eq!(fields["grant_type"], "client_credentials");
            }
        }
    }
    assert_eq!(md5_hex(b"abc").unwrap(), "900150983cd24fb0d6963f7d28e17f72");
}
#[test]
fn body_limit_is_enforced_before_extending_buffer() {
    let mut bytes = vec![0; MAX_BODY - 1];
    append_chunk(&mut bytes, b"x").unwrap();
    assert_eq!(bytes.len(), MAX_BODY);
    assert!(append_chunk(&mut bytes, b"x").is_err());
    assert_eq!(bytes.len(), MAX_BODY);
}

#[test]
fn oauth_invalid_client_on_http_400_remains_an_authentication_error() {
    assert_eq!(
        validate_response(Service::BaiduOcr, 400, br#"{"error":"invalid_client"}"#)
            .unwrap_err()
            .code,
        "invalidCredentials"
    );
}

#[test]
fn official_ocr_scope_variants_are_accepted_but_generic_scope_is_not() {
    for scope in ["vis-ocr_all", "vis-ocr", "brain_ocr", "brain_ocr_general"] {
        assert!(validate_response(
            Service::BaiduOcr,
            200,
            &serde_json::to_vec(
                &json!({"access_token":"synthetic", "expires_in":3600, "scope":scope})
            )
            .unwrap()
        )
        .is_ok());
    }
    assert_eq!(validate_response(Service::BaiduOcr, 200, &serde_json::to_vec(&json!({"access_token":"synthetic", "expires_in":3600, "scope":"basic brain_all_scope"})).unwrap()).unwrap_err().code, "validationFailed");
}
