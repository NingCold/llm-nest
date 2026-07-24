mod command;

use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use events::ChatEvent;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use llm::{GenerationOptions, ModelSelection};
use rustyline::DefaultEditor;
use runtime::runtime::Runtime;
use tokio_util::sync::CancellationToken;

use command::Command;

#[tokio::main]
async fn main() -> Result<()> {
    let runtime = Runtime::from_config("config/llmn.toml")?;

    let chat = Arc::new(chat::ChatFeature::new());
    runtime.register_feature(chat.clone()).await;
    runtime.initialize_features().await?;

    let sessions = runtime.list_sessions().await;
    let mut session_id = if sessions.is_empty() {
        runtime.create_session(None).await
    } else {
        sessions[0]
    };

    println!("==============================");
    println!("         LLM Nest Chat");
    println!("      Type /help for help");
    println!("==============================");

    let mut rl = DefaultEditor::new()?;

    loop {
        let input = rl.readline("You>")?;
        rl.add_history_entry(&input)?;

        if input.trim().is_empty() {
            continue;
        }

        if let Some(cmd) = Command::parse(&input) {
            match cmd {
                Command::New => {
                    let id = runtime.create_session(None).await;
                    session_id = id;
                    println!("已创建新会话");
                }
                Command::Switch { target } => {
                    if let Ok(id) = target.parse::<common::SessionId>() {
                        if runtime.get_session(&id).await.is_some() {
                            session_id = id;
                            println!("已切换到: {}", target);
                            continue;
                        }
                    }
                    let ids = runtime.list_sessions().await;
                    let mut found = false;
                    for id in &ids {
                        if let Some(s) = runtime.get_session(id).await {
                            if s.title() == Some(target.as_str()) {
                                session_id = *id;
                                println!("已切换到: {}", target);
                                found = true;
                                break;
                            }
                        }
                    }
                    if !found {
                        println!("未找到会话: {}", target);
                    }
                }
                Command::Rename { title } => {
                    runtime.rename_session(session_id, title).await?;
                    println!("已重命名");
                }
                Command::Delete { id } => {
                    if let Ok(sid) = id.parse::<common::SessionId>() {
                        runtime.delete_session(sid).await?;
                        println!("已删除: {}", id);
                    }
                }
                Command::List => {
                    let sessions = runtime.list_sessions().await;
                    if sessions.is_empty() {
                        println!("没有会话");
                    } else {
                        for id in &sessions {
                            let marker = if *id == session_id { "*" } else { " " };
                            let title = runtime
                                .get_session(id)
                                .await
                                .and_then(|s| s.title().map(String::from))
                                .unwrap_or_default();
                            println!(" {} {}  {}", marker, id, title);
                        }
                    }
                }
                Command::Help => {
                    println!("{}", Command::help_text());
                }
                Command::Quit => return Ok(()),
            }
            continue;
        }

        let model = ModelSelection {
            provider: "chatecnu".into(),
            model: "ecnu-max".into(),
        };
        let options = GenerationOptions {
            stream: true,
            ..Default::default()
        };

        let cancel = CancellationToken::new();
        let mut stream = chat
            .chat(session_id, input, model, options, cancel.clone())
            .await?;

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(ProgressStyle::with_template("{spinner} {msg}")?);
        spinner.set_message("Thinking · 0.0s");

        let spinner_handle = {
            let spinner = spinner.clone();
            tokio::spawn(async move {
                let start = std::time::Instant::now();
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let secs = start.elapsed().as_secs_f64();
                    spinner.set_message(format!("Thinking · {secs:.1}s"));
                }
            })
        };

        let mut has_content = false;
        while let Some(event) = stream.next().await {
            match event {
                ChatEvent::Delta { content, .. } => {
                    if !has_content {
                        spinner_handle.abort();
                        spinner.finish_and_clear();
                        has_content = true;
                    }
                    print!("{}", content);
                    io::stdout().flush()?;
                }
                ChatEvent::Finished { .. } => {
                    if !has_content {
                        spinner_handle.abort();
                        spinner.finish_and_clear();
                    }
                    println!();
                }
                ChatEvent::Error { error, .. } => {
                    spinner_handle.abort();
                    spinner.finish_and_clear();
                    eprintln!("\nError: {}", error);
                }
                ChatEvent::Cancelled { .. } => {
                    spinner_handle.abort();
                    spinner.finish_and_clear();
                }
            }
        }
        if !has_content {
            spinner_handle.abort();
            spinner.finish_and_clear();
        }
    }
}