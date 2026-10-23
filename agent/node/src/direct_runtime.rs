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
            let mut bootstrap = api
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
            crate::crypto::sync(&api, store, config, binding).await?;
            crate::crypto::decrypt_bootstrap(&api, store, &mut bootstrap).await?;
            direct::cache_bootstrap_downloads(&api, store, &grant, &bootstrap).await?;
            direct::cache(store, owner, &grant, &bootstrap)?;
            bootstrap.conversation.id
        }
        LocalOperation::View { conversation_id } => conversation_id,
        LocalOperation::Send {
            conversation_id,
            request_id,
            text,
            sealed,
            message_id,
            key_messages,
            encrypted_attachments,
            reply_to,
            reference,
            expected_conversation_revision,
        } => {
            direct::view(store, owner, &conversation_id)?;
            let (_, ai, _, bootstrap) = direct::binding(store, &conversation_id)?;
            if bootstrap.conversation.encryption.is_some() {
                ensure!(
                    expected_conversation_revision.as_ref()
                        == Some(&bootstrap.conversation.revision),
                    "direct_conversation_changed"
                );
            }
            if let Some(selection) = &reference {
                direct::reference(store, &conversation_id, selection)?;
            }
            if let Some(reply) = &reply_to {
                ensure!(
                    direct::view(store, owner, &conversation_id)?
                        .messages
                        .iter()
                        .any(|m| m.message.id == *reply),
                    "direct_reply_missing"
                );
            }
            ensure!(
                !crate::crypto::draining(store, &ai)?,
                "node handover is draining"
            );
            let binding = bindings
                .iter()
                .find(|b| b.ai == ai)
                .ok_or_else(|| anyhow::anyhow!("AI is not bound"))?;
            let api = client(config, &binding.secret)?;
            let message_id = if bootstrap.conversation.encryption.is_none() {
                Some(message_id.unwrap_or_else(|| request_id.clone()))
            } else {
                message_id
            };
            let input = crate::crypto::direct_input(
                &api,
                store,
                &ai,
                &conversation_id,
                owner,
                &text,
                sealed,
                message_id,
                key_messages,
                &encrypted_attachments,
                reply_to,
                reference,
                expected_conversation_revision,
            )
            .await?;
            let decoded = input.as_ref().map(|m| m.text.clone()).unwrap_or(text);
            direct::send_message(
                store,
                owner,
                &conversation_id,
                &request_id,
                &decoded,
                input,
                &encrypted_attachments,
            )?;
            conversation_id
        }
        LocalOperation::PrepareReference {
            conversation_id,
            selection,
        } => {
            direct::view(store, owner, &conversation_id)?;
            let (_, ai, grant, bootstrap) = direct::binding(store, &conversation_id)?;
            let binding = bindings
                .iter()
                .find(|b| b.ai == ai)
                .ok_or_else(|| anyhow::anyhow!("AI is not bound"))?;
            let api = client(config, &binding.secret)?;
            ensure!(
                bootstrap.binding_version == binding.version,
                "direct_binding_changed"
            );
            let mut reference = match api
                .direct_reference(
                    &ai,
                    &connector::direct::DirectReferenceRequest {
                        grant: grant.clone(),
                        selection: selection.clone(),
                    },
                )
                .await
            {
                Ok(reference) => reference,
                Err(NodeError::Http(error)) if error.is_connect() || error.is_timeout() => {
                    return Ok(LocalReply::Reference {
                        reference: direct::reference(store, &conversation_id, &selection)?,
                    });
                }
                Err(error) => {
                    if matches!(
                        error,
                        NodeError::Api {
                            status: 403 | 404 | 409,
                            ..
                        }
                    ) {
                        direct::discard_reference(store, &conversation_id, &selection)?;
                    }
                    return Err(error.into());
                }
            };
            crate::crypto::decrypt_reference(&api, store, &ai, &mut reference).await?;
            direct::cache_downloads(
                &api,
                store,
                &ai,
                &grant,
                &reference.conversation.id,
                &reference.messages,
            )
            .await?;
            direct::cache_reference(store, &conversation_id, &selection, &reference)?;
            return Ok(LocalReply::Reference { reference });
        }
        LocalOperation::Attachment {
            conversation_id,
            message_id,
            index,
            revision,
        } => {
            let view = direct::view(store, owner, &conversation_id)?;
            ensure!(
                view.messages.iter().any(|m| m.message.id == message_id
                    && m.message.revision == revision
                    && m.message.attachments.iter().any(|a| a.index == index)),
                "direct_attachment_unavailable"
            );
            use base64::Engine;
            return Ok(LocalReply::Attachment {
                data: base64::engine::general_purpose::STANDARD.encode(direct::attachment(
                    store,
                    &conversation_id,
                    &message_id,
                    &revision,
                    index,
                )?),
            });
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
    let mut reply = match operation(config, store, bindings, sync_windows, request).await {
        Ok(reply) => reply,
        Err(error) => LocalReply::Error {
            error: DirectError {
                code: "direct_unavailable".into(),
                message: error.to_string(),
            },
        },
    };
    if let LocalReply::View { view } = &mut reply
        && let Some(encryption) = &view.conversation.encryption
    {
        let activation = encryption.activation_position.parse::<u64>()?;
        view.messages.retain(|row| {
            row.message.sealed.is_some()
                || row
                    .message
                    .position
                    .parse::<u64>()
                    .is_ok_and(|position| position <= activation)
        });
        for row in &mut view.messages {
            if row.message.sealed.is_some() {
                row.message.text.clear();
                row.message.attachments = direct::wire_attachments(&row.message);
            }
        }
    }
    if let LocalReply::Reference { reference } = &mut reply {
        for m in &mut reference.messages {
            if m.sealed.is_some() {
                m.text.clear();
                m.attachments = direct::wire_attachments(m);
            }
        }
    }
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
    let mut sequence = 0u64;
    let outcome = async {
        let (_, ai, _, bootstrap) = direct::binding(store, cid)?;
        let binding = bindings.iter().find(|b| b.ai == ai).ok_or_else(|| anyhow::anyhow!("AI is not bound to this node"))?;
        ensure!(bootstrap.binding_version == binding.version, "direct_binding_changed");
        ensure!(!crate::crypto::draining(store,&ai)?,"node handover is draining");
        if bootstrap.conversation.encryption.is_some(){crate::crypto::ensure_ready(store,&ai)?;}
        let api = client(config, &binding.secret)?;
        crate::crypto::prepare_direct_run(store,&ai,cid,request_id,&bootstrap.conversation.members).await?;
        let profile = store.profile(&ai)?;
        let (human, _, _) = direct::turn(store, request_id)?;
        let view = direct::view(store, owner, cid)?;
        let messages: Vec<_> = view.messages.into_iter().filter(|m| m.status == "committed").map(|m| m.message).collect();
        let reference = direct::selected_reference(store,cid,&human)?;
        let material = TaskMaterials {
            task: NodeTask { id: request_id.into(), ai_id: ai.clone(), conversation_id: cid.into(), binding_version: binding.version.clone(), revision: "1".into(), trigger: NodeTrigger::Message { message_id: human.id.clone(), input_ids: vec![human.id] }, automatic: false, status: "queued".into(), expires_at: (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339() },
            ai: bootstrap.ai, conversation: bootstrap.conversation, reference,
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
                    direct::save(store, request_id, &text, "running", &execution,None)?;
                    sequence+=1;
                    let sealed=crate::crypto::direct_output(&api,store,&ai,cid,request_id,Some(sequence)).await?;
                    direct::save(store,request_id,&text,"running",&execution,sealed)?;
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
    let (_, ai, _, _) = direct::binding(store, cid)?;
    let binding = bindings.iter().find(|b| b.ai == ai).unwrap();
    let api = client(config, &binding.secret)?;
    direct::save(store, request_id, &text, "running", &execution, None)?;
    let sealed = crate::crypto::direct_output(&api, store, &ai, cid, request_id, None).await;
    let sealed = match sealed {
        Ok(value) => value,
        Err(error) => {
            execution.error = Some("encryption_failed".into());
            direct::save(store, request_id, &text, "uncertain", &execution, None)?;
            return Err(error);
        }
    };
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
        sealed,
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

#[cfg(test)]
mod tests {
    use super::*;
    use connector::direct::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn offline_reference_requires_prior_preparation_and_never_masks_forbidden() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        direct::initialize(&store).unwrap();
        let ai = crate::id();
        let owner = crate::id();
        let cid = crate::id();
        let source = crate::id();
        let bootstrap: DirectBootstrap=serde_json::from_value(json!({"ai":{"id":ai,"name":"ai","bio":"","kind":"ai"},"human":{"id":owner,"name":"human","bio":"","kind":"human"},"conversation":{"id":cid,"kind":"direct","name":"chat","revision":"1","members":[]},"bindingVersion":"1","base":null,"messages":[],"hasMore":false})).unwrap();
        direct::cache(&store, &owner, "grant", &bootstrap).unwrap();
        let selection = ReferenceSelection {
            conversation_id: source.clone(),
            sources: vec![SourceRevision {
                message_id: crate::id(),
                revision: "1".into(),
            }],
        };
        let reference = NodeReference {
            message_id: crate::id(),
            revision: "1".into(),
            required: true,
            conversation: NodeConversation {
                id: source,
                kind: "group".into(),
                name: "group".into(),
                revision: "1".into(),
                members: vec![],
                encryption: None,
            },
            messages: vec![],
            sources: selection.sources.clone(),
        };
        let mut config: Config=serde_json::from_value(json!({"server":"http://127.0.0.1:1","node_id":"test","node_name":"test","daily_calls":20,"daily_tokens":20000,"providers":{},"profile":{"revision":"1","persona":"test","selection":{"provider":"test","model":"small"},"temperature":null,"max_tokens":null}})).unwrap();
        let bindings = vec![Binding {
            ai: ai.clone(),
            version: "1".into(),
            secret: "test".into(),
        }];
        let windows = direct::Windows::default();
        let request = || LocalRequest {
            owner_id: owner.clone(),
            operation: LocalOperation::PrepareReference {
                conversation_id: cid.clone(),
                selection: selection.clone(),
            },
        };
        assert!(
            operation(&config, &store, &bindings, &windows, request())
                .await
                .is_err()
        );
        direct::cache_reference(&store, &cid, &selection, &reference).unwrap();
        assert!(matches!(
            operation(&config, &store, &bindings, &windows, request())
                .await
                .unwrap(),
            LocalReply::Reference { .. }
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        config.server = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 8192];
            let _ = socket.read(&mut bytes).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                )
                .await
                .unwrap();
        });
        let error = operation(&config, &store, &bindings, &windows, request())
            .await
            .err()
            .unwrap();
        assert!(matches!(
            error.downcast_ref::<NodeError>(),
            Some(NodeError::Api { status: 403, .. })
        ));
        server.await.unwrap();
        assert!(direct::reference(&store, &cid, &selection).is_err());
        config.server = "http://127.0.0.1:1".into();
        assert!(
            operation(&config, &store, &bindings, &windows, request())
                .await
                .is_err()
        );
    }
}
