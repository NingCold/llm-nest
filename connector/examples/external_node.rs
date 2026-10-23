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
            loop {
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
                                configuration_revision: "deterministic-example-v1".into(),
                            },
                        )
                        .await;
                    let receipt = match started {
                        Ok(receipt) => receipt,
                        Err(error) if error.is_queue_wait() => break,
                        Err(error) => return Err(error.into()),
                    };
                    client.result(&task.id, &ResultRequest {
                    request_id: task.id.clone(), run_id: receipt.run_id,
                    message_id: receipt.message_id, confirmation_id: receipt.confirmation_id,
                    status: "completed".into(), text: "[Connector demonstration: fixed response; no model was called.]".into(),
                    error_code: None, usage: None,
                }).await?;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
        _ => return Err("usage: external_node pair | status PAIRING_ID | run".into()),
    }
    Ok(())
}
