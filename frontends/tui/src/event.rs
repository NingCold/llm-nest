use std::time::Instant;

use chat::{ChatEvent, ChatOptions, Role};
use crossterm::event::KeyCode;
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::app::{App, InputEvent, InputMode, UserEvent};
use crate::command;

pub fn handle_event(app: &mut App, uevt: UserEvent, evt_tx: &mpsc::UnboundedSender<UserEvent>) {
    match uevt {
        UserEvent::Chat(event) => match event {
            ChatEvent::ResponseDelta { content } => {
                app.update_last_message_content(&content);
                app.mark_dirty();
            }
            ChatEvent::ResponseFinished => {
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
            ChatEvent::Info(msg) => {
                app.status = msg;
                app.mark_dirty();
            }
            ChatEvent::Error { message } => {
                app.waiting = false;
                app.thinking_start = None;
                app.status = format!("Error: {}", message);
                app.mark_dirty();
            }
            _ => {}
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
                                    let svc = app.svc.clone();
                                    tokio::spawn(async move {
                                        tracing::debug!(target: "tui::chat", "sending chat request");
                                        match svc
                                            .chat_stream(
                                                input,
                                                ChatOptions {
                                                    stream: true,
                                                    ..Default::default()
                                                },
                                            )
                                            .await
                                        {
                                            Ok(mut stream) => {
                                                tracing::debug!(target: "tui::chat", "chat stream started");
                                                while let Some(event) = stream.next().await {
                                                    tracing::trace!(target: "tui::chat", "chat event: {event:?}");
                                                    if tx.send(UserEvent::Chat(event)).is_err() {
                                                        break;
                                                    }
                                                }
                                                tracing::debug!(target: "tui::chat", "chat stream ended");
                                            }
                                            Err(e) => {
                                                tracing::error!(target: "tui::chat", "chat request failed: {e}");
                                                let _ =
                                                    tx.send(UserEvent::Chat(ChatEvent::Error {
                                                        message: e.to_string(),
                                                    }));
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
