//! Persisted public model metadata, separate from user configuration and sessions.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ai_client::AiClient;
use ai_client::catalog_sync::{self, CatalogModels, Download};
use serde::{Deserialize, Serialize};

use crate::config::{ConfigLoader, RuntimeConfig};
use crate::error::{Result, RuntimeError};
use crate::runtime::Runtime;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Cache {
    version: u32,
    source: String,
    updated_at: Option<String>,
    models: CatalogModels,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            version: 1,
            source: catalog_sync::SOURCE.into(),
            updated_at: None,
            models: BTreeMap::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct CatalogState {
    cache: Cache,
    warnings: Vec<String>,
}

impl CatalogState {
    pub(crate) fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let loaded = (|| -> Result<Cache> {
            let metadata = match std::fs::metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Cache::default());
                }
                Err(error) => return Err(error.into()),
            };
            if metadata.len() > 16 * 1024 * 1024 {
                return Err(RuntimeError::ConfigError("目录缓存过大".into()));
            }
            let cache: Cache = serde_json::from_str(&std::fs::read_to_string(path)?)
                .map_err(|_| RuntimeError::ConfigError("目录缓存格式错误".into()))?;
            if cache.version != 1 || cache.source != catalog_sync::SOURCE {
                return Err(RuntimeError::ConfigError(
                    "不支持的目录缓存版本或来源".into(),
                ));
            }
            catalog_sync::validate(&cache.models)?;
            Ok(cache)
        })();
        match loaded {
            Ok(cache) => Self {
                cache,
                warnings: vec![],
            },
            Err(error) => Self {
                warnings: vec![format!(
                    "无法读取模型目录缓存 {}：{error}。已使用内置目录，可重新同步。",
                    path.display()
                )],
                ..Self::default()
            },
        }
    }

    pub(crate) fn client(&self, config: &RuntimeConfig) -> Result<AiClient> {
        Ok(AiClient::from_config(&catalog_sync::apply(
            config.providers(),
            &self.cache.models,
        ))?)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    pub source: String,
    pub updated_at: Option<String>,
    pub provider_count: usize,
    pub model_count: usize,
    pub cache_path: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSyncReport {
    pub status: CatalogStatus,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
}

impl Runtime {
    pub(crate) async fn catalog_client(&self, config: &RuntimeConfig) -> Result<AiClient> {
        self.catalog_state.read().await.client(config)
    }

    pub async fn catalog_status(&self) -> CatalogStatus {
        let state = self.catalog_state.read().await;
        CatalogStatus {
            source: state.cache.source.clone(),
            updated_at: state.cache.updated_at.clone(),
            provider_count: state.cache.models.len(),
            model_count: state.cache.models.values().map(BTreeMap::len).sum(),
            cache_path: self
                .catalog_path
                .as_ref()
                .map(|path| path.display().to_string()),
            warnings: state.warnings.clone(),
        }
    }

    /// A template shows the same cached directory that later joins a configured route.
    pub async fn catalog_template_models(&self, provider: &str) -> Vec<(String, String)> {
        let mut models: BTreeMap<String, String> = ai_client::catalog::builtin_provider(provider)
            .into_iter()
            .flat_map(|entry| entry.models.iter())
            .map(|m| (m.id.into(), m.display_name.into()))
            .collect();
        if let Some(cached) = self.catalog_state.read().await.cache.models.get(provider) {
            for model in cached.values() {
                models.insert(model.id.clone(), model.display_name.clone());
            }
        }
        models.into_iter().collect()
    }

    /// Explicit user action only. Download outside the config transaction, then
    /// re-read the latest user configuration before atomically publishing.
    pub async fn sync_catalog(&self) -> Result<CatalogSyncReport> {
        let _sync = self
            .catalog_sync
            .try_lock()
            .map_err(|_| RuntimeError::ConfigError("模型目录正在同步，请稍候".into()))?;
        if self.catalog_path.is_none() || self.config_path.is_none() {
            return Err(RuntimeError::ConfigError(
                "当前运行时未绑定配置和目录缓存".into(),
            ));
        }
        self.install_catalog(catalog_sync::download().await?).await
    }

    async fn install_catalog(&self, downloaded: Download) -> Result<CatalogSyncReport> {
        let _update = self.config_updates.lock().await;
        let path = self
            .catalog_path
            .as_ref()
            .ok_or_else(|| RuntimeError::ConfigError("没有目录缓存路径".into()))?;
        let config_path = self
            .config_path
            .as_ref()
            .ok_or_else(|| RuntimeError::ConfigError("没有配置文件路径".into()))?;
        let mut next = self.catalog_state.read().await.cache.clone();
        catalog_sync::validate(&downloaded.models)?;
        let mut added = 0;
        let mut updated = 0;
        for (provider, entries) in downloaded.models {
            let current = next.models.entry(provider).or_default();
            for (id, model) in entries {
                match current.get(&id) {
                    None => added += 1,
                    Some(old) if old != &model => updated += 1,
                    _ => {}
                }
                current.insert(id, model);
            }
        }
        // Retain missing/failed providers and vanished model IDs for saved sessions.
        next.updated_at = Some(chrono::Utc::now().to_rfc3339());
        let config = ConfigLoader::load(config_path)?;
        let candidate =
            AiClient::from_config(&catalog_sync::apply(config.providers(), &next.models))?;
        let text = serde_json::to_string_pretty(&next)
            .map_err(|e| RuntimeError::ConfigError(e.to_string()))?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(RuntimeError::ConfigError("目录缓存过大".into()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::config::persist::atomic_write(path, &text)?;
        self.llm.install(candidate).await;
        *self.catalog_state.write().await = CatalogState {
            cache: next,
            warnings: downloaded.warnings,
        };
        Ok(CatalogSyncReport {
            status: self.catalog_status().await,
            added,
            updated,
            skipped: downloaded.skipped,
        })
    }
}

pub(crate) fn cache_path(directory: &Path) -> PathBuf {
    // Session storage scans root *.json; metadata belongs in a subdirectory.
    directory.join("cache").join("catalog.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_client::catalog_sync::CatalogModel;

    fn download_fixture() -> Download {
        let model = CatalogModel {
            id: "sync-test-model".into(),
            display_name: "Synced model".into(),
            context_window: 10000,
            max_tokens: 1000,
            protocol: None,
        };
        Download {
            models: BTreeMap::from([(
                "deepseek".into(),
                BTreeMap::from([(model.id.clone(), model)]),
            )]),
            ..Download::default()
        }
    }

    #[tokio::test]
    async fn sync_persists_outside_config_survives_reload_and_restart() {
        let dir = std::env::temp_dir().join(format!("llmn-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        let source = "# unchanged\n[providers.deepseek]\napi_key = 'test'\ndefault_model = 'deepseek-chat'\n";
        std::fs::write(&path, source).unwrap();
        let runtime = Runtime::open(&path, &dir).await.unwrap();
        let before = runtime.default_model().await.unwrap();
        let frozen = runtime.llm_client().snapshot().await;
        let report = runtime.install_catalog(download_fixture()).await.unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(runtime.default_model().await.unwrap(), before);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        assert!(
            !frozen
                .list_models()
                .await
                .iter()
                .any(|m| m.spec.wire == "sync-test-model")
        );
        runtime.reload_config(&path).await.unwrap();
        assert!(
            runtime
                .list_models()
                .await
                .iter()
                .any(|m| m.spec.wire == "sync-test-model")
        );
        // Partial results retain vanished IDs and do not mount unconfigured providers.
        let mut partial = download_fixture();
        let mut model = partial.models["deepseek"].values().next().unwrap().clone();
        model.id = "another-model".into();
        partial.models.clear();
        partial.models.insert(
            "anthropic".into(),
            BTreeMap::from([(model.id.clone(), model)]),
        );
        partial.warnings.push("deepseek: offline".into());
        let partial = runtime.install_catalog(partial).await.unwrap();
        assert_eq!(partial.status.model_count, 2);
        assert_eq!(partial.status.warnings, ["deepseek: offline"]);
        assert!(
            runtime
                .list_models()
                .await
                .iter()
                .all(|m| m.provider == "deepseek")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);

        runtime.shutdown_features().await.unwrap();
        drop(runtime);
        let runtime = Runtime::open(&path, &dir).await.unwrap();
        assert!(
            runtime
                .list_models()
                .await
                .iter()
                .any(|m| m.spec.wire == "sync-test-model")
        );
        // A failed disk write keeps both the installed catalog and the timestamp.
        let old = runtime.catalog_status().await;
        let cache = cache_path(&dir);
        std::fs::remove_file(&cache).unwrap();
        std::fs::create_dir(&cache).unwrap();
        assert!(runtime.install_catalog(download_fixture()).await.is_err());
        assert_eq!(runtime.catalog_status().await.updated_at, old.updated_at);
        runtime.shutdown_features().await.unwrap();
        drop(runtime);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_cache_falls_back_with_visible_warning() {
        let dir = std::env::temp_dir().join(format!("llmn-catalog-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("catalog.json");
        std::fs::write(&path, "broken").unwrap();
        let state = CatalogState::load(Some(&path));
        assert!(state.cache.models.is_empty());
        assert_eq!(state.warnings.len(), 1);
        assert!(state.warnings[0].contains(path.to_str().unwrap()));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
