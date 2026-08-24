//! Real end-to-end tool-calling check: drives ChatFeature against a live
//! provider (config/llmn.toml) with the builtin `add` tool registered, and
//! requires the full agent loop — model tool call → execution → result →
//! final answer — to complete.
//!
//! Usage (from the workspace root):
//!   cargo run -p chat --example tool_e2e -- <provider> <model> [prompt] [config]
//! e.g.
//!   GEMINI_API_KEY=... cargo run -p chat --example tool_e2e -- gemini gemini-2.5-flash
//!   cargo run -p chat --example tool_e2e -- chatecnu-anthropic ecnu-max
//!   cargo run -p chat --example tool_e2e -- chatecnu-responses ecnu-max

use std::sync::Arc;

use ai_client::ModelSelection;
use chat::ChatFeature;
use common::{ContentPart, GenerationOptions};
use events::ChatEvent;
use futures_util::StreamExt;
use runtime::runtime::Runtime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let provider = args.next().unwrap_or_else(|| "chatecnu".into());
    let model = args.next().unwrap_or_else(|| "ecnu-max".into());
    let prompt = args.next().unwrap_or_else(|| {
        "请调用 add 工具计算 6+4，然后告诉我结果。".to_string()
    });
    let config = args.next().unwrap_or_else(|| "config/llmn.toml".into());

    println!("== provider={provider} model={model}");
    println!("== prompt: {prompt}");

    let runtime = Runtime::from_config(&config)?;
    let chat = Arc::new(ChatFeature::new());
    runtime.register_feature(chat.clone()).await;
    runtime.initialize_features().await?;

    let session_id = runtime.create_session(None).await?;
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut stream = chat
        .chat(
            session_id,
            prompt,
            Vec::<ContentPart>::new(),
            ModelSelection {
                provider,
                model,
                reasoning_effort: None,
            },
            GenerationOptions::default(),
            cancel,
        )
        .await?;

    let mut saw_call = false;
    let mut saw_result = false;
    let mut saw_finish = false;
    let mut final_text = String::new();
    while let Some(ev) = stream.next().await {
        match ev {
            ChatEvent::ToolCall { name, arguments, .. } => {
                saw_call = true;
                println!("[tool_call] {name} {arguments}");
            }
            ChatEvent::ToolResult {
                name,
                content,
                is_error,
                duration_ms,
                ..
            } => {
                saw_result = true;
                println!(
                    "[tool_result] {name} -> {content} err={is_error} dur={}ms",
                    duration_ms.unwrap_or(0)
                );
            }
            ChatEvent::Delta { content, .. } => {
                final_text.push_str(&content);
                print!("{content}");
            }
            ChatEvent::ReasoningDelta { content, .. } => print!("〔{content}〕"),
            ChatEvent::Finished {
                usage, timings, ..
            } => {
                saw_finish = true;
                println!();
                println!("[finished] usage={usage:?} timings={timings:?}");
            }
            ChatEvent::Error { error, .. } => {
                eprintln!();
                eprintln!("[error] {error}");
                std::process::exit(1);
            }
            ChatEvent::Cancelled { .. } => {
                println!();
                println!("[cancelled]");
                std::process::exit(3);
            }
        }
    }
    println!();
    println!("== final text: {final_text}");
    println!("== session history:");
    for m in runtime.get_messages(&session_id).await {
        println!("  [{:?}] {:?}", m.role, m.content);
    }
    if !(saw_call && saw_result && saw_finish) {
        eprintln!(
            "MISSING EVENTS: tool_call={saw_call} tool_result={saw_result} finished={saw_finish}"
        );
        std::process::exit(2);
    }
    Ok(())
}
