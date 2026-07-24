use std::sync::Arc;
use std::time::Instant;

use chat::ChatFeature;
use common::Role;
use crossterm::event::KeyCode;
use events::ChatEvent;
use futures_util::StreamExt;
use llm::{GenerationOptions, ModelSelection};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::{App, InputEvent, InputMode, UserEvent};
use crate::command;

pub fn handle_event(
    app: &mut App,
    uevt: UserEvent,
    evt_tx: &mpsc::UnboundedSender<UserEvent>,
    chat_feature: Arc<ChatFeature>,
) {
    match uevt {
        UserEvent::Status(msg) => {
            app.status = msg;
            app.mark_dirty();
        }
        UserEvent::SessionSwitched(sid) => {
            app.cur_session = Some(sid);
            app.mark_dirty();
        }
        UserEvent::Chat(event) => match event {
            ChatEvent::Delta { content, .. } => {
                app.update_last_message_content(&content);
                app.mark_dirty();
            }
            ChatEvent::Finished { .. } => {
                let elapsed = app.thinking_start.map(|t| t.elapsed()).unwrap_or_default();
                app.waiting = false;
                app.thinking_start = None;
                app.status = String::new();
                app.mark_dirty();
                tracing::info!(
                    target: "tui::chat",
                    elapsed_ms = elapsed.as_millis(),
                    "chat stream finished",
                );
            }
            ChatEvent::Error { error, .. } => {
                app.waiting = false;
                app.thinking_start = None;
                app.status = format!("Error: {}", error);
                app.mark_dirty();
            }
            ChatEvent::Cancelled { .. } => {
                app.waiting = false;
                app.thinking_start = None;
                app.status = "Cancelled".into();
                app.mark_dirty();
            }
        },
        UserEvent::Input(ievt) => {
            match ievt {
                InputEvent::Key(key) => match app.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('i') => app.input_mode = InputMode::Insert,
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char('h') => app.show_help = !app.show_help,
                        KeyCode::Char('j') | KeyCode::Down => {
                            app.scroll = app.scroll.saturating_sub(1);
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.scroll = app.scroll.saturating_add(1);
                        }
                        _ => {}
                    },
                    InputMode::Insert => match key.code {
                        KeyCode::Enter => {
                            if app.waiting {
                                return;
                            }
                            if !app.input.is_empty() {
                                if app.input.starts_with('/') {
                                    let trimmed = app.input.trim().to_string();
                                    app.input.clear();
                                    app.cursor = 0;
                                    app.show_suggestions = false;
                                    command::handle_command(app, trimmed, evt_tx);
                                } else {
                                    let input = std::mem::take(&mut app.input);
                                    app.cursor = 0;
                                    app.show_suggestions = false;
                                    app.add_message(Role::User, &input);
                                    app.add_message(Role::Assistant, "");
                                    app.waiting = true;
                                    app.thinking_tick = 0;
                                    app.thinking_start = Some(Instant::now());
                                    app.scroll = 0;

                                    let tx = evt_tx.clone();
                                    let session_id = app.cur_session;
                                    let chat = chat_feature.clone();
                                    let rt = app.runtime.clone();
                                    tokio::spawn(async move {
                                        let sid = match session_id {
                                            Some(id) => id,
                                            None => {
                                                let id = rt.create_session(None).await;
                                                let _ = tx.send(UserEvent::Status(format!(
                                                    "Created new session: {}",
                                                    id
                                                )));
                                                id
                                            }
                                        };

                                        let cancel = CancellationToken::new();
                                        let mut stream = match chat
                                            .chat(
                                                sid,
                                                input,
                                                ModelSelection {
                                                    provider: "chatecnu".into(),
                                                    model: "ecnu-max".into(),
                                                },
                                                GenerationOptions {
                                                    stream: true,
                                                    ..Default::default()
                                                },
                                                cancel,
                                            )
                                            .await
                                        {
                                            Ok(s) => s,
                                            Err(e) => {
                                                let _ = tx.send(UserEvent::Chat(
                                                    ChatEvent::Error {
                                                        message_id: common::MessageId::new(),
                                                        error: e.to_string(),
                                                    },
                                                ));
                                                return;
                                            }
                                        };

                                        while let Some(event) = stream.next().await {
                                            if tx.send(UserEvent::Chat(event)).is_err() {
                                                return;
                                            }
                                        }
                                    });
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            app.input.insert(app.cursor, c);
                            app.cursor += c.len_utf8();
                            app.show_suggestions =
                                app.input.starts_with('/') && !app.input.is_empty();
                        }
                        KeyCode::Backspace => {
                            if app.cursor > 0 {
                                let prev = app.input[..app.cursor].chars().next_back().unwrap();
                                app.cursor -= prev.len_utf8();
                                app.input.remove(app.cursor);
                            }
                            app.show_suggestions =
                                app.input.starts_with('/') && !app.input.is_empty();
                        }
                        KeyCode::Delete => {
                            if app.cursor < app.input.len() {
                                app.input.remove(app.cursor);
                            }
                        }
                        KeyCode::Left => {
                            if app.cursor > 0 {
                                let prev = app.input[..app.cursor].chars().next_back().unwrap();
                                app.cursor -= prev.len_utf8();
                            }
                        }
                        KeyCode::Right => {
                            if app.cursor < app.input.len() {
                                let next = app.input[app.cursor..].chars().next().unwrap();
                                app.cursor += next.len_utf8();
                            }
                        }
                        KeyCode::Home => app.cursor = 0,
                        KeyCode::End => app.cursor = app.input.len(),
                        KeyCode::Esc => app.input_mode = InputMode::Normal,
                        _ => {}
                    },
                },
                InputEvent::ScrollUp => {
                    app.scroll = app.scroll.saturating_add(3);
                }
                InputEvent::ScrollDown => {
                    app.scroll = app.scroll.saturating_sub(3);
                }
            }
            app.mark_dirty();
        }
    }
}