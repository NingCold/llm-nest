mod command;

use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use common::GenerationOptions;
use events::ChatEvent;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use runtime::config::ConfigWatchEvent;
use runtime::runtime::Runtime;
use rustyline::DefaultEditor;
use tokio_util::sync::CancellationToken;

use command::Command;

/// Configuration document this binary loads at startup and hot-reloads.
const CONFIG_PATH: &str = "config/llmn.toml";

#[tokio::main]
async fn main() -> Result<()> {
    runtime::worker_entry();
    let runtime = Arc::new(Runtime::from_config_persistent(
        CONFIG_PATH,
        storage::default_data_dir(),
    )?);

    let chat = Arc::new(chat::ChatFeature::new());
    runtime.register_feature(chat.clone()).await;
    runtime.initialize_features().await?;

    let sessions = runtime.list_sessions().await;
    let mut session_id = if sessions.is_empty() {
        runtime.create_session(None).await?
    } else {
        sessions[0]
    };

    let mut current: Option<ai_client::ModelSelection> = runtime.default_model().await;
    if current.is_none() {
        eprintln!("\nError: no configured models");
        return Ok(());
    }

    // Hot reload: file edits are re-applied automatically; failures keep the
    // running configuration and are reported here.
    let (watch_tx, mut watch_rx) = tokio::sync::mpsc::unbounded_channel::<ConfigWatchEvent>();
    runtime::config::spawn_config_watcher(runtime.clone(), CONFIG_PATH, watch_tx)?;
    tokio::spawn(async move {
        while let Some(event) = watch_rx.recv().await {
            match event {
                ConfigWatchEvent::Reloaded => {
                    println!(
                        "\n[配置] 已热更新。若当前模型不可用，用 /models 查看后 /model 切换。"
                    );
                }
                ConfigWatchEvent::Failed(message) => {
                    eprintln!("\n[配置] 重载失败，保留旧配置: {message}");
                }
            }
        }
    });

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
                    let id = runtime.create_session(None).await?;
                    session_id = id;
                    // 新会话没有记住的模型 → 回退全局默认
                    current = runtime.session_model(&session_id).await;
                    println!("已创建新会话");
                }
                Command::Switch { target } => {
                    if let Ok(id) = target.parse::<common::SessionId>()
                        && runtime.get_session(&id).await.is_some()
                    {
                        session_id = id;
                        // 会话记住的模型（无则全局默认）
                        current = runtime.session_model(&session_id).await;
                        println!("已切换到: {}", target);
                        continue;
                    }
                    let ids = runtime.list_sessions().await;
                    let mut found = false;
                    for id in &ids {
                        if let Some(s) = runtime.get_session(id).await
                            && s.title() == Some(target.as_str())
                        {
                            session_id = *id;
                            current = runtime.session_model(&session_id).await;
                            println!("已切换到: {}", target);
                            found = true;
                            break;
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
                Command::Models => {
                    let models = runtime.list_models().await;
                    if models.is_empty() {
                        println!("没有可用模型");
                    } else {
                        println!("可用模型:");
                        for m in &models {
                            let reasoning = m
                                .spec
                                .reasoning
                                .as_ref()
                                .map(|c| {
                                    format!(
                                        " [reasoning: {} · {}]",
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
                            println!(
                                " {} {}/{} ({}){reasoning}",
                                mark, m.provider, m.spec.wire, m.spec.display_name
                            );
                        }
                    }
                }
                Command::Model { target } => {
                    if target.is_empty() {
                        let m = match &current {
                            Some(c) => format!("当前模型: {}/{}", c.provider, c.model),
                            None => "当前模型: 未设置".to_string(),
                        };
                        println!("{} (用法: /model <provider/model>)", m);
                        continue;
                    }
                    match runtime.select_model(&target).await {
                        Ok(sel) => {
                            // 模型选择绑定到当前会话
                            if let Err(e) =
                                runtime.set_session_model(&session_id, sel.clone()).await
                            {
                                println!("{}", e);
                                continue;
                            }
                            current = Some(sel.clone());
                            println!("已切换到模型: {}/{}", sel.provider, sel.model);
                        }
                        Err(e) => println!("{}", e),
                    }
                }
                Command::Effort { level } => {
                    let Some(current_sel) = current.clone() else {
                        println!("未设置当前模型，先 /model <provider/model>");
                        continue;
                    };
                    let Some(level) = level else {
                        let shown = current_sel
                            .reasoning_effort
                            .map(|e| e.as_wire().to_string())
                            .unwrap_or_else(|| "未设置".to_string());
                        println!(
                            "当前 effort: {}（当前模型 {}/{}，用法: /effort <off|low|medium|high|max>）",
                            shown, current_sel.provider, current_sel.model
                        );
                        continue;
                    };
                    let Some(effort) = parse_effort(&level) else {
                        // 列出当前模型实际支持的级别（off 恒可），而不是全局枚举
                        let supported = match runtime.resolve_model(&current_sel).await {
                            Ok(resolved) => resolved
                                .spec
                                .reasoning
                                .map(|cap| {
                                    let mut levels: Vec<&str> = vec!["off"];
                                    levels.extend(cap.levels.iter().map(|l| l.as_wire()));
                                    levels.join(" / ")
                                })
                                .unwrap_or_else(|| "off（该模型不支持 reasoning）".to_string()),
                            Err(_) => "off / low / medium / high".to_string(),
                        };
                        println!(
                            "无效 effort: {level}（{}/{} 实际支持: {supported}）",
                            current_sel.provider, current_sel.model
                        );
                        continue;
                    };
                    // 用当前模型的 provider/model 构造新选择并走路由校验，
                    // 确保该模型声明支持这个级别。
                    let candidate = ai_client::ModelSelection {
                        reasoning_effort: Some(effort),
                        ..current_sel.clone()
                    };
                    match runtime.resolve_model(&candidate).await {
                        Ok(_) => {
                            // effort 同样绑定到当前会话
                            if let Err(e) = runtime
                                .set_session_model(&session_id, candidate.clone())
                                .await
                            {
                                println!("{}", e);
                                continue;
                            }
                            current = Some(candidate);
                            println!(
                                "已设置 effort: {}（{}/{}）",
                                level, current_sel.provider, current_sel.model
                            );
                        }
                        Err(e) => println!("{}", e),
                    }
                }
                Command::Current => match &current {
                    Some(sel) => {
                        println!("当前模型:");
                        println!("  provider: {}", sel.provider);
                        println!("  model:    {}", sel.model);
                        match sel.reasoning_effort {
                            Some(e) => println!("  effort:   {}", e.as_wire()),
                            None => println!("  effort:   （未设置，走 provider 默认）"),
                        }
                    }
                    None => println!("未设置当前模型"),
                },
                Command::Reload => match runtime.reload_config(CONFIG_PATH).await {
                    Ok(()) => println!("已重新加载配置"),
                    Err(e) => eprintln!("重载失败（保留旧配置）: {e}"),
                },
                Command::Refresh { provider } => {
                    if provider.is_empty() {
                        println!(
                            "用法: /refresh <provider>（从该 provider 的 GET /models 合并新模型并写回配置）"
                        );
                        continue;
                    }
                    match runtime.refresh_models(&provider).await {
                        Ok(added) if added.is_empty() => {
                            println!("{provider}: 无新模型（现有清单已是最新）")
                        }
                        Ok(added) => println!(
                            "{provider}: 已合并 {} 个新模型并写回 config/llmn.toml",
                            added.len()
                        ),
                        Err(e) => eprintln!("{provider} 刷新失败: {e}"),
                    }
                }
                Command::Help => {
                    println!("{}", Command::help_text());
                }
                Command::Quit => return Ok(()),
            }
            continue;
        }

        let model = match &current {
            Some(m) => m.clone(),
            None => {
                eprintln!("\nError: no configured models");
                return Ok(());
            }
        };
        let options = GenerationOptions {
            stream: true,
            ..Default::default()
        };

        let cancel = CancellationToken::new();
        let mut stream = chat
            .chat(
                session_id,
                input,
                Vec::new(),
                model,
                options,
                cancel.clone(),
            )
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
        let mut reasoning_active = false;
        while let Some(event) = stream.next().await {
            match event {
                ChatEvent::ReasoningDelta { content, .. } => {
                    if !has_content {
                        spinner_handle.abort();
                        spinner.finish_and_clear();
                        has_content = true;
                    }
                    // 思维链用浅色（dim）显示，正文开始前复位。
                    if !reasoning_active {
                        print!("\x1b[2m");
                        reasoning_active = true;
                    }
                    print!("{}", content);
                    io::stdout().flush()?;
                }
                ChatEvent::Delta { content, .. } => {
                    if !has_content {
                        spinner_handle.abort();
                        spinner.finish_and_clear();
                        has_content = true;
                    }
                    if reasoning_active {
                        print!("\x1b[0m");
                        reasoning_active = false;
                    }
                    print!("{}", content);
                    io::stdout().flush()?;
                }
                ChatEvent::Finished { .. } => {
                    if reasoning_active {
                        print!("\x1b[0m");
                        reasoning_active = false;
                    }
                    if !has_content {
                        spinner_handle.abort();
                        spinner.finish_and_clear();
                    }
                    println!();
                }
                ChatEvent::Error { error, .. } => {
                    if reasoning_active {
                        print!("\x1b[0m");
                        reasoning_active = false;
                    }
                    spinner_handle.abort();
                    spinner.finish_and_clear();
                    eprintln!("\nError: {}", error);
                }
                ChatEvent::ToolCall {
                    name, arguments, ..
                } => {
                    if reasoning_active {
                        print!("\x1b[0m");
                        reasoning_active = false;
                    }
                    spinner_handle.abort();
                    spinner.finish_and_clear();
                    println!("\n[tool] {} {arguments}", name);
                }
                ChatEvent::ToolResult {
                    name,
                    content,
                    is_error,
                    duration_ms,
                    ..
                } => {
                    let ms = duration_ms
                        .map(|d| format!(" ({}ms)", d))
                        .unwrap_or_default();
                    if is_error {
                        println!("[tool] {name} error: {content}{ms}");
                    } else {
                        println!("[tool] {name} -> {content}{ms}");
                    }
                }
                ChatEvent::Cancelled { .. } => {
                    if reasoning_active {
                        print!("\x1b[0m");
                        reasoning_active = false;
                    }
                    spinner_handle.abort();
                    spinner.finish_and_clear();
                }
            }
        }
        if reasoning_active {
            print!("\x1b[0m");
        }
        if !has_content {
            spinner_handle.abort();
            spinner.finish_and_clear();
        }
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
