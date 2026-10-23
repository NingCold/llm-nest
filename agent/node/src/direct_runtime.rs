//! Private same-device RPC and local model jobs; the Agent owns all execution.
use crate::{
    client, consciousness, context, direct,
    store::{Binding, BudgetLimits, Config, Store},
};
use ai_client::{AiClient, ChatChunk, ModelRouter};
use anyhow::{Result, ensure};
use connector::{
    direct::{
        DirectBinding, DirectError, DirectPrepare, LocalOperation, LocalReply, LocalRequest,
        ipc::{Connection, Listener},
    },
    node::*,
};
use futures_util::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use std::{path::Path, time::Duration};
use tokio::sync::Semaphore;

async fn operation(
    config: &Config,
    store: &Store,
    bindings: &[Binding],
    sync_windows: &direct::Windows,
    request: LocalRequest,
) -> Result<LocalReply> {
    let owner = &request.owner_id;
    let cid = match request.operation {
        LocalOperation::Status => {
            return Ok(LocalReply::Status {
                bindings: bindings
                    .iter()
                    .map(|b| DirectBinding {
                        ai_id: b.ai.clone(),
                        node_id: config.node_id.clone(),
                        binding_version: b.version.clone(),
                    })
                    .collect(),
            });
        }
        LocalOperation::Prepare { ai_id, grant } => {
            let binding = bindings
                .iter()
                .find(|b| b.ai == ai_id)
                .ok_or_else(|| anyhow::anyhow!("AI is not bound to this node"))?;
            let api = client(config, &binding.secret)?;
            let bootstrap = api
                .direct_prepare(&DirectPrepare {
                    grant: grant.clone(),
                })
                .await?;
            ensure!(
                bootstrap.human.id == *owner
                    && bootstrap.ai.id == ai_id
                    && bootstrap.binding_version == binding.version,
                "direct_owner_or_binding_mismatch"
            );
            consciousness::sync(&api, store, &ai_id).await?;
            direct::cache(store, owner, &grant, &bootstrap)?;
            bootstrap.conversation.id
        }
        LocalOperation::View { conversation_id } => conversation_id,
        LocalOperation::Send {
            conversation_id,
            request_id,
            text,
        } => {
            direct::send(store, owner, &conversation_id, &request_id, &text)?;
            conversation_id
        }
        LocalOperation::Stop { conversation_id } => {
            direct::stop(store, owner, &conversation_id)?;
            conversation_id
        }
        LocalOperation::Sync { conversation_id } => {
            // Scope the caller before selecting credentials or issuing network requests.
            direct::view(store, owner, &conversation_id)?;
            if let Some(_sync) = sync_windows.enter(&conversation_id) {
                let (_, ai, _, _) = direct::binding(store, &conversation_id)?;
                let binding = bindings
                    .iter()
                    .find(|b| b.ai == ai)
                    .ok_or_else(|| anyhow::anyhow!("AI is not bound to this node"))?;
                let api = client(config, &binding.secret)?;
                direct::sync(store, &api, &conversation_id).await?;
            }
            conversation_id
        }
    };
    Ok(LocalReply::View {
        view: Box::new(direct::view(store, owner, &cid)?),
    })
}

async fn handle_connection(
    mut connection: Connection,
    config: &Config,
    store: &Store,
    bindings: &[Binding],
    sync_windows: &direct::Windows,
) -> Result<()> {
    let request = tokio::time::timeout(Duration::from_secs(10), connection.receive()).await??;
    let reply = match operation(config, store, bindings, sync_windows, request).await {
        Ok(reply) => reply,
        Err(error) => LocalReply::Error {
            error: DirectError {
                code: "direct_unavailable".into(),
                message: error.to_string(),
            },
        },
    };
    connection.respond(&reply).await?;
    Ok(())
}

fn current(store: &Store, request: &str, ai: &str, execution: &direct::Execution) -> Result<()> {
    ensure!(
        direct::active(store, request)? && store.scopes_current(ai, &execution.scopes)?,
        "source_cancelled"
    );
    Ok(())
}

async fn generate(
    config: &Config,
    store: &Store,
    bindings: &[Binding],
    llm: &AiClient,
    request_id: &str,
    cid: &str,
    owner: &str,
) -> Result<()> {
    let mut execution = direct::Execution::default();
    let mut text = String::new();
    let outcome = async {
        let (_, ai, _, bootstrap) = direct::binding(store, cid)?;
        let binding = bindings.iter().find(|b| b.ai == ai).ok_or_else(|| anyhow::anyhow!("AI is not bound to this node"))?;
        ensure!(bootstrap.binding_version == binding.version, "direct_binding_changed");
        let api = client(config, &binding.secret)?;
        let profile = store.profile(&ai)?;
        let (human, _, _) = direct::turn(store, request_id)?;
        let view = direct::view(store, owner, cid)?;
        let messages: Vec<_> = view.messages.into_iter().filter(|m| m.status == "committed").map(|m| m.message).collect();
        let material = TaskMaterials {
            task: NodeTask { id: request_id.into(), ai_id: ai.clone(), conversation_id: cid.into(), binding_version: binding.version.clone(), revision: "1".into(), trigger: NodeTrigger::Message { message_id: human.id.clone(), input_ids: vec![human.id] }, automatic: false, status: "queued".into(), expires_at: (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339() },
            ai: bootstrap.ai, conversation: bootstrap.conversation, reference: None,
            sources: messages.iter().map(|m| SourceRevision { message_id: m.id.clone(), revision: m.revision.clone() }).collect(), messages,
        };
        let router = ModelRouter::new(&config.providers)?;
        let (request, tokens, derived, scopes, sources) = context::prepare_direct(&api, &router, &profile, material, Some(store)).await?;
        execution.sources = sources;
        execution.derived = derived;
        execution.scopes = scopes;
        execution.profile = profile.revision;
        current(store, request_id, &ai, &execution)?;
        store.reserve_local(request_id, &ai, &chrono::Utc::now().format("%Y-%m-%d").to_string(), tokens, BudgetLimits { calls: config.daily_calls, tokens: config.daily_tokens, automatic: None })?;
        direct::begin(store, request_id, &execution)?;
        let snapshot = llm.snapshot().await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        let mut checks = tokio::time::interval(Duration::from_millis(500));
        let opening = snapshot.complete_stream(request);
        tokio::pin!(opening);
        let mut stream = loop {
            tokio::select! {
                biased;
                _ = checks.tick() => current(store, request_id, &ai, &execution)?,
                _ = tokio::time::sleep_until(deadline) => anyhow::bail!("generation_timeout"),
                result = &mut opening => break result?,
            }
        };
        loop {
            tokio::select! {
                biased;
                _ = checks.tick() => {
                    current(store, request_id, &ai, &execution)?;
                    direct::save(store, request_id, &text, "running", &execution)?;
                },
                _ = tokio::time::sleep_until(deadline) => anyhow::bail!("generation_timeout"),
                chunk = stream.next() => match chunk {
                    Some(Ok(ChatChunk::Delta { content })) => { ensure!(text.len() + content.len() <= 256 * 1024, "output_limit"); text.push_str(&content); },
                    Some(Ok(ChatChunk::Done { usage })) => {
                        execution.usage = usage.map(|u| NodeUsage { input_tokens: u.prompt_tokens as u64, output_tokens: u.completion_tokens as u64, cached_tokens: u.cached_tokens as u64 });
                        current(store, request_id, &ai, &execution)?;
                        return Ok::<_, anyhow::Error>(());
                    },
                    Some(Ok(ChatChunk::ReasoningDelta { .. })) => {},
                    Some(Ok(ChatChunk::ToolCall { .. })) => anyhow::bail!("tools_not_enabled"),
                    Some(Err(error)) => return Err(error.into()),
                    None => anyhow::bail!("provider_stream_incomplete"),
                }
            }
        }
    }.await;
    let (state, error) = crate::generation_outcome(outcome, &text);
    execution.error = error.map(str::to_string);
    direct::save(
        store,
        request_id,
        &text,
        if state == "completed" {
            "committed"
        } else {
            state
        },
        &execution,
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn serve(
    config: &Config,
    store: &Store,
    _directory: &Path,
    bindings: &[Binding],
    llm: &AiClient,
    slots: &Semaphore,
    windows: &direct::Windows,
    listener: &mut Listener,
) -> Result<()> {
    let sync_windows = direct::Windows::default();
    let mut jobs: FuturesUnordered<LocalBoxFuture<'_, ()>> = FuturesUnordered::new();
    let mut wake = tokio::time::interval(Duration::from_millis(100));
    let mut sync = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            connection = listener.accept() => {
                let connection = connection?;
                let sync_windows = &sync_windows;
                jobs.push(async move { if let Err(error) = handle_connection(connection, config, store, bindings, sync_windows).await { eprintln!("Local IPC failed: {error}"); } }.boxed_local());
            },
            _ = jobs.next(), if !jobs.is_empty() => {},
            _ = wake.tick() => {
                for (request, cid, owner) in direct::pending(store)? {
                    let Some(guard) = windows.enter(&cid) else { continue; };
                    let Ok(slot) = slots.try_acquire() else { break; };
                    jobs.push(async move {
                        let (_guard, _slot) = (guard, slot);
                        if let Err(error) = generate(config, store, bindings, llm, &request, &cid, &owner).await { eprintln!("Local execution save failed: {error}"); }
                    }.boxed_local());
                }
            },
            _ = sync.tick() => {
                for (cid, _, ai) in direct::windows(store)? {
                    let Some(guard) = sync_windows.enter(&cid) else { continue; };
                    let Some(binding) = bindings.iter().find(|b| b.ai == ai) else { continue; };
                    let api = client(config, &binding.secret)?;
                    jobs.push(async move { let _guard = guard; let _ = direct::sync(store, &api, &cid).await; }.boxed_local());
                }
            },
        }
    }
}
