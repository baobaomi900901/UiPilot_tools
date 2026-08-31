use std::{
    collections::HashSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use png::{Decoder, Limits};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::atomic_file;

const DATA_URL_PREFIX: &str = "data:image/png;base64,";
const CONFIG_SCHEMA_VERSION: u32 = 1;
const ICON_EDGE: u32 = 128;
const MAX_ICON_BYTES: usize = 256 * 1024;

static NEXT_ICON_TOKEN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) enum QuicklinkErrorCode {
    #[serde(rename = "quicklinkLoadFailed")]
    LoadFailed,
    #[serde(rename = "quicklinkSaveFailed")]
    SaveFailed,
    #[serde(rename = "quicklinkDeleteFailed")]
    DeleteFailed,
    #[serde(rename = "quicklinkCommandConflict")]
    CommandConflict,
    #[serde(rename = "quicklinkInvalidCommand")]
    InvalidCommand,
    #[serde(rename = "quicklinkInvalidTemplate")]
    InvalidTemplate,
    #[serde(rename = "quicklinkInvalidIcon")]
    InvalidIcon,
    #[serde(rename = "quicklinkOpenFailed")]
    OpenFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuicklinkView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon_data_url: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuicklinkListResponse {
    pub(crate) items: Vec<QuicklinkView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) load_error: Option<QuicklinkErrorCode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuicklinkSaveInput {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) template: String,
    pub(crate) icon_token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuicklinkIconCandidate {
    pub(crate) token: String,
    pub(crate) data_url: String,
}

pub(crate) struct QuicklinksStore {
    root: PathBuf,
    state: Mutex<StoreState>,
}

struct StoreState {
    loaded: bool,
    document: StoredDocument,
    load_error: Option<QuicklinkErrorCode>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredDocument {
    schema_version: u32,
    next_id: u64,
    items: Vec<StoredQuicklink>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredQuicklink {
    id: String,
    name: String,
    command: String,
    template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon_asset: Option<String>,
    created_at: String,
    updated_at: String,
}

impl QuicklinksStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: Mutex::new(StoreState {
                loaded: false,
                document: StoredDocument::empty(),
                load_error: None,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn list(&self) -> QuicklinkListResponse {
        let mut state = self.state.lock().expect("quicklinks store lock poisoned");
        self.ensure_loaded_locked(&mut state);
        QuicklinkListResponse {
            items: state
                .document
                .items
                .iter()
                .map(|item| self.view_for(item))
                .collect(),
            load_error: state.load_error,
        }
    }

    #[cfg(test)]
    pub(crate) fn find_by_command(&self, command: &str) -> Option<QuicklinkView> {
        let mut state = self.state.lock().expect("quicklinks store lock poisoned");
        self.ensure_loaded_locked(&mut state);
        state
            .document
            .items
            .iter()
            .find(|item| item.command == command)
            .map(|item| self.view_for(item))
    }

    pub(crate) fn commands(&self) -> Vec<String> {
        let mut state = self.state.lock().expect("quicklinks store lock poisoned");
        self.ensure_loaded_locked(&mut state);
        state
            .document
            .items
            .iter()
            .map(|item| item.command.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn save(
        &self,
        input: QuicklinkSaveInput,
    ) -> Result<QuicklinkView, QuicklinkErrorCode> {
        self.save_with_external_conflicts(input, &[])
    }

    pub(crate) fn save_with_external_conflicts(
        &self,
        input: QuicklinkSaveInput,
        external_conflicts: &[String],
    ) -> Result<QuicklinkView, QuicklinkErrorCode> {
        if !valid_quicklink_command(&input.command) {
            return Err(QuicklinkErrorCode::InvalidCommand);
        }
        if reserved_quicklink_command(&input.command) {
            return Err(QuicklinkErrorCode::CommandConflict);
        }
        if external_conflicts
            .iter()
            .any(|value| value == &input.command)
        {
            return Err(QuicklinkErrorCode::CommandConflict);
        }
        if input.name.trim().is_empty()
            || input.name.trim() != input.name
            || contains_forbidden_text(&input.name)
        {
            return Err(QuicklinkErrorCode::SaveFailed);
        }
        expand_template(&input.template, "x")?;

        let mut state = self.state.lock().expect("quicklinks store lock poisoned");
        self.ensure_loaded_locked(&mut state);
        let existing_index = input
            .id
            .as_ref()
            .and_then(|id| state.document.items.iter().position(|item| &item.id == id));
        if state
            .document
            .items
            .iter()
            .enumerate()
            .any(|(index, item)| item.command == input.command && Some(index) != existing_index)
        {
            return Err(QuicklinkErrorCode::CommandConflict);
        }

        let now = timestamp_string();
        let id = if let Some(index) = existing_index {
            state.document.items[index].id.clone()
        } else if let Some(id) = input.id.as_ref() {
            id.clone()
        } else {
            let id = state.document.next_id.to_string();
            state.document.next_id = state
                .document
                .next_id
                .checked_add(1)
                .ok_or(QuicklinkErrorCode::SaveFailed)?;
            id
        };
        let created_at = existing_index
            .map(|index| state.document.items[index].created_at.clone())
            .unwrap_or_else(|| now.clone());
        let icon_asset = self.save_icon_for_record(
            &id,
            input.icon_token.as_deref(),
            existing_index.map(|index| state.document.items[index].icon_asset.clone()),
        )?;
        let record = StoredQuicklink {
            id,
            name: input.name,
            command: input.command,
            template: input.template,
            icon_asset,
            created_at,
            updated_at: now,
        };
        if let Some(index) = existing_index {
            state.document.items[index] = record.clone();
        } else {
            state.document.items.push(record.clone());
        }
        self.write_document(&state.document)?;
        state.load_error = None;
        Ok(self.view_for(&record))
    }

    pub(crate) fn delete(&self, id: &str) -> Result<(), QuicklinkErrorCode> {
        let mut state = self.state.lock().expect("quicklinks store lock poisoned");
        self.ensure_loaded_locked(&mut state);
        let Some(index) = state.document.items.iter().position(|item| item.id == id) else {
            return Ok(());
        };
        let removed = state.document.items.remove(index);
        self.write_document(&state.document)
            .map_err(|_| QuicklinkErrorCode::DeleteFailed)?;
        if let Some(asset) = removed.icon_asset {
            let _ = fs::remove_file(self.icons_dir().join(asset));
        }
        Ok(())
    }

    pub(crate) fn create_icon_candidate_from_path(
        &self,
        path: &Path,
    ) -> Result<QuicklinkIconCandidate, QuicklinkErrorCode> {
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("png"))
        {
            return Err(QuicklinkErrorCode::InvalidIcon);
        }
        let bytes = fs::read(path).map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
        validate_png_icon(&bytes)?;
        let token = format!(
            "icon-{}-{}",
            std::process::id(),
            NEXT_ICON_TOKEN.fetch_add(1, Ordering::Relaxed)
        );
        let destination = self.icon_candidates_dir().join(format!("{token}.png"));
        fs::create_dir_all(self.icon_candidates_dir())
            .map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
        fs::write(&destination, &bytes).map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
        Ok(QuicklinkIconCandidate {
            token,
            data_url: data_url(&bytes),
        })
    }

    fn ensure_loaded_locked(&self, state: &mut StoreState) {
        if state.loaded {
            return;
        }
        match self.load_document() {
            Ok(document) => {
                state.document = document;
                state.load_error = None;
            }
            Err(LoadDocumentError::Missing) => {
                state.document = StoredDocument::empty();
                state.load_error = None;
            }
            Err(LoadDocumentError::Invalid) => {
                let _ = quarantine_corrupt_config(&self.config_path());
                state.document = StoredDocument::empty();
                state.load_error = Some(QuicklinkErrorCode::LoadFailed);
            }
            Err(LoadDocumentError::ReadFailed) => {
                state.document = StoredDocument::empty();
                state.load_error = Some(QuicklinkErrorCode::LoadFailed);
            }
        }
        state.loaded = true;
    }

    fn load_document(&self) -> Result<StoredDocument, LoadDocumentError> {
        let bytes = match atomic_file::read_optional(&self.config_path()) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Err(LoadDocumentError::Missing),
            Err(_) => return Err(LoadDocumentError::ReadFailed),
        };
        let document: StoredDocument =
            serde_json::from_slice(&bytes).map_err(|_| LoadDocumentError::Invalid)?;
        validate_document(&document).map_err(|_| LoadDocumentError::Invalid)?;
        Ok(document)
    }

    fn write_document(&self, document: &StoredDocument) -> Result<(), QuicklinkErrorCode> {
        fs::create_dir_all(self.quicklinks_dir()).map_err(|_| QuicklinkErrorCode::SaveFailed)?;
        let bytes =
            serde_json::to_vec_pretty(document).map_err(|_| QuicklinkErrorCode::SaveFailed)?;
        atomic_file::replace_current(&self.config_path(), &bytes)
            .map_err(|_| QuicklinkErrorCode::SaveFailed)
    }

    fn view_for(&self, item: &StoredQuicklink) -> QuicklinkView {
        QuicklinkView {
            id: item.id.clone(),
            name: item.name.clone(),
            command: item.command.clone(),
            template: item.template.clone(),
            icon_data_url: item.icon_asset.as_ref().and_then(|asset| {
                fs::read(self.icons_dir().join(asset))
                    .ok()
                    .map(|bytes| data_url(&bytes))
            }),
            created_at: item.created_at.clone(),
            updated_at: item.updated_at.clone(),
        }
    }

    fn save_icon_for_record(
        &self,
        id: &str,
        token: Option<&str>,
        previous: Option<Option<String>>,
    ) -> Result<Option<String>, QuicklinkErrorCode> {
        let Some(token) = token else {
            return Ok(previous.flatten());
        };
        if !valid_icon_token(token) {
            return Err(QuicklinkErrorCode::InvalidIcon);
        }
        let source = self.icon_candidates_dir().join(format!("{token}.png"));
        let bytes = fs::read(&source).map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
        validate_png_icon(&bytes)?;
        fs::create_dir_all(self.icons_dir()).map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
        let asset = format!("{id}.png");
        fs::write(self.icons_dir().join(&asset), bytes)
            .map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
        let _ = fs::remove_file(source);
        Ok(Some(asset))
    }

    fn quicklinks_dir(&self) -> PathBuf {
        self.root.join("quicklinks")
    }

    fn config_path(&self) -> PathBuf {
        self.quicklinks_dir().join("quicklinks.json")
    }

    fn icons_dir(&self) -> PathBuf {
        self.quicklinks_dir().join("icons")
    }

    fn icon_candidates_dir(&self) -> PathBuf {
        self.quicklinks_dir().join("icon-candidates")
    }
}

impl StoredDocument {
    fn empty() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            next_id: 1,
            items: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadDocumentError {
    Missing,
    ReadFailed,
    Invalid,
}

pub(crate) fn valid_quicklink_command(command: &str) -> bool {
    crate::model::valid_launcher_command(command)
}

pub(crate) fn reserved_quicklink_command(command: &str) -> bool {
    matches!(command, "find" | "quicklinks" | "web-search")
}

pub(crate) fn expand_template(template: &str, query: &str) -> Result<Url, QuicklinkErrorCode> {
    if !valid_template_text(template) {
        return Err(QuicklinkErrorCode::InvalidTemplate);
    }
    let probe = template.replace("{Query}", "x");
    let probe = Url::parse(&probe).map_err(|_| QuicklinkErrorCode::InvalidTemplate)?;
    if !matches!(probe.scheme(), "http" | "https") {
        return Err(QuicklinkErrorCode::InvalidTemplate);
    }
    let expanded = template.replace("{Query}", &percent_encode_component(query));
    let url = Url::parse(&expanded).map_err(|_| QuicklinkErrorCode::InvalidTemplate)?;
    matches!(url.scheme(), "http" | "https")
        .then_some(url)
        .ok_or(QuicklinkErrorCode::InvalidTemplate)
}

fn valid_template_text(template: &str) -> bool {
    template.contains("{Query}") && !contains_forbidden_text(template)
}

fn contains_forbidden_text(value: &str) -> bool {
    value
        .chars()
        .any(|character| character == '\0' || character.is_control())
}

fn percent_encode_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                output.push(*byte as char);
            }
            _ => {
                use std::fmt::Write as _;
                let _ = write!(&mut output, "%{byte:02X}");
            }
        }
    }
    output
}

fn validate_png_icon(bytes: &[u8]) -> Result<(), QuicklinkErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_ICON_BYTES {
        return Err(QuicklinkErrorCode::InvalidIcon);
    }
    let limits = Limits {
        bytes: MAX_ICON_BYTES * 2,
    };
    let mut reader = Decoder::new_with_limits(Cursor::new(bytes), limits)
        .read_info()
        .map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
    let info = reader.info();
    if info.width != ICON_EDGE || info.height != ICON_EDGE || info.animation_control.is_some() {
        return Err(QuicklinkErrorCode::InvalidIcon);
    }
    let output_size = reader
        .output_buffer_size()
        .ok_or(QuicklinkErrorCode::InvalidIcon)?;
    let mut output = vec![0_u8; output_size];
    reader
        .next_frame(&mut output)
        .map_err(|_| QuicklinkErrorCode::InvalidIcon)?;
    reader.finish().map_err(|_| QuicklinkErrorCode::InvalidIcon)
}

fn validate_document(document: &StoredDocument) -> Result<(), ()> {
    if document.schema_version != CONFIG_SCHEMA_VERSION || document.next_id == 0 {
        return Err(());
    }
    let mut ids = HashSet::new();
    let mut commands = HashSet::new();
    let mut max_id = 0_u64;
    for item in &document.items {
        if item.id.is_empty()
            || item.name.trim().is_empty()
            || item.name.trim() != item.name
            || contains_forbidden_text(&item.name)
            || !valid_quicklink_command(&item.command)
            || reserved_quicklink_command(&item.command)
            || expand_template(&item.template, "x").is_err()
            || contains_forbidden_text(&item.created_at)
            || contains_forbidden_text(&item.updated_at)
            || !ids.insert(item.id.clone())
            || !commands.insert(item.command.clone())
        {
            return Err(());
        }
        if let Ok(parsed) = item.id.parse::<u64>() {
            max_id = max_id.max(parsed);
        }
        if item
            .icon_asset
            .as_deref()
            .is_some_and(|asset| !valid_icon_asset(asset))
        {
            return Err(());
        }
    }
    if document.next_id <= max_id {
        return Err(());
    }
    Ok(())
}

fn valid_icon_asset(asset: &str) -> bool {
    asset.ends_with(".png")
        && asset.len() > 4
        && asset
            .strip_suffix(".png")
            .is_some_and(|id| id.bytes().all(|byte| byte.is_ascii_digit()))
}

fn valid_icon_token(token: &str) -> bool {
    token.starts_with("icon-")
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn data_url(bytes: &[u8]) -> String {
    format!("{DATA_URL_PREFIX}{}", base64_encode(bytes))
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(third & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn quarantine_corrupt_config(path: &Path) -> Result<(), ()> {
    let Some(parent) = path.parent() else {
        return Err(());
    };
    let timestamp = timestamp_string();
    for index in 0..100_u32 {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!(".{index}")
        };
        let backup = parent.join(format!("quicklinks.corrupt.{timestamp}{suffix}.json"));
        if !backup.exists() {
            fs::rename(path, backup).map_err(|_| ())?;
            return Ok(());
        }
    }
    Err(())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uipilot-quicklinks-{label}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn store(label: &str) -> (TestDir, QuicklinksStore) {
        let dir = TestDir::new(label);
        let store = QuicklinksStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn command_validation_matches_launcher_grammar_and_reserved_names() {
        assert!(valid_quicklink_command("jd"));
        assert!(valid_quicklink_command("jd-1"));
        for invalid in ["", "1jd", "-jd", "Jd", "jd_1", &"a".repeat(33)] {
            assert!(!valid_quicklink_command(invalid), "{invalid}");
        }
        for reserved in ["find", "quicklinks", "web-search"] {
            assert!(reserved_quicklink_command(reserved), "{reserved}");
        }
    }

    #[test]
    fn template_expansion_validates_http_and_percent_encodes_query_component() {
        let url =
            expand_template("https://search.jd.com/Search?keyword={Query}", "手机 A&B?").unwrap();
        assert_eq!(
            url.as_str(),
            "https://search.jd.com/Search?keyword=%E6%89%8B%E6%9C%BA%20A%26B%3F"
        );

        for template in [
            "file:///tmp?q={Query}",
            "https://example.com/search",
            "https://example.com/search?q={Query}\n",
        ] {
            assert_eq!(
                expand_template(template, "x").unwrap_err(),
                QuicklinkErrorCode::InvalidTemplate
            );
        }
    }

    #[test]
    fn save_updates_cache_and_rejects_reserved_or_duplicate_commands() {
        let (_dir, store) = store("save");
        let saved = store
            .save(QuicklinkSaveInput {
                id: None,
                name: "京东搜索".into(),
                command: "jd".into(),
                template: "https://search.jd.com/Search?keyword={Query}".into(),
                icon_token: None,
            })
            .unwrap();
        assert_eq!(saved.id, "1");
        assert_eq!(saved.command, "jd");
        assert_eq!(store.find_by_command("jd").unwrap().name, "京东搜索");

        assert_eq!(
            store
                .save(QuicklinkSaveInput {
                    id: None,
                    name: "冲突".into(),
                    command: "jd".into(),
                    template: "https://example.com/?q={Query}".into(),
                    icon_token: None,
                })
                .unwrap_err(),
            QuicklinkErrorCode::CommandConflict
        );
        assert_eq!(
            store
                .save(QuicklinkSaveInput {
                    id: None,
                    name: "内置".into(),
                    command: "find".into(),
                    template: "https://example.com/?q={Query}".into(),
                    icon_token: None,
                })
                .unwrap_err(),
            QuicklinkErrorCode::CommandConflict
        );
    }

    #[test]
    fn corrupt_config_is_quarantined_and_returns_load_error() {
        let dir = TestDir::new("corrupt");
        let quicklinks_dir = dir.path().join("quicklinks");
        fs::create_dir_all(&quicklinks_dir).unwrap();
        fs::write(quicklinks_dir.join("quicklinks.json"), b"{not valid json").unwrap();

        let store = QuicklinksStore::new(dir.path().to_path_buf());
        let response = store.list();
        assert_eq!(response.items.len(), 0);
        assert_eq!(response.load_error, Some(QuicklinkErrorCode::LoadFailed));
        assert!(fs::read_dir(&quicklinks_dir).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("quicklinks.corrupt.")));
    }

    #[test]
    fn png_icons_must_decode_to_exactly_128_square_and_return_data_url() {
        let (_dir, store) = store("icons");
        let valid = write_png(store.root().join("valid.png"), 128, 128);
        let wide = write_png(store.root().join("wide.png"), 129, 128);
        let fake = store.root().join("fake.png");
        fs::write(&fake, b"not a png").unwrap();

        let accepted = store.create_icon_candidate_from_path(&valid).unwrap();
        assert!(accepted.token.starts_with("icon-"));
        assert!(accepted.data_url.starts_with("data:image/png;base64,"));

        for path in [&wide, &fake] {
            assert_eq!(
                store.create_icon_candidate_from_path(path).unwrap_err(),
                QuicklinkErrorCode::InvalidIcon
            );
        }
    }

    fn write_png(path: PathBuf, width: u32, height: u32) -> PathBuf {
        let file = fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let bytes = vec![0_u8; width as usize * height as usize * 4];
        writer.write_image_data(&bytes).unwrap();
        path
    }
}
