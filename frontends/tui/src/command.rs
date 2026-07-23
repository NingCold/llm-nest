use chat::ChatEvent;
use tokio::sync::mpsc;

use crate::app::{App, UserEvent};

pub fn handle_command(app: &mut App, cmd: String, evt_tx: &mpsc::UnboundedSender<UserEvent>) {
    if cmd == "/new" {
        app.clear_messages();
        app.scroll = 0;
        let tx = evt_tx.clone();
        let svc = app.svc.clone();
        tracing::info!(target: "tui::cmd", "create new session");
        tokio::spawn(async move {
            let _ = svc.new_session(None).await;
            if let Ok(s) = svc.list_sessions().await {
                let cur = svc.current_session().await.map(|id| id.to_string());
                let mut list = String::from("Switched to new session\nSessions:\n");
                for (id, title) in &s {
                    let marker = cur
                        .as_ref()
                        .map_or(" ", |c| if c == id { "*" } else { " " });
                    list.push_str(&format!(" {} {}  {}\n", marker, id, title));
                }
                let _ = tx.send(UserEvent::Chat(ChatEvent::Info(list.trim().to_string())));
            }
        });
    } else if cmd.starts_with("/switch ") {
        let target = cmd.trim_start_matches("/switch ").trim().to_string();
        let tx = evt_tx.clone();
        let svc = app.svc.clone();
        tracing::info!(target: "tui::cmd", target = %target, "switch session");
        tokio::spawn(async move {
            if svc.switch_session(&target).await.unwrap_or(false) {
                let _ = tx.send(UserEvent::Chat(ChatEvent::Info(format!(
                    "Switched to: {}",
                    target
                ))));
            } else {
                let _ = tx.send(UserEvent::Chat(ChatEvent::Error {
                    message: format!("Session not found: {}", target),
                }));
            }
        });
    } else if cmd.starts_with("/rename ") {
        let title = cmd.trim_start_matches("/rename ").trim().to_string();
        let tx = evt_tx.clone();
        let svc = app.svc.clone();
        let cur = app.cur_session.clone();
        tracing::info!(target: "tui::cmd", title = %title, "rename session");
        tokio::spawn(async move {
            if let Some(id) = cur
                && svc.rename_session(&id, title.clone()).await.is_ok()
            {
                let _ = tx.send(UserEvent::Chat(ChatEvent::Info(format!(
                    "Renamed to: {}",
                    title
                ))));
            }
        });
    } else if cmd.starts_with("/delete ") {
        let id = cmd.trim_start_matches("/delete ").trim().to_string();
        let tx = evt_tx.clone();
        let svc = app.svc.clone();
        tracing::info!(target: "tui::cmd", session_id = %id, "delete session");
        tokio::spawn(async move {
            if svc.delete_session(&id).await.is_ok() {
                let _ = tx.send(UserEvent::Chat(ChatEvent::Info(format!("Deleted: {}", id))));
            } else {
                let _ = tx.send(UserEvent::Chat(ChatEvent::Error {
                    message: "Failed to delete".into(),
                }));
            }
        });
    } else if cmd == "/list" {
        tracing::info!(target: "tui::cmd", "list sessions");
        let tx = evt_tx.clone();
        let svc = app.svc.clone();
        tokio::spawn(async move {
            if let Ok(sessions) = svc.list_sessions().await {
                let cur = svc.current_session().await.map(|id| id.to_string());
                let mut list = String::from("Sessions:\n");
                for (id, title) in &sessions {
                    let marker = cur
                        .as_ref()
                        .map_or(" ", |c| if c == id { "*" } else { " " });
                    list.push_str(&format!(" {} {}  {}\n", marker, id, title));
                }
                let _ = tx.send(UserEvent::Chat(ChatEvent::Info(list.trim().to_string())));
            }
        });
    } else if cmd == "/help" {
        app.show_help = !app.show_help;
        app.mark_dirty();
    } else if cmd == "/quit" || cmd == "/exit" {
        app.should_quit = true;
    } else {
        app.status = format!("Unknown command: {}", cmd);
        app.mark_dirty();
    }
}
