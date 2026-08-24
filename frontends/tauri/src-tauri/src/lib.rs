use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use runtime::runtime::Runtime;
use tauri::Manager;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub mod commands;

pub struct AppState {
    pub runtime: Runtime,
    pub chat: Arc<chat::ChatFeature>,
    pub cancel_map: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

fn find_config_path() -> Result<PathBuf, String> {
    let candidates = [PathBuf::from("config/llmn.toml"), {
        let mut p = std::env::current_exe().map_err(|e| e.to_string())?;
        p.pop();
        p.pop();
        p.pop();
        p.push("config/llmn.toml");
        p
    }];
    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    Err(format!(
        "config/llmn.toml not found, tried: {:?}",
        candidates
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let config_path = find_config_path()?;
            let runtime =
                Runtime::from_config_persistent(&config_path, storage::default_data_dir())
                    .map_err(|e| e.to_string())?;
            let runtime = Arc::new(runtime);

            let chat = Arc::new(chat::ChatFeature::new());
            let chat_clone = chat.clone();
            let rt = runtime.clone();
            tauri::async_runtime::block_on(async move {
                rt.register_feature(chat_clone).await;
                rt.initialize_features().await.map_err(|e| e.to_string())
            })?;

            let state = AppState {
                runtime: (*runtime).clone(),
                chat,
                cancel_map: Arc::new(Mutex::new(HashMap::new())),
            };
            app.manage(state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::init_app,
            commands::chat,
            commands::cancel_chat,
            commands::get_messages,
            commands::new_session,
            commands::delete_session,
            commands::rename_session,
            commands::list_sessions,
            commands::set_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
