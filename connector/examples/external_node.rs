//! Deterministic transport demonstration, not a model-backed Agent.
use connector::node::*;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    let origin = env::var("LLMN_SERVER")?;
    let secret = env::var("LLMN_NODE_SECRET")?;
    let ca = env::var("LLMN_CA_FILE")
        .ok()
        .map(std::fs::read)
        .transpose()?;
    let client = NodeClient::new(&origin, &secret, ca.as_deref())?;
    match command {
        "pair" => {
            let node_id = env::var("LLMN_NODE_ID")?;
            let pairing = client
                .pair(&PairingRequest {
                    node_id,
                    node_name: "Connector demonstration".into(),
                })
                .await?;
            println!(
                "Pairing ID: {}\nAuthorize code in My Account → My managed AI: {}",
                pairing.pairing_id, pairing.user_code
            );
        }
        "status" => println!(
            "{:?}",
            client
                .pairing_status(args.get(2).ok_or("pairing ID required")?)
                .await?
        ),
        "run" => {
            let mut socket = client.connect().await?;
            socket
                .send(&ClientFrame::Hello {
                    protocol_version: 4,
                    node_id: env::var("LLMN_NODE_ID")?,
                    ai_id: env::var("LLMN_AI_ID")?,
                    binding_version: env::var("LLMN_BINDING_VERSION")?,
                    capabilities: vec![],
                })
                .await?;
            match socket.recv().await? {
                Some(ServerFrame::Ready {
                    protocol_version: 4,
                }) => {}
                _ => return Err("node handshake was not accepted".into()),
            }
            loop {
                // Receive revocation/cancellation while waiting for the next polling pass.
                tokio::select! {
                    frame = socket.recv() => match frame? {
                        Some(ServerFrame::ManagementRequest { relay_id, request_id, .. }) => {
                            socket.send(&ClientFrame::ManagementResponse { relay_id, request_id, view: None, error: Some(ConsciousnessError { code: "management_unsupported".into(), message: "This fixed-response example does not manage profiles.".into() }) }).await?;
                        }
                        Some(ServerFrame::ConsciousnessRequest { relay_id, request_id, .. }) => {
                            socket.send(&ClientFrame::ConsciousnessResponse { relay_id, request_id, view: None, error: Some(ConsciousnessError { code: "management_unsupported".into(), message: "This example does not store consciousness materials.".into() }) }).await?;
                        }
                        Some(ServerFrame::Error { code, .. }) => return Err(format!("node authorization ended: {code}").into()),
                        None => return Err("node connection closed".into()),
                        _ => {}
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                }

                for task in client.tasks().await? {
                    // This example deliberately never invokes a model or resumes started work.
                    if task.status != "queued" {
                        continue;
                    }
                    let material = client.materials(&task.id).await?;
                    // This fixed-response demonstration implements no consciousness management.
                    if material
                        .reference
                        .as_ref()
                        .is_some_and(|reference| reference.required)
                    {
                        client.reject(&task.id,&RejectRequest{request_id:task.id.clone(),task_revision:task.revision.clone(),code:"reference_unsupported".into(),message:"The fixed-response demonstration does not handle required topic references.".into()}).await?;
                        continue;
                    }
                    let scope = MaterialScope {
                        conversation_id: task.conversation_id.clone(),
                        revision: task.conversation_id.clone(),
                    };
                    client.announce(&scope).await?;
                    // Stable IDs allow repeated start/result submissions with identical content.
                    let started = client
                        .start(
                            &task.id,
                            &StartRequest {
                                request_id: task.id.clone(),
                                task_revision: task.revision.clone(),
                                conversation_revision: material.conversation.revision,
                                sources: material.sources,
                                derived_sources: vec![],
                                material_scopes: vec![scope],
                                configuration_revision: "deterministic-example-v4".into(),
                            },
                        )
                        .await;
                    let receipt = match started {
                        Ok(receipt) => receipt,
                        Err(error) if error.is_queue_wait() => break,
                        Err(error) => return Err(error.into()),
                    };
                    let demonstration = match &material.task.trigger {
                        NodeTrigger::Message { input_ids, .. } => format!("[Connector demonstration: fixed response to {} required input(s); no model was called.]", input_ids.len()),
                        NodeTrigger::Contact { .. } => "[Connector demonstration: one-shot authorized contact; no model was called.]".into(),
                    };
                    // A pending fixed response is cancellable too. Re-check authoritative
                    // task state after draining control frames before attempting delivery.
                    let cancelled = wait_pending(&mut socket, &task.id).await?;
                    if cancelled || client.task(&task.id).await?.status != "running" {
                        continue;
                    }
                    client
                        .result(
                            &task.id,
                            &ResultRequest {
                                request_id: task.id.clone(),
                                run_id: receipt.run_id,
                                message_id: receipt.message_id,
                                confirmation_id: receipt.confirmation_id,
                                status: "completed".into(),
                                text: demonstration,
                                error_code: None,
                                usage: None,
                            },
                        )
                        .await?;
                }
            }
        }
        _ => return Err("usage: external_node pair | status PAIRING_ID | run".into()),
    }
    Ok(())
}

async fn wait_pending(
    socket: &mut NodeConnection,
    task: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(200);

    loop {
        tokio::select! {
            frame = socket.recv() => match frame? {
                Some(ServerFrame::Cancel { task_id }) if task_id == task => return Ok(true),
                Some(ServerFrame::Error { code, .. }) => return Err(format!("node authorization ended: {code}").into()),
                None => return Err("node connection closed".into()),
                Some(ServerFrame::ManagementRequest { relay_id, request_id, .. }) => {
                    socket.send(&ClientFrame::ManagementResponse { relay_id, request_id, view: None, error: Some(ConsciousnessError { code: "management_unsupported".into(), message: "This example does not manage profiles.".into() }) }).await?;
                }
                Some(ServerFrame::ConsciousnessRequest { relay_id, request_id, .. }) => {
                    socket.send(&ClientFrame::ConsciousnessResponse { relay_id, request_id, view: None, error: Some(ConsciousnessError { code: "management_unsupported".into(), message: "This example does not store consciousness materials.".into() }) }).await?;
                }
                _ => {}
            },
            _ = tokio::time::sleep_until(deadline) => return Ok(false),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    #[tokio::test]
    async fn pending_response_observes_cancel_revoke_and_unsupported_management() {
        for revoke in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                ws.send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::to_string(&ServerFrame::ManagementRequest {
                        relay_id: "relay".into(),
                        request_id: "request".into(),
                        operator_id: "human".into(),
                        operation: AgentManagementOperation::View,
                    })
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
                let response = ws.next().await.unwrap().unwrap().into_text().unwrap();
                assert!(
                    matches!(serde_json::from_str::<ClientFrame>(&response).unwrap(),ClientFrame::ManagementResponse { view:None,error:Some(ConsciousnessError {code,..}),.. } if code=="management_unsupported")
                );
                let frame = if revoke {
                    ServerFrame::Error {
                        code: "node_revoked".into(),
                        message: "revoked".into(),
                    }
                } else {
                    ServerFrame::Cancel {
                        task_id: "task".into(),
                    }
                };
                ws.send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::to_string(&frame).unwrap().into(),
                ))
                .await
                .unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            });
            let client = NodeClient::new(&origin, "private-secret", None).unwrap();
            let mut socket = client.connect().await.unwrap();
            let result = wait_pending(&mut socket, "task").await;
            if revoke {
                assert!(result.unwrap_err().to_string().contains("node_revoked"));
            } else {
                assert!(result.unwrap());
            }
            server.await.unwrap();
        }
    }
}
