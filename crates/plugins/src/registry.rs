use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryConfig {
    pub default_registry: String,
    pub check_updates_on_startup: bool,
    pub signature_policy: String,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            default_registry: "ghcr.io/waddle-social".to_string(),
            check_updates_on_startup: true,
            signature_policy: "warn".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSummary {
    pub reference: String,
    pub name: String,
    pub description: String,
    pub latest_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFiles {
    pub manifest: PluginManifest,
    pub wasm_path: PathBuf,
    pub vue_dir: Option<PathBuf>,
    pub assets_dir: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("failed to resolve reference {reference}: {reason}")]
    ResolveFailed { reference: String, reason: String },

    #[error("failed to pull {reference}: {reason}")]
    PullFailed { reference: String, reason: String },

    #[error("invalid manifest for plugin {id}: {reason}")]
    InvalidManifest { id: String, reason: String },

    #[error("signature verification failed for {reference}: {reason}")]
    SignatureVerificationFailed { reference: String, reason: String },

    #[error("plugin {id} not installed")]
    NotInstalled { id: String },

    #[error("plugin {id} already installed at version {version}")]
    AlreadyInstalled { id: String, version: String },

    #[error("registry authentication failed for {registry}: {reason}")]
    AuthenticationFailed { registry: String, reason: String },

    #[error("plugin registry is not implemented")]
    NotImplemented,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct PluginRegistry {
    config: RegistryConfig,
    data_dir: PathBuf,
    installed: RwLock<Vec<InstalledPlugin>>,
}

impl PluginRegistry {
    pub fn new(config: RegistryConfig, data_dir: PathBuf) -> Result<Self, RegistryError> {
        std::fs::create_dir_all(data_dir.join("plugins"))?;

        Ok(Self {
            config,
            data_dir,
            installed: RwLock::new(Vec::new()),
        })
    }

    pub fn config(&self) -> &RegistryConfig {
        &self.config
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub async fn install(&self, _reference: &str) -> Result<InstalledPlugin, RegistryError> {
        Err(RegistryError::NotImplemented)
    }

    pub async fn uninstall(&self, _plugin_id: &str) -> Result<(), RegistryError> {
        Err(RegistryError::NotImplemented)
    }

    pub async fn update(&self, _plugin_id: &str) -> Result<Option<InstalledPlugin>, RegistryError> {
        Err(RegistryError::NotImplemented)
    }

    pub async fn search(
        &self,
        _registry: &str,
        _query: &str,
    ) -> Result<Vec<PluginSummary>, RegistryError> {
        Err(RegistryError::NotImplemented)
    }

    pub fn list_installed(&self) -> Result<Vec<InstalledPlugin>, RegistryError> {
        let installed = self
            .installed
            .read()
            .map_err(|_| RegistryError::NotImplemented)?;

        Ok(installed.clone())
    }

    pub fn get_plugin_files(&self, _plugin_id: &str) -> Result<PluginFiles, RegistryError> {
        Err(RegistryError::NotImplemented)
    }

    pub async fn list_versions(&self, _reference: &str) -> Result<Vec<String>, RegistryError> {
        Err(RegistryError::NotImplemented)
    }
}
