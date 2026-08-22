use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{json, Value};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use super::{
    manifest::{PublicOutputMode, PublicPermission},
    package, stage_public_package,
    webview_audio_guard::{INERT_DOCUMENT, INERT_PATH},
    PublicPackageError, PublicPackageSource, PublicPlatform, PublicPluginHost, PublicPluginService,
};
use crate::plugins::{PluginCatalog, Version};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[test]
fn public_plugin_icon_protocol_rejects_query_parameters() {
    let service = PublicPluginService::default();
    let response = service.asset_response(
        "main",
        "/__uipilot_icon/installed/com.example.demo/1/icon.png",
        Some("cache=bust"),
    );
    assert_eq!(response.status(), 403);
}

#[test]
fn public_plugin_alarm_protocol_is_always_forbidden() {
    let service = PublicPluginService::default();
    for label in [
        "public-runtime-com.example.timer-g1",
        "public-plugin-content-com.example.timer",
    ] {
        let response = service.asset_response(label, "/assets/sounds/timer-alarm.wav", None);
        assert_eq!(response.status(), 403);
        assert!(response.body().is_empty());
    }
}

#[test]
fn inert_webview_document_is_host_owned_and_denies_media() {
    let service = PublicPluginService::default();
    let response = service.asset_response("untrusted-label", &format!("/{INERT_PATH}"), None);

    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), INERT_DOCUMENT.as_bytes());
    assert_eq!(
        response.headers()["content-security-policy"],
        "default-src 'none'; media-src 'none'; base-uri 'none'; form-action 'none'"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[test]
fn public_plugin_protocol_csp_denies_media() {
    assert!(super::PUBLIC_PLUGIN_CSP.contains("media-src 'none'"));
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(name: &str) -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uipilot-public-package-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn package(&self) -> PathBuf {
        let path = self.0.join("source");
        fs::create_dir(&path).unwrap();
        path
    }

    fn staging(&self) -> PathBuf {
        self.0.join("staging")
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn accepts_archive_and_directory_packages() {
    for (name, archive, mode) in [
        ("directory-window", false, "window"),
        ("archive-main", true, "mainResult"),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &manifest(mode));
        let source = if archive {
            let path = root.0.join("candidate.uipilot-plugin");
            archive_directory(&source, &path);
            PublicPackageSource::Archive(path)
        } else {
            PublicPackageSource::DevelopmentDirectory(source)
        };

        let prepared = stage_public_package(source, &root.staging(), &host()).unwrap();
        assert_eq!(prepared.manifest.plugin_id, "com.uipilot.demo");
        assert_eq!(prepared.digest.len(), 64);
        assert_eq!(prepared.resources["plugin.json"].mime, "application/json");
        assert_eq!(
            prepared.resources["dist/runtime.js"].mime,
            "text/javascript"
        );
        assert_eq!(prepared.revalidate(), Ok(()));
        if archive {
            let runtime = prepared.package_root.join("dist/runtime.js");
            package::make_file_writable(&runtime);
            fs::write(&runtime, "tampered").unwrap();
            assert_eq!(
                prepared.revalidate(),
                Err(PublicPackageError::InvalidPackage)
            );
        }
        assert_eq!(
            prepared.manifest.command.output_mode,
            if mode == "window" {
                PublicOutputMode::Window
            } else {
                PublicOutputMode::MainResult
            }
        );
        let transaction = prepared.transaction_root().to_path_buf();
        assert!(prepared.package_root.starts_with(&transaction));
        drop(prepared);
        assert!(!transaction.exists());
    }
}

#[test]
fn accepts_timer_alarm_in_directory_and_archive_without_exposing_it_publicly() {
    for (name, archive) in [("timer-directory", false), ("timer-archive", true)] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &timer_manifest());
        let alarm = source.join("assets/sounds/timer-alarm.wav");
        fs::create_dir_all(alarm.parent().unwrap()).unwrap();
        fs::write(&alarm, valid_alarm_wav()).unwrap();
        let source = if archive {
            let path = root.0.join("candidate.uipilot-plugin");
            archive_directory(&source, &path);
            PublicPackageSource::Archive(path)
        } else {
            PublicPackageSource::DevelopmentDirectory(source)
        };

        let prepared = stage_public_package(source, &root.staging(), &host()).unwrap();

        assert_eq!(prepared.revalidate(), Ok(()));
        assert!(!prepared
            .resources
            .contains_key("assets/sounds/timer-alarm.wav"));
    }
}

#[test]
fn accepts_supported_alarm_pcm_boundaries() {
    for (name, wav) in [
        ("mono-24-padding", alarm_wav(1, 1, 44_100, 24)),
        ("stereo-48k-24", alarm_wav(100, 2, 48_000, 24)),
        (
            "max-duration-mono-24",
            alarm_wav(44_100 * 15, 1, 44_100, 24),
        ),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &timer_manifest());
        write_alarm(&source, &wav);
        let prepared = stage_public_package(
            PublicPackageSource::DevelopmentDirectory(source),
            &root.staging(),
            &host(),
        )
        .unwrap();
        assert!(prepared.alarm.is_some());
        assert_eq!(prepared.revalidate(), Ok(()));
    }
}

#[test]
fn timer_permission_and_fixed_alarm_must_be_declared_together() {
    let missing = TestRoot::new("timer-alarm-missing");
    let source = missing.package();
    write_package(&source, &timer_manifest());
    assert_rejected(
        PublicPackageSource::DevelopmentDirectory(source),
        &missing,
        PublicPackageError::InvalidPackage,
    );

    let unexpected = TestRoot::new("timer-alarm-unexpected");
    let source = unexpected.package();
    write_package(&source, &manifest("window"));
    write_alarm(&source, &valid_alarm_wav());
    assert_rejected(
        PublicPackageSource::DevelopmentDirectory(source),
        &unexpected,
        PublicPackageError::InvalidPackage,
    );
}

#[test]
fn rejects_malformed_or_unsupported_timer_alarm_wav() {
    let mut bad_riff = valid_alarm_wav();
    bad_riff[0] = b'X';
    let mut trailing = valid_alarm_wav();
    trailing.push(0);
    let mut non_pcm = valid_alarm_wav();
    non_pcm[20..22].copy_from_slice(&3_u16.to_le_bytes());
    let mut channels = valid_alarm_wav();
    channels[22..24].copy_from_slice(&3_u16.to_le_bytes());
    let mut sample_rate = valid_alarm_wav();
    sample_rate[24..28].copy_from_slice(&22_050_u32.to_le_bytes());
    let mut byte_rate = valid_alarm_wav();
    byte_rate[28..32].copy_from_slice(&1_u32.to_le_bytes());
    let mut block_align = valid_alarm_wav();
    block_align[32..34].copy_from_slice(&1_u16.to_le_bytes());
    let mut bits = valid_alarm_wav();
    bits[34..36].copy_from_slice(&8_u16.to_le_bytes());
    let mut unknown_chunk = valid_alarm_wav();
    unknown_chunk[36..40].copy_from_slice(b"JUNK");
    let mut extended_fmt = valid_alarm_wav();
    extended_fmt[16..20].copy_from_slice(&18_u32.to_le_bytes());
    let mut mismatched_data_length = valid_alarm_wav();
    mismatched_data_length[40..44].copy_from_slice(&199_u32.to_le_bytes());
    let mut even_padding = valid_alarm_wav();
    even_padding.push(0);
    let riff_size = u32::try_from(even_padding.len() - 8).unwrap();
    even_padding[4..8].copy_from_slice(&riff_size.to_le_bytes());
    let mut bad_padding = alarm_wav(1, 1, 44_100, 24);
    *bad_padding.last_mut().unwrap() = 1;
    let mut missing_padding = alarm_wav(1, 1, 44_100, 24);
    missing_padding.pop();
    let zero_frames = alarm_wav(0, 1, 44_100, 16);
    let too_long = alarm_wav(44_100 * 15 + 1, 1, 44_100, 16);

    for (name, wav) in [
        ("bad-riff", bad_riff),
        ("trailing", trailing),
        ("non-pcm", non_pcm),
        ("channels", channels),
        ("sample-rate", sample_rate),
        ("byte-rate", byte_rate),
        ("block-align", block_align),
        ("bits", bits),
        ("unknown-chunk", unknown_chunk),
        ("extended-fmt", extended_fmt),
        ("mismatched-data-length", mismatched_data_length),
        ("even-padding", even_padding),
        ("bad-padding", bad_padding),
        ("missing-padding", missing_padding),
        ("zero-frames", zero_frames),
        ("too-long", too_long),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &timer_manifest());
        write_alarm(&source, &wav);
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }
}

#[test]
fn source_alarm_hardlink_is_copied_but_staged_multilink_is_rejected() {
    let root = TestRoot::new("timer-alarm-hardlink");
    let source = root.package();
    write_package(&source, &timer_manifest());
    let original = root.0.join("original.wav");
    fs::write(&original, valid_alarm_wav()).unwrap();
    let alarm = source.join("assets/sounds/timer-alarm.wav");
    fs::create_dir_all(alarm.parent().unwrap()).unwrap();
    fs::hard_link(&original, &alarm).unwrap();

    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &host(),
    )
    .unwrap();
    fs::write(&original, alarm_wav(101, 1, 44_100, 16)).unwrap();
    assert_eq!(prepared.revalidate(), Ok(()));

    let staged_alarm = prepared.package_root.join("assets/sounds/timer-alarm.wav");
    fs::hard_link(staged_alarm, root.0.join("staged-alarm-link.wav")).unwrap();
    assert_eq!(
        prepared.revalidate(),
        Err(PublicPackageError::InvalidPackage)
    );
}

#[test]
fn pomodoro_reference_package_is_installable() {
    let root = TestRoot::new("pomodoro-reference");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/public-plugins/com.uipilot.pomodoro/package");
    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &host(),
    )
    .unwrap();

    assert_eq!(prepared.manifest.plugin_id, "com.uipilot.pomodoro");
    assert_eq!(
        prepared.manifest.permissions,
        vec![
            PublicPermission::UiWindow,
            PublicPermission::NotificationsPublish,
            PublicPermission::TimerControl,
        ]
    );
    assert_eq!(prepared.revalidate(), Ok(()));
}

#[test]
fn rejects_resource_and_archive_path_variants() {
    for path in ["dist/icon.png", "dist/runtime.min.js", "dist/data.json"] {
        let root = TestRoot::new(path.replace(['/', '.'], "-").as_str());
        let source = root.package();
        write_package(&source, &manifest("mainResult"));
        let extra = path
            .split('/')
            .fold(source.clone(), |root, part| root.join(part));
        fs::create_dir_all(extra.parent().unwrap()).unwrap();
        fs::write(extra, "x").unwrap();
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }

    for (name, entries) in [
        (
            "traversal",
            vec![
                ("../escape.js", b"x".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
        (
            "case-collision",
            vec![
                ("dist/runtime.js", b"x".as_slice()),
                ("dist/Runtime.js", b"y".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
        (
            "parent-case-collision",
            vec![
                ("Dist/a.js", b"x".as_slice()),
                ("dist/b.js", b"y".as_slice()),
                ("plugin.json", b"{}".as_slice()),
            ],
        ),
    ] {
        let root = TestRoot::new(name);
        let archive = root.0.join("candidate.uipilot-plugin");
        write_archive(&archive, &entries);
        assert_rejected(
            PublicPackageSource::Archive(archive),
            &root,
            PublicPackageError::InvalidPackage,
        );
        assert!(!root.0.join("escape.js").exists());
    }
}

#[test]
fn rejects_incompatible_or_malformed_and_preserves_legacy_loader() {
    for (name, field, replacement, expected) in [
        (
            "platform",
            "supportedPlatforms",
            json!(["macos"]),
            PublicPackageError::IncompatiblePlatform,
        ),
        (
            "api",
            "apiVersion",
            json!(2),
            PublicPackageError::IncompatibleApi,
        ),
        (
            "permission",
            "permissions",
            json!(["network.https"]),
            PublicPackageError::UnsupportedPermission,
        ),
        (
            "settings",
            "settings",
            json!([{"key":"limit","type":"number","label":"Limit","min":10,"max":5}]),
            PublicPackageError::InvalidPackage,
        ),
    ] {
        let root = TestRoot::new(name);
        let source = root.package();
        let mut candidate = manifest("mainResult");
        candidate[field] = replacement;
        write_package(&source, &candidate);
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            expected,
        );
    }

    let root = TestRoot::new("legacy");
    let legacy = root.0.join("internal.sample");
    fs::create_dir(&legacy).unwrap();
    fs::write(
        legacy.join("plugin.json"),
        r#"{"manifest":1,"id":"internal.sample","version":"1.0.0","minHostVersion":"0.2.0","runtime":"index.html","feature":{"id":"calculate","trigger":"/sample"},"permissions":["clipboard.writeText"]}"#,
    )
    .unwrap();
    fs::write(legacy.join("index.html"), "").unwrap();
    assert_eq!(
        PluginCatalog::load(&root.0, Version::new(0, 2, 0))
            .unwrap()
            .entry_count_for_test(),
        1
    );
}

#[test]
fn notifications_publish_permission_is_available_only_on_windows() {
    for (label, platform, expected) in [
        ("notifications-windows", PublicPlatform::Windows, Ok(())),
        (
            "notifications-macos",
            PublicPlatform::Macos,
            Err(PublicPackageError::UnsupportedPermission),
        ),
    ] {
        let root = TestRoot::new(label);
        let source = root.package();
        let mut candidate = manifest("mainResult");
        candidate["supportedPlatforms"] = json!(["windows", "macos"]);
        candidate["permissions"] = json!(["notifications.publish"]);
        write_package(&source, &candidate);
        let result = stage_public_package(
            PublicPackageSource::DevelopmentDirectory(source),
            &root.staging(),
            &PublicPluginHost::current(platform),
        )
        .map(|_| ());
        assert_eq!(result, expected);
    }
}

fn host() -> PublicPluginHost {
    PublicPluginHost::current(PublicPlatform::Windows)
}

fn manifest(mode: &str) -> Value {
    let window = mode == "window";
    let mut value = json!({
        "schemaVersion":1,
        "pluginId":"com.uipilot.demo",
        "version":"1.0.0",
        "apiVersion":1,
        "minimumHostVersion":"0.2.0",
        "name":"Demo",
        "supportedPlatforms":["windows"],
        "command":{
            "defaultName":"demo",
            "activationMode":if window { "submit" } else { "live" },
            "outputMode":mode,
            "inputRequired":false
        },
        "runtime":{"entry":"dist/runtime.js"},
        "permissions":if window { json!(["ui.window"]) } else { json!([]) }
    });
    if window {
        value["window"] = json!({"entry":"dist/window.html"});
    }
    value
}

fn timer_manifest() -> Value {
    let mut value = manifest("window");
    value["permissions"] = json!(["ui.window", "notifications.publish", "timer.control"]);
    value
}

fn valid_alarm_wav() -> Vec<u8> {
    alarm_wav(100, 1, 44_100, 16)
}

fn alarm_wav(frames: u32, channels: u16, sample_rate: u32, bits_per_sample: u16) -> Vec<u8> {
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data = vec![0_u8; usize::from(block_align) * usize::try_from(frames).unwrap()];
    let padding = data.len() % 2;
    let riff_size = 36_u32 + u32::try_from(data.len() + padding).unwrap();
    let mut wav = Vec::with_capacity(44 + data.len() + padding);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    wav.extend_from_slice(&data);
    if padding == 1 {
        wav.push(0);
    }
    wav
}

fn write_alarm(root: &Path, bytes: &[u8]) {
    let alarm = root.join("assets/sounds/timer-alarm.wav");
    fs::create_dir_all(alarm.parent().unwrap()).unwrap();
    fs::write(alarm, bytes).unwrap();
}

fn write_package(root: &Path, manifest: &Value) {
    fs::create_dir(root.join("dist")).unwrap();
    fs::write(
        root.join("plugin.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("dist/runtime.js"),
        "export function onCommand() {}",
    )
    .unwrap();
    if manifest["command"]["outputMode"] == "window" {
        fs::write(root.join("dist/window.html"), "<!doctype html>").unwrap();
    }
}

fn assert_rejected(source: PublicPackageSource, root: &TestRoot, expected: PublicPackageError) {
    assert_eq!(
        stage_public_package(source, &root.staging(), &host()).unwrap_err(),
        expected
    );
    assert!(!root.staging().exists() || fs::read_dir(root.staging()).unwrap().next().is_none());
}

fn archive_directory(source: &Path, destination: &Path) {
    let mut entries = Vec::new();
    collect_files(source, source, &mut entries);
    let borrowed = entries
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    write_archive(destination, &borrowed);
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(root, &path, output);
        } else {
            output.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(path).unwrap(),
            ));
        }
    }
}

fn write_archive(destination: &Path, entries: &[(&str, &[u8])]) {
    let mut archive = ZipWriter::new(File::create(destination).unwrap());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (path, bytes) in entries {
        archive.start_file(*path, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn png_with_dimensions(source: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut png = source.to_vec();
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}

fn png_with_animation_control(source: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(20);
    chunk.extend_from_slice(&8_u32.to_be_bytes());
    chunk.extend_from_slice(b"acTL");
    chunk.extend_from_slice(&1_u32.to_be_bytes());
    chunk.extend_from_slice(&0_u32.to_be_bytes());
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut png = Vec::with_capacity(source.len() + chunk.len());
    png.extend_from_slice(&source[..33]);
    png.extend_from_slice(&chunk);
    png.extend_from_slice(&source[33..]);
    png
}

#[test]
fn public_plugin_icon_validation_is_fixed_bounded_and_atomic() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let valid_icon =
        fs::read(workspace.join("examples/public-plugins/com.uipilot.demo-win/package/icon.png"))
            .unwrap();

    let root = TestRoot::new("valid-icon");
    let source = root.package();
    write_package(&source, &manifest("mainResult"));
    fs::write(source.join("icon.png"), &valid_icon).unwrap();
    let prepared = stage_public_package(
        PublicPackageSource::DevelopmentDirectory(source),
        &root.staging(),
        &host(),
    )
    .unwrap();
    assert_eq!(prepared.resources["icon.png"].mime, "image/png");
    assert_eq!(
        prepared.resources["icon.png"].length,
        valid_icon.len() as u64
    );
    assert_eq!(
        fs::read(prepared.package_root.join("icon.png")).unwrap(),
        valid_icon
    );

    let invalid_icons = [
        ("corrupt", b"not a png".to_vec()),
        ("wrong-size", png_with_dimensions(&valid_icon, 64, 128)),
        ("animated", png_with_animation_control(&valid_icon)),
        ("oversized", {
            let mut bytes = valid_icon.clone();
            bytes.resize(128 * 1024 + 1, 0);
            bytes
        }),
    ];
    for (name, bytes) in invalid_icons {
        let root = TestRoot::new(name);
        let source = root.package();
        write_package(&source, &manifest("mainResult"));
        fs::write(source.join("icon.png"), bytes).unwrap();
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }

    for path in ["Icon.png", "assets/icon.png", "other.png"] {
        let root = TestRoot::new(path.replace(['/', '.'], "-").as_str());
        let source = root.package();
        write_package(&source, &manifest("mainResult"));
        let icon = path
            .split('/')
            .fold(source.clone(), |parent, component| parent.join(component));
        fs::create_dir_all(icon.parent().unwrap()).unwrap();
        fs::write(icon, &valid_icon).unwrap();
        assert_rejected(
            PublicPackageSource::DevelopmentDirectory(source),
            &root,
            PublicPackageError::InvalidPackage,
        );
    }

    let root = TestRoot::new("extra-icon");
    let source = root.package();
    write_package(&source, &manifest("mainResult"));
    fs::write(source.join("icon.png"), &valid_icon).unwrap();
    fs::write(source.join("other.png"), &valid_icon).unwrap();
    assert_rejected(
        PublicPackageSource::DevelopmentDirectory(source),
        &root,
        PublicPackageError::InvalidPackage,
    );
}

#[test]
fn repository_demo_examples_stage_as_independently_removable_public_plugins() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cases = [
        (
            "com.uipilot.demo-win",
            "demo-win",
            "打开演示子窗口",
            PublicOutputMode::Window,
            "1.0.4",
            vec![
                PublicPermission::UiWindow,
                PublicPermission::NotificationsPublish,
            ],
            true,
            6,
        ),
        (
            "com.uipilot.demo-return",
            "demo-return",
            "返回示例文本到主界面",
            PublicOutputMode::MainResult,
            "1.0.2",
            vec![PublicPermission::ClipboardWrite],
            false,
            3,
        ),
    ];

    for (
        plugin_id,
        command,
        summary,
        output_mode,
        version,
        permissions,
        has_window,
        resource_count,
    ) in cases
    {
        let root = TestRoot::new(command);
        let source = workspace.join(format!("examples/public-plugins/{plugin_id}/package"));
        let prepared = stage_public_package(
            PublicPackageSource::DevelopmentDirectory(source),
            &root.staging(),
            &host(),
        )
        .unwrap();
        assert_eq!(prepared.manifest.plugin_id, plugin_id);
        assert_eq!(prepared.manifest.command.default_name, command);
        assert_eq!(prepared.manifest.command.output_mode, output_mode);
        let serialized = serde_json::to_value(&prepared.manifest).unwrap();
        assert_eq!(serialized["version"], version);
        assert_eq!(serialized["command"]["summary"], summary);
        assert_eq!(serialized["command"]["inputPlaceholder"], "请输入信息回车");
        assert_eq!(prepared.manifest.permissions, permissions);
        assert_eq!(prepared.manifest.window.is_some(), has_window);
        assert_eq!(prepared.resources.len(), resource_count);
    }

    for production in [
        include_str!("../commands.rs"),
        include_str!("../public_plugins.rs"),
        include_str!("../../../src/launcher-core.ts"),
    ] {
        for command in ["/demo-win", "/demo-return"] {
            assert!(!production.contains(command));
        }
    }
}
#[cfg(windows)]
#[test]
fn demo_packaging_script_writes_both_installable_archives() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for (plugin_id, resource_count) in [("com.uipilot.demo-win", 6), ("com.uipilot.demo-return", 3)]
    {
        let root = TestRoot::new(plugin_id);
        let output = root.0.join(format!("{plugin_id}.uipilot-plugin"));
        let status = std::process::Command::new("powershell.exe")
            .current_dir(workspace)
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "scripts/package-demo-plugin.ps1",
                "-PluginId",
                plugin_id,
                "-OutputPath",
            ])
            .arg(&output)
            .status()
            .unwrap();
        assert!(status.success());
        let prepared = stage_public_package(
            PublicPackageSource::Archive(output),
            &root.staging(),
            &host(),
        )
        .unwrap();
        assert_eq!(prepared.manifest.plugin_id, plugin_id);
        assert_eq!(prepared.resources.len(), resource_count);
    }
}
