#[tokio::test]
async fn share_prepare_native_network_and_credentials_deny_before_secret_usage_and_image_claim() {
    use super::super::share_prepare::{SharePrepareLease, SharePrepareOwner};
    let dir = TestDir::new("share-prepare-effects");
    write_host_service_package(&dir);
    let path = dir.source().join("plugin.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["minimumHostVersion"] = json!("0.22.0");
    value["command"]["activationMode"] = json!("submit");
    value["command"]["outputMode"] = json!("panel");
    value["command"]["inputCapabilities"] = json!(["image.ocr"]);
    value["panel"] = json!({"entry":"dist/panel.html","runtimeActions":true});
    value["permissions"] = json!([
        "ui.panel",
        "network.https",
        "credentials.youdaoTranslation",
        "credentials.baiduOcr"
    ]);
    value["network"]["httpsHosts"] =
        json!(["openapi.youdao.com", "aip.baidubce.com", "api.example.com"]);
    value["settings"] = json!([{"type":"secret","key":"token","label":"Token"}]);
    value["network"]["credentials"] =
        json!([{"id":"service","settingKey":"token","host":"api.example.com","type":"bearer"}]);
    value["share"] = json!({"version":1,"receivers":[{"id":"receive","title":"Receive","commandId":"default","types":["text/plain"],"maxBytes":8192}]});
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    fs::write(dir.source().join("dist/panel.html"), "<!doctype html>").unwrap();
    let (gate, _started) = PluginNetworkAuthorityGate::blocking_for_test();
    let manager = manager_with_network_authority(&dir, gate.clone());
    let now = Instant::now();
    let prepared = manager.prepare("main", source(&dir.source()), now).unwrap();
    manager
        .commit_with_readiness(
            "main",
            &prepared.token,
            BTreeSet::from([PublicPermission::UiPanel, PublicPermission::NetworkHttps]),
            now,
            |_| true,
        )
        .unwrap();
    for permission in [
        PublicPermission::CredentialsYoudaoTranslation,
        PublicPermission::CredentialsBaiduOcr,
    ] {
        manager
            .set_permission_access_with_effects(
                "com.example.network",
                permission,
                true,
                &manager.state.inventory_revision().unwrap().to_string(),
                &mut ManagementEffects::default(),
            )
            .unwrap();
    }
    let receiver = manager.share_catalog().unwrap().remove(0);
    let owner = SharePrepareOwner {
        receiver,
        prepare_id: "host-transfer".into(),
        lease: SharePrepareLease::default(),
    };
    let request = manager.admit_share_prepare(&owner, now).unwrap();
    let caller = PluginRuntimeIdentity {
        plugin_id: request.context.plugin_id.clone(),
        generation: request.context.plugin_generation,
    };
    let store = crate::credentials::CredentialStore::new(dir.path().join("synthetic-unused"));
    // Empty credential stores make ordering observable: denial must precede credentialNotConfigured.
    for target in ["zh-CHS", "en"] {
        let input = serde_json::from_value(
            json!({"context":request.context,"input":{"text":"hello","from":"en","to":target}}),
        )
        .unwrap();
        assert_eq!(
            manager
                .youdao_translate(caller.clone(), input, &store)
                .await
                .unwrap_err()
                .code,
            "servicePermissionDenied"
        );
    }
    assert_eq!(
        manager
            .baidu_ocr(
                caller.clone(),
                baidu_input(&request.context, "missing-image"),
                &store
            )
            .await
            .unwrap_err()
            .code,
        "servicePermissionDenied"
    );
    let input: PluginNetworkCommandInput = serde_json::from_value(json!({"context":request.context,"request":{"url":"https://api.example.com/test","method":"GET","credential":"service"}})).unwrap();
    assert_eq!(
        manager.request_network(caller.clone(), input.clone()).await,
        Err(PluginNetworkErrorCode::PermissionDenied)
    );
    assert_eq!(gate.global_active_for_test(), 0);
    // A plugin-supplied serialized entry cannot regain the retired native authority.
    owner.lease.cancel();
    assert_eq!(
        manager.request_network(caller, input).await,
        Err(PluginNetworkErrorCode::ExpiredRequest)
    );
}
