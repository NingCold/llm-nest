use tokio::sync::mpsc;

use crate::app::{App, UserEvent};

pub fn handle_command(app: &mut App, cmd: String, evt_tx: &mpsc::UnboundedSender<UserEvent>) {
    if cmd == "/new" {
        app.clear_messages();
        app.scroll = 0;
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        tx.send(UserEvent::Status("Creating new session...".into())).ok();
        tokio::spawn(async move {
            let id = rt.create_session(None).await;
            let _ = tx.send(UserEvent::SessionSwitched(id));
            let ids = rt.list_sessions().await;
            let mut list = String::from("Switched to new session\nSessions:\n");
            for sid in &ids {
                let title = rt
                    .get_session(sid)
                    .await
                    .and_then(|s| s.title().map(String::from))
                    .unwrap_or_default();
                let marker = if *sid == id { "*" } else { " " };
                list.push_str(&format!(" {} {}  {}\n", marker, sid, title));
            }
            let _ = tx.send(UserEvent::Status(list.trim().to_string()));
        });
    } else if cmd.starts_with("/switch ") {
        let target = cmd.trim_start_matches("/switch ").trim().to_string();
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        tokio::spawn(async move {
            // try UUID match
            if let Ok(id) = target.parse::<common::SessionId>() {
                if rt.get_session(&id).await.is_some() {
                    let _ = tx.send(UserEvent::SessionSwitched(id));
                    let _ = tx.send(UserEvent::Status(format!("Switched to: {}", target)));
                    return;
                }
            }
            // try title match
            let ids = rt.list_sessions().await;
            for id in &ids {
                if let Some(s) = rt.get_session(id).await {
                    if s.title() == Some(target.as_str()) {
                        let _ = tx.send(UserEvent::SessionSwitched(*id));
                        let _ = tx.send(UserEvent::Status(format!("Switched to: {}", target)));
                        return;
                    }
                }
            }
            let _ = tx.send(UserEvent::Status(format!("Session not found: {}", target)));
        });
    } else if cmd.starts_with("/rename ") {
        let title = cmd.trim_start_matches("/rename ").trim().to_string();
        if let Some(id) = app.cur_session {
            let rt = app.runtime.clone();
            let tx = evt_tx.clone();
            tokio::spawn(async move {
                if rt.rename_session(id, title.clone()).await.is_ok() {
                    let _ = tx.send(UserEvent::Status(format!("Renamed to: {}", title)));
                }
            });
        }
    } else if cmd.starts_with("/delete ") {
        let id_str = cmd.trim_start_matches("/delete ").trim().to_string();
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        let cur = app.cur_session;
        tokio::spawn(async move {
            if let Ok(sid) = id_str.parse::<common::SessionId>() {
                if rt.delete_session(sid).await.is_ok() {
                    if Some(sid) == cur {
                        // pick first available session
                        let remaining = rt.list_sessions().await;
                        if let Some(first) = remaining.first() {
                            let _ = tx.send(UserEvent::SessionSwitched(*first));
                        } else {
                            let id = rt.create_session(None).await;
                            let _ = tx.send(UserEvent::SessionSwitched(id));
                        }
                    }
                    let _ = tx.send(UserEvent::Status(format!("Deleted: {}", id_str)));
                } else {
                    let _ = tx.send(UserEvent::Status("Failed to delete".into()));
                }
            }
        });
    } else if cmd == "/list" {
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        tokio::spawn(async move {
            let ids = rt.list_sessions().await;
            let mut list = String::from("Sessions:\n");
            for sid in &ids {
                let title = rt
                    .get_session(sid)
                    .await
                    .and_then(|s| s.title().map(String::from))
                    .unwrap_or_default();
                list.push_str(&format!("   {}  {}\n", sid, title));
            }
            let _ = tx.send(UserEvent::Status(list.trim().to_string()));
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