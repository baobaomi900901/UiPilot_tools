use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const INSTANCE_NAME: &str = "UiPilot";
pub const SERVICE_NAME: &str = "Everything (UiPilot)";
pub const PIPE_NAME: &str = r"\\.\PIPE\UiPilotEverythingService";
pub const PIPE_SDDL_TEMPLATE: &str =
    "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GRGW;;;${desktop_user_sid})";
pub const MUTEX_NAME: &str = r"Global\UiPilotManagedEverythingInstaller";
pub const ACTIVITY_EVENT_NAME: &str = r"Global\UiPilotManagedEverythingInstallerActivity";
pub const MANAGED_RELATIVE_ROOT: &str = r"UiPilot_tools\everything";
pub const RUNTIME_MANIFEST_NAME: &str = "managed-runtime.json";
pub const TRANSACTION_MANIFEST_NAME: &str = "managed-transaction.json";

pub const EMBEDDED_LOCK: &[u8] =
    include_bytes!("../../../resources/everything/everything.lock.json");
pub const EMBEDDED_LICENSE: &[u8] = include_bytes!("../../../resources/everything/LICENSE.txt");
pub const EMBEDDED_INI: &[u8] =
    include_bytes!("../../../resources/everything/Everything-UiPilot.ini");

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(String);

impl OperationId {
    pub fn parse(value: &str) -> Result<Self, String> {
        let bytes = value.as_bytes();
        let valid = bytes.len() == 36
            && [8, 13, 18, 23].iter().all(|&index| bytes[index] == b'-')
            && bytes.iter().enumerate().all(|(index, byte)| {
                [8, 13, 18, 23].contains(&index)
                    || byte.is_ascii_digit()
                    || (b'a'..=b'f').contains(byte)
                    || (b'A'..=b'F').contains(byte)
            });
        if !valid || value == "00000000-0000-0000-0000-000000000000" {
            return Err("invalid-operation-id".into());
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub fn fixture(seed: u8) -> Self {
        Self(format!("{seed:08x}-0000-4000-8000-000000000001"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    PrepareInstall,
    RecoverInstall,
    CommitInstall,
    RollbackInstall,
    PreUninstall,
    PostUninstall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub action: Action,
    pub operation_id: OperationId,
    pub staging_dir: PathBuf,
    pub request_path: PathBuf,
    pub journal_path: PathBuf,
    pub parent_pid: u32,
    pub parent_creation_time: u64,
    pub install_dir: Option<PathBuf>,
    pub recovered_transaction_id: Option<OperationId>,
}

impl Request {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.parent_pid == 0 || self.parent_creation_time == 0 {
            return Err("invalid-request-header".into());
        }
        if self.request_path.parent() != Some(self.staging_dir.as_path())
            || self.journal_path.parent() != Some(self.staging_dir.as_path())
            || self.request_path == self.journal_path
        {
            return Err("invalid-staging-path".into());
        }
        match self.action {
            Action::CommitInstall if self.install_dir.is_none() => {
                Err("missing-install-dir".into())
            }
            Action::CommitInstall
            | Action::PrepareInstall
            | Action::RollbackInstall
            | Action::PreUninstall
            | Action::PostUninstall
                if self.recovered_transaction_id.is_some() =>
            {
                Err("unexpected-recovered-transaction-id".into())
            }
            Action::PrepareInstall
            | Action::RecoverInstall
            | Action::RollbackInstall
            | Action::PreUninstall
            | Action::PostUninstall
                if self.install_dir.is_some() =>
            {
                Err("unexpected-install-dir".into())
            }
            _ => Ok(()),
        }
    }

    #[cfg(test)]
    pub fn fixture(action: Action) -> Self {
        let staging_dir = PathBuf::from(r"C:\stage");
        Self {
            schema_version: 1,
            action,
            operation_id: OperationId::fixture(1),
            request_path: staging_dir.join("request.json"),
            journal_path: staging_dir.join("journal.json"),
            staging_dir,
            parent_pid: 42,
            parent_creation_time: 7,
            install_dir: (action == Action::CommitInstall)
                .then(|| PathBuf::from(r"C:\Program Files\UiPilot_tools")),
            recovered_transaction_id: (action == Action::RecoverInstall)
                .then(|| OperationId::fixture(2)),
        }
    }
}

pub fn parse_request(bytes: &[u8]) -> Result<Request, String> {
    let request: Request = serde_json::from_slice(bytes).map_err(|_| "invalid-request-json")?;
    OperationId::parse(request.operation_id.as_str())?;
    if let Some(id) = request.recovered_transaction_id.as_ref() {
        OperationId::parse(id.as_str())?;
    }
    request.validate()?;
    Ok(request)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedLock {
    pub schema_version: u32,
    pub version: String,
    pub architecture: String,
    #[serde(rename = "everythingExeSha256")]
    pub everything_exe_sha256: String,
    #[serde(rename = "authenticodePublisher")]
    pub authenticode_publisher: String,
    #[serde(flatten)]
    pub reviewed_metadata: std::collections::BTreeMap<String, serde_json::Value>,
}

pub fn embedded_lock() -> Result<ReviewedLock, String> {
    serde_json::from_slice(EMBEDDED_LOCK).map_err(|_| "invalid-embedded-lock".into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Prepared,
    JournalCreated,
    NewBinaryVerified,
    OldServiceStopped,
    OldServiceRemoved,
    NewServiceInstalled,
    NewServiceRunning,
    ManifestReplaced,
    CleanupComplete,
    RollbackStarted,
    RollbackComplete,
    UninstallServiceRemoved,
    UninstallDirectoriesRemoved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub schema_version: u32,
    pub version: String,
    pub architecture: String,
    pub sha256: String,
    pub publisher: String,
    pub instance: String,
    pub service_name: String,
    pub pipe_name: String,
    pub desktop_user_sid: String,
    pub expanded_sddl: String,
    pub service_binary_path: PathBuf,
}

impl RuntimeManifest {
    pub fn validate_fixed_identity(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.instance != INSTANCE_NAME
            || self.service_name != SERVICE_NAME
            || self.pipe_name != PIPE_NAME
            || self.architecture != "x64"
            || self.service_binary_path.file_name() != Some(std::ffi::OsStr::new("Everything.exe"))
            || !canonical_version(&self.version)
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.publisher.is_empty()
            || self.publisher.len() > 1024
            || self.publisher.chars().any(char::is_control)
        {
            return Err("foreign-runtime-identity".into());
        }
        let expected = PIPE_SDDL_TEMPLATE.replace("${desktop_user_sid}", &self.desktop_user_sid);
        if self.expanded_sddl != expected || !canonical_sid(&self.desktop_user_sid) {
            return Err("invalid-runtime-sddl".into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn fixture() -> Self {
        let sid = "S-1-5-21-1-2-3-1001".to_string();
        Self {
            schema_version: 1,
            version: "1.4.1.1032".into(),
            architecture: "x64".into(),
            sha256: "a".repeat(64),
            publisher: "voidtools".into(),
            instance: INSTANCE_NAME.into(),
            service_name: SERVICE_NAME.into(),
            pipe_name: PIPE_NAME.into(),
            expanded_sddl: PIPE_SDDL_TEMPLATE.replace("${desktop_user_sid}", &sid),
            desktop_user_sid: sid,
            service_binary_path: PathBuf::from(
                r"C:\ProgramData\UiPilot_tools\everything\1.4.1.1032\Everything.exe",
            ),
        }
    }
}

fn canonical_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionJournal {
    pub schema_version: u32,
    pub transaction_id: OperationId,
    pub phase: Phase,
    pub old_manifest: Option<RuntimeManifest>,
    pub new_manifest: RuntimeManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagingJournal {
    pub schema_version: u32,
    pub operation_id: OperationId,
    pub desktop_user_sid: String,
    pub expanded_sddl: String,
    pub old_manifest: Option<RuntimeManifest>,
    pub recovered_transaction_id: Option<OperationId>,
}

pub fn canonical_sid(value: &str) -> bool {
    value.starts_with("S-1-")
        && value.len() <= 184
        && value.split('-').skip(2).all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        })
}

fn split_windows_command_line(value: &str) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in value.chars() {
        match ch {
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted && !current.is_empty() => {
                result.push(std::mem::take(&mut current));
            }
            ' ' | '\t' if !quoted => {}
            _ => current.push(ch),
        }
    }
    if quoted {
        return None;
    }
    if !current.is_empty() {
        result.push(current);
    }
    Some(result)
}

pub fn service_arguments_match(arguments: &str, manifest: &RuntimeManifest) -> bool {
    let Some(tokens) = split_windows_command_line(arguments) else {
        return false;
    };
    let mut service = false;
    let mut instance = None;
    let mut pipe = None;
    let mut sddl = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "-svc" if !service => service = true,
            "-instance" | "-service-pipe-name" | "-service-security-descriptor"
                if index + 1 < tokens.len() =>
            {
                let slot = match tokens[index].as_str() {
                    "-instance" => &mut instance,
                    "-service-pipe-name" => &mut pipe,
                    _ => &mut sddl,
                };
                if slot.replace(tokens[index + 1].clone()).is_some() {
                    return false;
                }
                index += 1;
            }
            _ => return false,
        }
        index += 1;
    }
    service
        && instance.as_deref() == Some(INSTANCE_NAME)
        && pipe.as_deref() == Some(PIPE_NAME)
        && sddl.as_deref() == Some(manifest.expanded_sddl.as_str())
}

pub fn is_within_managed_root(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
        && path != root
        && !path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_identity_and_embedded_resources_are_exact() {
        assert_eq!(INSTANCE_NAME, "UiPilot");
        assert_eq!(SERVICE_NAME, "Everything (UiPilot)");
        assert_eq!(PIPE_NAME, r"\\.\PIPE\UiPilotEverythingService");
        assert_eq!(MUTEX_NAME, r"Global\UiPilotManagedEverythingInstaller");
        assert_eq!(
            ACTIVITY_EVENT_NAME,
            r"Global\UiPilotManagedEverythingInstallerActivity"
        );
        assert!(EMBEDDED_LOCK.starts_with(b"{"));
        assert!(EMBEDDED_LICENSE.starts_with(b"Everything"));
        assert!(EMBEDDED_INI.windows(18).any(|w| w == b"instance_name=UiPi"));
    }

    #[test]
    fn strict_request_rejects_unknown_and_duplicate_fields() {
        let valid = r#"{"schema_version":1,"action":"prepare-install","operation_id":"00112233-4455-6677-8899-aabbccddeeff","staging_dir":"C:\\stage","request_path":"C:\\stage\\request.json","journal_path":"C:\\stage\\journal.json","parent_pid":42,"parent_creation_time":7,"install_dir":null,"recovered_transaction_id":null}"#;
        assert!(parse_request(valid.as_bytes()).is_ok());
        let unknown = valid.replace(
            "\"schema_version\":1",
            "\"schema_version\":1,\"extra\":true",
        );
        assert!(parse_request(unknown.as_bytes()).is_err());
        let duplicate = valid.replace("\"parent_pid\":42", "\"parent_pid\":42,\"parent_pid\":43");
        assert!(parse_request(duplicate.as_bytes()).is_err());
    }

    #[test]
    fn operation_ids_and_action_specific_fields_are_enforced() {
        assert!(OperationId::parse("00112233-4455-6677-8899-aabbccddeeff").is_ok());
        assert!(OperationId::parse("{00112233-4455-6677-8899-aabbccddeeff}").is_err());
        assert!(OperationId::parse("00000000-0000-0000-0000-000000000000").is_err());

        let mut request = Request::fixture(Action::CommitInstall);
        request.install_dir = None;
        assert!(request.validate().is_err());
        request.install_dir = Some(r"C:\Program Files\UiPilot_tools".into());
        assert!(request.validate().is_ok());
        request.recovered_transaction_id = Some(OperationId::fixture(2));
        assert!(request.validate().is_err());
    }

    #[test]
    fn runtime_manifest_rejects_foreign_identity_and_bad_sddl() {
        let manifest = RuntimeManifest::fixture();
        assert!(manifest.validate_fixed_identity().is_ok());
        let mut foreign = manifest.clone();
        foreign.service_name = "Everything".into();
        assert!(foreign.validate_fixed_identity().is_err());
        let mut weak = manifest;
        weak.expanded_sddl = "D:(A;;GA;;;WD)".into();
        assert!(weak.validate_fixed_identity().is_err());

        let mut malformed = RuntimeManifest::fixture();
        malformed.sha256 = "A".repeat(64);
        assert!(malformed.validate_fixed_identity().is_err());
        malformed = RuntimeManifest::fixture();
        malformed.version = "../foreign".into();
        assert!(malformed.validate_fixed_identity().is_err());
        malformed = RuntimeManifest::fixture();
        malformed.publisher.clear();
        assert!(malformed.validate_fixed_identity().is_err());
    }

    #[test]
    fn semantic_service_arguments_ignore_only_quoting_and_order() {
        let manifest = RuntimeManifest::fixture();
        assert!(service_arguments_match(
            r#"-svc -instance "UiPilot" -service-pipe-name "\\.\PIPE\UiPilotEverythingService" -service-security-descriptor "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GRGW;;;S-1-5-21-1-2-3-1001)""#,
            &manifest
        ));
        assert!(!service_arguments_match(
            "-svc -instance default",
            &manifest
        ));
    }
}
