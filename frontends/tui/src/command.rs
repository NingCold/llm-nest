use tokio::sync::mpsc;

use crate::app::{App, UserEvent};

pub fn handle_command(app: &mut App, cmd: String, evt_tx: &mpsc::UnboundedSender<UserEvent>) {
    if cmd == "/new" {
        app.clear_messages();
        app.scroll = 0;
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        tx.send(UserEvent::Status("Creating new session...".into()))
            .ok();
        tokio::spawn(async move {
            let id = match rt.create_session(None).await {
                Ok(id) => id,
                Err(e) => {
                    let _ = tx.send(UserEvent::Status(format!("Failed to create session: {e}")));
                    return;
                }
            };
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
                            match rt.create_session(None).await {
                                Ok(id) => {
                                    let _ = tx.send(UserEvent::SessionSwitched(id));
                                }
                                Err(e) => {
                                    let _ = tx.send(UserEvent::Status(format!(
                                        "Failed to create session: {e}"
                                    )));
                                }
                            }
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
    } else if cmd == "/models" {
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        let current = app.model.clone();
        tokio::spawn(async move {
            let models = rt.list_models().await;
            if models.is_empty() {
                let _ = tx.send(UserEvent::Status("No models available".into()));
                return;
            }
            let mut list = String::from("Models:\n");
            for m in &models {
                let reasoning = m
                    .spec
                    .reasoning
                    .as_ref()
                    .map(|c| {
                        format!(
                            "  [reasoning: {} · {}]",
                            c.levels
                                .iter()
                                .map(|l| l.as_wire())
                                .collect::<Vec<_>>()
                                .join("/"),
                            c.format.as_wire()
                        )
                    })
                    .unwrap_or_default();
                let mark = if current
                    .as_ref()
                    .is_some_and(|c| c.provider == m.provider && c.model == m.spec.wire)
                {
                    "*"
                } else {
                    " "
                };
                list.push_str(&format!(
                    " {} {}/{} ({}){}\n",
                    mark, m.provider, m.spec.wire, m.spec.display_name, reasoning
                ));
            }
            let _ = tx.send(UserEvent::Status(list.trim().to_string()));
        });
    } else if cmd == "/model" || cmd.starts_with("/model ") {
        let target = cmd.trim_start_matches("/model").trim().to_string();
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        let current = app.model.clone();
        let cur_session = app.cur_session;
        tokio::spawn(async move {
            if target.is_empty() {
                let msg = match &current {
                    Some(c) => format!(
                        "Current model: {}/{}  (usage: /model <provider/model>)",
                        c.provider, c.model
                    ),
                    None => "No model selected  (usage: /model <provider/model>)".to_string(),
                };
                let _ = tx.send(UserEvent::Status(msg));
                return;
            }
            match rt.select_model(&target).await {
                Ok(sel) => {
                    // 模型绑定到当前会话
                    if let Some(sid) = cur_session {
                        let _ = rt.set_session_model(&sid, sel.clone()).await;
                    }
                    let _ = tx.send(UserEvent::ModelSwitched(sel.clone()));
                    let _ = tx.send(UserEvent::Status(format!(
                        "已切换到模型: {}/{}",
                        sel.provider, sel.model
                    )));
                }
                Err(e) => {
                    let _ = tx.send(UserEvent::Status(e.to_string()));
                }
            }
        });
    } else if cmd == "/effort" || cmd.starts_with("/effort ") {
        let level = cmd.trim_start_matches("/effort").trim().to_string();
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        let current = app.model.clone();
        let cur_session = app.cur_session;
        tokio::spawn(async move {
            let Some(current_sel) = current else {
                let _ = tx.send(UserEvent::Status(
                    "No model selected; use /model <provider/model> first".into(),
                ));
                return;
            };
            if level.is_empty() {
                let shown = current_sel
                    .reasoning_effort
                    .map(|e| e.as_wire().to_string())
                    .unwrap_or_else(|| "unset".to_string());
                let _ = tx.send(UserEvent::Status(format!(
                    "Current effort: {shown}  (usage: /effort <off|low|medium|high|max>)"
                )));
                return;
            }
            let Some(effort) = parse_effort(&level) else {
                // 列出当前模型实际支持的级别（off 恒可），而不是全局枚举
                let supported = match rt.resolve_model(&current_sel).await {
                    Ok(resolved) => resolved
                        .spec
                        .reasoning
                        .map(|cap| {
                            let mut levels: Vec<&str> = vec!["off"];
                            levels.extend(cap.levels.iter().map(|l| l.as_wire()));
                            levels.join(" / ")
                        })
                        .unwrap_or_else(|| "off (model does not support reasoning)".to_string()),
                    Err(_) => "off / low / medium / high".to_string(),
                };
                let _ = tx.send(UserEvent::Status(format!(
                    "Invalid effort: {level} ({}/{} supports: {supported})",
                    current_sel.provider, current_sel.model
                )));
                return;
            };
            let candidate = ai_client::ModelSelection {
                reasoning_effort: Some(effort),
                ..current_sel.clone()
            };
            match rt.resolve_model(&candidate).await {
                Ok(_) => {
                    // effort 同样绑定到当前会话
                    if let Some(sid) = cur_session {
                        let _ = rt.set_session_model(&sid, candidate.clone()).await;
                    }
                    let _ = tx.send(UserEvent::ModelSwitched(candidate.clone()));
                    let _ = tx.send(UserEvent::Status(format!(
                        "已设置 effort: {}（{}/{}）",
                        level, candidate.provider, candidate.model
                    )));
                }
                Err(e) => {
                    let _ = tx.send(UserEvent::Status(e.to_string()));
                }
            }
        });
    } else if cmd == "/current" {
        let msg = match &app.model {
            Some(sel) => {
                let effort = sel
                    .reasoning_effort
                    .map(|e| e.as_wire().to_string())
                    .unwrap_or_else(|| "(unset, provider default)".to_string());
                format!(
                    "provider: {}\nmodel: {}\neffort: {}",
                    sel.provider, sel.model, effort
                )
            }
            None => "No model selected".to_string(),
        };
        let _ = evt_tx.send(UserEvent::Status(msg));
        app.mark_dirty();
    } else if cmd == "/reload" {
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        tokio::spawn(async move {
            match rt.reload_config(crate::runner::CONFIG_PATH).await {
                Ok(()) => {
                    let _ = tx.send(UserEvent::Status("配置已重新加载".into()));
                }
                Err(e) => {
                    let _ = tx.send(UserEvent::Status(format!(
                        "配置重载失败（保留旧配置）: {e}"
                    )));
                }
            }
        });
    } else if let Some(provider) = cmd.strip_prefix("/refresh") {
        let provider = provider.trim().to_string();
        if provider.is_empty() {
            let _ = evt_tx.send(UserEvent::Status(
                "用法: /refresh <provider>（从 GET /models 合并新模型并写回配置）".into(),
            ));
            return;
        }
        let tx = evt_tx.clone();
        let rt = app.runtime.clone();
        tokio::spawn(async move {
            match rt.refresh_models(&provider).await {
                Ok(added) if added.is_empty() => {
                    let _ = tx.send(UserEvent::Status(format!(
                        "{provider}: 无新模型（现有清单已是最新）"
                    )));
                }
                Ok(added) => {
                    let _ = tx.send(UserEvent::Status(format!(
                        "{provider}: 已合并 {} 个新模型并写回 config/llmn.toml",
                        added.len()
                    )));
                }
                Err(e) => {
                    let _ = tx.send(UserEvent::Status(format!("{provider} 刷新失败: {e}")));
                }
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

/// Parse a `/effort` level argument into the neutral effort enum.
fn parse_effort(level: &str) -> Option<ai_client::ReasoningEffort> {
    match level {
        "off" => Some(ai_client::ReasoningEffort::Off),
        "low" => Some(ai_client::ReasoningEffort::Low),
        "medium" => Some(ai_client::ReasoningEffort::Medium),
        "high" => Some(ai_client::ReasoningEffort::High),
        "max" => Some(ai_client::ReasoningEffort::Max),
        _ => None,
    }
}
