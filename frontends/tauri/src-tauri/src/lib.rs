use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use runtime::runtime::Runtime;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

pub mod commands;
mod config_path;
mod conversation_export;
mod directories;
mod draft_close;
mod social_commands;
mod social_subscription;
mod window_frame;

pub struct AppState {
    pub runtime: Runtime,
    pub social: Arc<social_feature::SocialFeature>,
}

/// A failed startup is not cached. The GUI can show the actual error and retry.
pub struct BackendState {
    paths: Result<config_path::AppPaths, String>,
    inner: Mutex<Option<Arc<AppState>>>,
    subscriptions: Arc<social_subscription::Subscriptions>,
    closing: AtomicBool,
    shutdown_started: AtomicBool,
    exit_ready: AtomicBool,
    drafts: draft_close::DraftCloseGuard,
}
impl BackendState {
    pub async fn get(&self) -> Result<Arc<AppState>, String> {
        let mut inner = self.inner.lock().await;
        if self.closing.load(Ordering::Acquire) {
            return Err("应用正在关闭".into());
        }
        if let Some(state) = inner.as_ref() {
            return Ok(state.clone());
        }
        let paths = self.paths.as_ref().map_err(Clone::clone)?;
        paths.prepare()?;
        let config_path = paths.config_file.clone();
        let runtime = Arc::new(
            Runtime::open(&config_path, &paths.data_dir)
                .await
                .map_err(|e| e.to_string())?,
        );

        let social = Arc::new(social_feature::SocialFeature::new());

        runtime.register_feature(social.clone()).await;
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

            social,
        });
        *inner = Some(state.clone());
        Ok(state)
    }

    async fn shutdown(&self) -> Result<(), String> {
        self.subscriptions.shutdown().await;
        // Also waits for an initialization already in progress, without starting one.
        let inner = self.inner.lock().await;
        if let Some(state) = inner.as_ref() {
            state
                .runtime
                .shutdown_features()
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn request_shutdown(app: &tauri::AppHandle) {
    let state = app.state::<BackendState>();
    if state.shutdown_started.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<BackendState>();
        if let Err(error) = state.drafts.prepare(&app).await {
            let _ = app.emit(
                "backend-shutdown-failed",
                format!("退出保存失败：{error}。应用仍保持打开。"),
            );
            state.shutdown_started.store(false, Ordering::Release);
            return;
        }
        state.closing.store(true, Ordering::Release);
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(20), state.shutdown()).await;
        match result {
            Ok(Ok(())) => {
                state.exit_ready.store(true, Ordering::Release);
                app.exit(0);
            }
            failed => {
                let message = match failed {
                    Ok(Err(error)) => format!("退出保存失败：{error}。请检查数据目录后再次关闭。"),
                    Err(_) => "退出保存超时，应用仍保持打开。请稍后再次关闭。".into(),
                    Ok(Ok(())) => unreachable!(),
                };
                eprintln!("{message}");
                let _ = app.emit("backend-shutdown-failed", message);
                state.shutdown_started.store(false, Ordering::Release);
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    runtime::worker_entry();
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            window_frame::setup(app);
            app.manage(BackendState {
                paths: config_path::AppPaths::resolve(),
                inner: Mutex::new(None),
                subscriptions: Arc::new(social_subscription::Subscriptions::default()),
                closing: AtomicBool::new(false),
                shutdown_started: AtomicBool::new(false),
                exit_ready: AtomicBool::new(false),
                drafts: draft_close::DraftCloseGuard::default(),
            });
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started
                && let Some(state) = webview.app_handle().try_state::<BackendState>()
            {
                state.subscriptions.release_owner(webview.label());
                if webview.label() == "main" {
                    state.drafts.release(None);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            draft_close::desktop_register_draft_guard,
            draft_close::desktop_unregister_draft_guard,
            draft_close::desktop_finish_draft_close,
            window_frame::set_window_appearance,
            directories::get_app_paths,
            directories::open_app_directory,
            commands::settings_snapshot,
            commands::provider_templates,
            commands::catalog_status,
            commands::sync_catalog,
            commands::upsert_provider,
            commands::delete_provider,
            social_commands::social_init,
            conversation_export::social_export_conversation,
            conversation_export::social_save_image,
            social_commands::social_preview_legacy,
            social_commands::social_import_legacy,
            social_commands::social_summaries,
            social_commands::social_reference_preview,
            social_commands::social_configure_summaries,
            social_commands::social_request_summary,
            social_commands::social_memories,
            social_commands::social_create_memory,
            social_commands::social_update_memory,
            social_commands::social_forget_memory,
            social_commands::social_account,
            social_commands::social_managed_accounts,
            social_commands::social_managed_account,
            social_commands::social_set_administrator,
            social_commands::social_set_contact,
            social_commands::social_set_contact_block,
            social_commands::social_update_account,
            social_commands::social_account_view,
            social_commands::social_account_view_messages,
            social_commands::social_create_ai,
            social_commands::social_update_ai,
            social_commands::social_control_account,
            social_commands::social_open_direct,
            social_commands::social_create_group,
            social_commands::social_change_membership,
            social_commands::social_plans,
            social_commands::social_message,
            social_commands::social_revise_message,
            social_commands::social_message_image,
            social_commands::social_account_view_image,
            social_commands::social_withdraw_message,
            social_commands::social_control_conversation,
            social_commands::social_pause_member,
            social_commands::social_update_participation,
            social_commands::social_update_contact_settings,
            social_commands::social_create_contact_intent,
            social_commands::social_cancel_contact_intent,
            social_commands::social_conversation,
            social_commands::social_messages,
            social_commands::social_search_messages,
            social_commands::social_message_context,
            social_commands::social_update_conversation_list,
            social_commands::social_send,
            social_commands::social_events,
            social_commands::social_run_details,
            social_commands::social_cancel_run,
            social_commands::social_subscribe,
            social_commands::social_ack_subscription,
            social_commands::social_unsubscribe,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                if label == "main" {
                    api.prevent_close();
                    request_shutdown(app);
                }
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } => {
                app.state::<BackendState>()
                    .subscriptions
                    .release_owner(&label);
                if label == "main" {
                    app.state::<BackendState>().drafts.release(None);
                }
            }
            tauri::RunEvent::ExitRequested { api, .. }
                if !app
                    .state::<BackendState>()
                    .exit_ready
                    .load(Ordering::Acquire) =>
            {
                api.prevent_exit();
                request_shutdown(app);
            }
            _ => {}
        });
}
