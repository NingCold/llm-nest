mod command;

use std::io::{self, Write};
use std::time::Duration;

use anyhow::Result;
use chat::{ChatEvent, ChatOptions, ChatService};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use rustyline::DefaultEditor;

use command::Command;

#[tokio::main]
async fn main() -> Result<()> {
    let svc = ChatService::from_config("config/llmn.toml")?;
    let mut rl = DefaultEditor::new()?;

    let _session = svc.new_session(None).await?;

    println!("==============================");
    println!("         LLM Nest Chat");
    println!("      Type /help for help");
    println!("==============================");

    loop {
        let input = rl.readline("You>")?;
        rl.add_history_entry(&input)?;

        if input.trim().is_empty() {
            continue;
        }

        if let Some(cmd) = Command::parse(&input) {
            match cmd {
                Command::New => {
                    svc.new_session(None).await?;
                    println!("新会话已创建");
                }

                Command::List => {
                    let sessions = svc.list_sessions().await?;
                    if sessions.is_empty() {
                        println!("没有会话");
                    } else {
                        let cur = svc.current_session().await;
                        for (id, title) in &sessions {
                            let marker = cur.as_ref().map_or(" ", |c| {
                                if c.to_string() == *id { "*" } else { " " }
                            });
                            println!(" {} {}  {}", marker, id, title);
                        }
                    }
                }

                Command::Switch { target } => {
                    let found = svc.switch_session(&target).await?;
                    if found {
                        println!("已切换到: {}", target);
                    } else {
                        println!("未找到会话: {}", target);
                    }
                }

                Command::Rename { title } => {
                    let cur = svc.current_session().await
                        .ok_or_else(|| anyhow::anyhow!("没有活跃会话"))?;
                    svc.rename_session(&cur.to_string(), title).await?;
                    println!("已重命名");
                }

                Command::Delete { id } => {
                    svc.delete_session(&id).await?;
                    println!("已删除: {}", id);
                }

                Command::Help => {
                    println!("{}", Command::help_text());
                }

                Command::Quit => {
                    break;
                }
            }
            continue;
        }

        let mut stream = svc.chat_stream(input, ChatOptions {
            stream: true,
            ..Default::default()
        }).await?;

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
                ChatEvent::ResponseStarted => {
                }
                ChatEvent::ResponseDelta { content } => {
                    if !has_content {
                        spinner_handle.abort();
                        spinner.finish_and_clear();
                        has_content = true;
                    }
                    print!("{}", content);
                    io::stdout().flush()?;
                }
                ChatEvent::ResponseFinished => {
                    if !has_content {
                        spinner_handle.abort();
                        spinner.finish_and_clear();
                    }
                    println!();
                }
                ChatEvent::Info(msg) => {
                    spinner_handle.abort();
                    spinner.finish_and_clear();
                    println!("\n[{}]", msg);
                }
                ChatEvent::Error { message } => {
                    spinner_handle.abort();
                    spinner.finish_and_clear();
                    eprintln!("\nError: {}", message);
                }
            }
        }
    }

    Ok(())
}