use std::sync::Arc;

use waddle_storage::{Database, StorageError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvQuota {
    pub max_keys: u64,
    pub max_value_bytes: u64,
}

impl Default for KvQuota {
    fn default() -> Self {
        Self {
            max_keys: 10_000,
            max_value_bytes: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KvUsage {
    pub key_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum KvError {
    #[error("value too large: {size} bytes exceeds limit of {limit} bytes")]
    ValueTooLarge { size: u64, limit: u64 },

    #[error("quota exceeded: plugin has {current} keys, limit is {limit}")]
    QuotaExceeded { current: u64, limit: u64 },

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("plugin kv store is not implemented")]
    NotImplemented,
}

pub struct PluginKvStore<D: Database> {
    plugin_id: String,
    db: Arc<D>,
    quota: KvQuota,
}

impl<D: Database> PluginKvStore<D> {
    pub fn new(plugin_id: String, db: Arc<D>, quota: KvQuota) -> Self {
        Self {
            plugin_id,
            db,
            quota,
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn quota(&self) -> &KvQuota {
        &self.quota
    }

    pub fn database(&self) -> &Arc<D> {
        &self.db
    }

    pub async fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, KvError> {
        Err(KvError::NotImplemented)
    }

    pub async fn set(&self, _key: &str, _value: &[u8]) -> Result<(), KvError> {
        Err(KvError::NotImplemented)
    }

    pub async fn delete(&self, _key: &str) -> Result<(), KvError> {
        Err(KvError::NotImplemented)
    }

    pub async fn list_keys(&self, _prefix: &str) -> Result<Vec<String>, KvError> {
        Err(KvError::NotImplemented)
    }

    pub async fn usage(&self) -> Result<KvUsage, KvError> {
        Err(KvError::NotImplemented)
    }

    pub async fn clear_all(&self) -> Result<(), KvError> {
        Err(KvError::NotImplemented)
    }
}
