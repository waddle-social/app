use std::collections::BTreeMap;
use std::sync::Arc;

use waddle_core::event::Event;
#[cfg(feature = "native")]
use waddle_core::event::EventBus;
use waddle_storage::Database;

use crate::registry::PluginManifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRuntimeConfig {
    pub fuel_per_invocation: u64,
    pub fuel_per_render: u64,
    pub epoch_timeout_ms: u64,
    pub max_memory_bytes: u64,
}

impl Default for PluginRuntimeConfig {
    fn default() -> Self {
        Self {
            fuel_per_invocation: 1_000_000,
            fuel_per_render: 5_000_000,
            epoch_timeout_ms: 5_000,
            max_memory_bytes: 16_777_216,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin runtime is not implemented")]
    NotImplemented,

    #[error("plugin {id} not found")]
    NotFound { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHandle {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    Loading,
    Active,
    Error(String),
    Unloading,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginCapability {
    EventHandler,
    StanzaProcessor { priority: i32 },
    TuiRenderer,
    GuiMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub status: PluginStatus,
    pub capabilities: Vec<PluginCapability>,
    pub error_count: u32,
}

#[derive(Debug, Clone)]
pub enum PluginHook {
    Event(Box<Event>),
    InboundStanza(String),
    OutboundStanza(String),
    TuiRender { width: u16, height: u16 },
    GuiGetComponentInfo,
}

pub struct PluginRuntime<D: Database> {
    config: PluginRuntimeConfig,
    #[cfg(feature = "native")]
    event_bus: Arc<dyn EventBus>,
    db: Arc<D>,
    plugins: BTreeMap<String, PluginInfo>,
}

impl<D: Database> PluginRuntime<D> {
    #[cfg(feature = "native")]
    pub fn new(config: PluginRuntimeConfig, event_bus: Arc<dyn EventBus>, db: Arc<D>) -> Self {
        Self {
            config,
            event_bus,
            db,
            plugins: BTreeMap::new(),
        }
    }

    #[cfg(not(feature = "native"))]
    pub fn new(config: PluginRuntimeConfig, db: Arc<D>) -> Self {
        Self {
            config,
            db,
            plugins: BTreeMap::new(),
        }
    }

    pub fn config(&self) -> &PluginRuntimeConfig {
        &self.config
    }

    #[cfg(feature = "native")]
    pub fn event_bus(&self) -> &Arc<dyn EventBus> {
        &self.event_bus
    }

    pub fn database(&self) -> &Arc<D> {
        &self.db
    }

    pub async fn load_plugin(
        &mut self,
        _manifest: PluginManifest,
        _wasm_bytes: &[u8],
    ) -> Result<PluginHandle, PluginError> {
        Err(PluginError::NotImplemented)
    }

    pub async fn unload_plugin(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        if self.plugins.remove(plugin_id).is_some() {
            return Ok(());
        }

        Err(PluginError::NotFound {
            id: plugin_id.to_string(),
        })
    }

    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins.values().cloned().collect()
    }

    pub fn get_plugin(&self, plugin_id: &str) -> Option<&PluginInfo> {
        self.plugins.get(plugin_id)
    }

    pub async fn invoke_hook(&self, _hook: PluginHook) -> Result<(), PluginError> {
        Err(PluginError::NotImplemented)
    }
}
