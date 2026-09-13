use std::collections::HashMap;
use std::sync::Arc;

use runtime::runtime::Runtime;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub mod commands;
mod config_path;

pub struct AppState {
    pub runtime: Runtime,
    pub chat: Arc<chat::ChatFeature>,
    pub cancel_map: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

/// A failed startup is not cached. The GUI can show the actual error and retry.
pub struct BackendState {
    inner: Mutex<Option<Arc<AppState>>>,
}
impl BackendState {
    pub async fn get(&self) -> Result<Arc<AppState>, String> {
        let mut inner = self.inner.lock().await;
        if let Some(state) = inner.as_ref() {
            return Ok(state.clone());
        }
        let config_path = config_path::find_config_path()?;
        let runtime = Arc::new(
            Runtime::from_config_persistent(&config_path, storage::default_data_dir())
                .map_err(|e| e.to_string())?,
        );
        let chat = Arc::new(chat::ChatFeature::new());
        runtime.register_feature(chat.clone()).await;
        runtime
            .initialize_features()
            .await
            .map_err(|e| e.to_string())?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        runtime::config::watcher::spawn_config_watcher(runtime.clone(), config_path, tx)
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let runtime::config::watcher::ConfigWatchEvent::Failed(error) = event {
                    eprintln!("config reload failed: {error}");
                }
            }
        });
        let state = Arc::new(AppState {
            runtime: (*runtime).clone(),
            chat,
            cancel_map: Arc::new(Mutex::new(HashMap::new())),
        });
        *inner = Some(state.clone());
        Ok(state)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    runtime::worker_entry();
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            app.manage(BackendState {
                inner: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::init_app,
            commands::chat,
            commands::cancel_chat,
            commands::get_messages,
            commands::set_message_feedback,
            commands::new_session,
            commands::delete_session,
            commands::rename_session,
            commands::list_sessions,
            commands::set_config,
            commands::provider_templates,
            commands::upsert_provider,
            commands::delete_provider,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
