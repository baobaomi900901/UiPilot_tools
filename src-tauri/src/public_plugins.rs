mod manifest;
mod package;
mod secrets;
mod state;
mod storage;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) use manifest::{PublicManifestV1, PublicPlatform};
pub(crate) use secrets::{PluginSecretError, PluginSecretStore};
pub(crate) use state::{
    EffectivePluginConfig, PluginStateError, PluginStateStore, PublicPluginFault,
};
pub(crate) use storage::{PluginStorageError, PluginStorageStore};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginDataScope {
    plugin_id: String,
}

impl PluginDataScope {
    pub(crate) fn new(plugin_id: &str) -> Result<Self, PublicPackageError> {
        manifest::valid_plugin_id(plugin_id)
            .then(|| Self {
                plugin_id: plugin_id.into(),
            })
            .ok_or(PublicPackageError::InvalidPackage)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidPluginScope;

fn authorize_plugin_scope(
    scope: &PluginDataScope,
    plugin_id: &str,
) -> Result<(), InvalidPluginScope> {
    (scope.plugin_id == plugin_id)
        .then_some(())
        .ok_or(InvalidPluginScope)
}

fn valid_json_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) => true,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(f64::is_finite),
        serde_json::Value::Array(values) => values.iter().all(valid_json_value),
        serde_json::Value::Object(values) => values.iter().all(|(key, value)| {
            !matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                && valid_json_value(value)
        }),
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PublicPackageSource {
    Archive(PathBuf),
    DevelopmentDirectory(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PublicPluginHost {
    pub(crate) platform: PublicPlatform,
    pub(crate) version: [u32; 3],
    pub(crate) api_version: u32,
}

impl PublicPluginHost {
    pub(crate) const fn current(platform: PublicPlatform) -> Self {
        Self {
            platform,
            version: [0, 2, 0],
            api_version: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublicPackageError {
    InvalidPackage,
    IncompatiblePlatform,
    IncompatibleApi,
    UnsupportedPermission,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicResource {
    pub(crate) mime: &'static str,
    pub(crate) length: u64,
    pub(crate) sha256: String,
}

#[derive(Debug)]
pub(crate) struct PreparedPublicPlugin {
    transaction_root: Option<PathBuf>,
    pub(crate) package_root: PathBuf,
    pub(crate) manifest: PublicManifestV1,
    pub(crate) digest: String,
    pub(crate) resources: BTreeMap<String, PublicResource>,
}

impl PreparedPublicPlugin {
    fn new(
        transaction_root: PathBuf,
        package_root: PathBuf,
        manifest: PublicManifestV1,
        digest: String,
        resources: BTreeMap<String, PublicResource>,
    ) -> Self {
        Self {
            transaction_root: Some(transaction_root),
            package_root,
            manifest,
            digest,
            resources,
        }
    }

    pub(crate) fn transaction_root(&self) -> &Path {
        self.transaction_root
            .as_deref()
            .expect("prepared transaction root missing")
    }

    pub(crate) fn revalidate(&self) -> Result<(), PublicPackageError> {
        package::revalidate_snapshot(&self.package_root, &self.digest, &self.resources)
    }

    pub(crate) fn disarm_cleanup(mut self) -> Result<PathBuf, PublicPackageError> {
        self.revalidate()?;
        Ok(self
            .transaction_root
            .take()
            .expect("prepared transaction root missing"))
    }
}

impl Drop for PreparedPublicPlugin {
    fn drop(&mut self) {
        if let Some(path) = self.transaction_root.take() {
            package::remove_transaction(path);
        }
    }
}

pub(crate) fn stage_public_package(
    source: PublicPackageSource,
    staging_root: &Path,
    host: &PublicPluginHost,
) -> Result<PreparedPublicPlugin, PublicPackageError> {
    package::stage(source, staging_root, host)
}
