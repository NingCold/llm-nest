use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};
mod cloud;
mod config_path;
mod directories;
mod draft_close;
mod local_agent;
mod local_agent_tray;
mod local_chat;
mod secure_credentials;
mod window_frame;
pub struct BackendState {
    paths: Result<config_path::AppPaths, String>,
    shutdown_started: AtomicBool,
    exit_ready: AtomicBool,
    drafts: draft_close::DraftCloseGuard,
}
fn request_shutdown(app: &tauri::AppHandle, force_exit: bool) {
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
        let local = app.state::<local_agent::LocalAgentState>();
        let running = match local.any_running().await {
            Ok(v) => v,
            Err(e) => {
                let _ = app.emit("backend-shutdown-failed", e);
                state.shutdown_started.store(false, Ordering::Release);
                return;
            }
        };
        if !force_exit && running {
            match local_agent_tray::background_choice(&app, &local).await {
                Ok(local_agent_tray::CloseChoice::Continue) => {
                    match app
                        .get_webview_window("main")
                        .ok_or_else(|| "主窗口不可用".to_owned())
                        .and_then(|window| {
                            window
                                .hide()
                                .map_err(|_| "隐藏窗口失败，请重试关闭".to_owned())
                        }) {
                        Ok(()) => {
                            let _ = app.emit("desktop-close-retained", ());
                        }
                        Err(error) => {
                            let _ = app.emit("backend-shutdown-failed", error);
                        }
                    }
                    state.shutdown_started.store(false, Ordering::Release);
                    return;
                }
                Ok(local_agent_tray::CloseChoice::Cancel) => {
                    let _ = app.emit("desktop-close-retained", ());
                    state.shutdown_started.store(false, Ordering::Release);
                    return;
                }
                Ok(local_agent_tray::CloseChoice::Stop) => {}
                Err(error) => {
                    let _ = app.emit("backend-shutdown-failed", error);
                    state.shutdown_started.store(false, Ordering::Release);
                    return;
                }
            }
        }
        if let Err(error) = local.stop_all(&app).await {
            let _ = app.emit("backend-shutdown-failed", error);
            state.shutdown_started.store(false, Ordering::Release);
            return;
        }
        state.exit_ready.store(true, Ordering::Release);
        app.exit(0);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            window_frame::setup(app);
            app.manage(cloud::CloudState::with_events(app.handle().clone()));
            app.manage(local_agent::LocalAgentState::new());
            local_agent_tray::setup(app)?;
            app.manage(BackendState {
                paths: config_path::AppPaths::resolve(),
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
                webview
                    .state::<cloud::CloudState>()
                    .release_owner(webview.label());
                if webview.label() == "main" {
                    state.drafts.release(None);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            local_chat::local_chat_prepare,
            local_chat::local_chat_view,
            local_chat::local_chat_send,
            local_chat::local_chat_prepare_reference,
            local_chat::local_chat_attachment,
            local_chat::local_chat_stop,
            local_chat::local_chat_sync,
            local_agent::local_agent_status,
            local_agent::local_agent_setup,
            local_agent::local_agent_pair,
            local_agent::local_agent_start,
            local_agent::local_agent_stop,
            local_agent::local_agent_models,
            cloud::cloud_set_server,
            cloud::cloud_unlock_support,
            cloud::cloud_unlock_save,
            cloud::cloud_unlock_load,
            cloud::cloud_unlock_remove,
            cloud::cloud_request,
            cloud::cloud_subscribe,
            cloud::cloud_unsubscribe,
            cloud::cloud_save_download,
            cloud::cloud_save_blob,
            draft_close::desktop_register_draft_guard,
            draft_close::desktop_unregister_draft_guard,
            draft_close::desktop_finish_draft_close,
            window_frame::set_window_appearance,
            directories::get_app_paths,
            directories::open_app_directory,
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
                    request_shutdown(app, false);
                }
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } => {
                app.state::<cloud::CloudState>().release_owner(&label);
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
                request_shutdown(app, true);
            }
            _ => {}
        });
}
