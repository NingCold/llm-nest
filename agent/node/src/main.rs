mod consciousness;
mod context;
mod store;
use ai_client::{AiClient, ChatChunk, ModelRouter};
use anyhow::{Context, Result, bail};
use connector::node::*;
use futures_util::StreamExt;
use rand::RngCore;
use rusqlite::OptionalExtension;
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};
use store::{Binding, Config, Store};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn client(config: &Config, secret: &str) -> Result<NodeClient> {
    let ca = config.ca_file.as_ref().map(fs::read).transpose()?;
    Ok(NodeClient::new(&config.server, secret, ca.as_deref())?)
}
fn private_directory(path: &Path) -> Result<()> {
    anyhow::ensure!(!path.exists(), "init requires a new node directory");
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        let identity = std::process::Command::new("whoami").output()?;
        anyhow::ensure!(identity.status.success(), "cannot determine local user");
        let user = String::from_utf8(identity.stdout)?.trim().to_string();
        let status = std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)F")])
            .output()?;
        anyhow::ensure!(
            status.status.success(),
            "cannot restrict node directory permissions"
        );
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    let directory = PathBuf::from(args.get(2).map(String::as_str).unwrap_or(".llmn-agent"));
    if command == "init" {
        private_directory(&directory)?;
        let config = format!(
            r#"server = "https://localhost:8443"
node_id = "{}"
node_name = "Local Agent"
daily_calls = 48
daily_tokens = 1048576
[profile]
revision = "1"
persona = "你是一位友善、诚实的 AI 联系人。"
[profile.selection]
provider = "chatecnu"
model = "ecnu-max"
[providers.chatecnu]
api_key = {{ env = "API_KEY_CHATECNU" }}
"#,
            id()
        );
        use std::io::Write;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("node.toml"))?;
        file.write_all(config.as_bytes())?;
        Store::open(&directory.join("node.sqlite"))?;
        println!(
            "Created node directory. Set server and optional ca_file in node.toml, then pair."
        );
        return Ok(());
    }
    if command != "pair" && command != "run" && command != "profile" {
        println!(
            "llmn-agent init|pair|run [directory]; profile <directory> <AI id> <profile.json>"
        );
        return Ok(());
    }
    let config: Config = toml::from_str(&fs::read_to_string(directory.join("node.toml"))?)?;
    anyhow::ensure!(
        config
            .providers
            .values()
            .all(|provider| matches!(provider.api_key, ai_client::ApiKey::FromEnv { .. })),
        "node provider keys must use environment references"
    );
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("node.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("node directory already in use")?;
    let store = Store::open(&directory.join("node.sqlite"))?;
    if command == "profile" {
        let ai = args.get(3).context("profile requires AI id")?;
        let path = args.get(4).context("profile requires profile JSON file")?;
        let profile: store::Profile = serde_json::from_str(&fs::read_to_string(path)?)?;
        anyhow::ensure!(
            profile.revision != store.profile(ai)?.revision,
            "profile revision must change"
        );
        let resolved = ModelRouter::new(&config.providers)?.resolve(&profile.selection)?;
        builtin_agent::generation::options(
            &builtin_agent::generation::Parameters {
                temperature: profile.temperature,
                max_tokens: profile.max_tokens,
            },
            &resolved,
        )?;
        let changed = store.db.execute(
            "UPDATE profiles SET json=?2 WHERE ai=?1",
            rusqlite::params![ai, serde_json::to_string(&profile)?],
        )?;
        anyhow::ensure!(changed == 1, "AI is not paired in this node directory");
        println!("AI profile saved. New tasks use the saved profile.");
        return Ok(());
    }
    if command == "pair" {
        return pair(&config, &store).await;
    }
    let bindings = store.bindings()?;
    if bindings.is_empty() {
        bail!("no AI binding; run pair first");
    }
    let model_slot = tokio::sync::Mutex::new(());
    let nodes = bindings.iter().map(|binding| async {
        loop {
            if let Err(error) = cycle(&config, &store, binding, &model_slot).await {
                eprintln!("Node cycle failed: {error}");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    tokio::select! { _=tokio::signal::ctrl_c()=>Ok(()), _=futures_util::future::join_all(nodes)=>Ok(()) }
}
fn result(receipt: &StartReceipt, status: &str, text: String, code: Option<&str>) -> ResultRequest {
    ResultRequest {
        request_id: id(),
        run_id: receipt.run_id.clone(),
        message_id: receipt.message_id.clone(),
        confirmation_id: receipt.confirmation_id.clone(),
        status: status.into(),
        text,
        error_code: code.map(str::to_string),
        usage: None,
    }
}
async fn recover(api: &NodeClient, store: &Store, binding: &Binding) -> Result<()> {
    for store::PendingTask {
        id: task,
        start,
        receipt,
        result: saved,
        draft,
    } in store.pending(&binding.ai)?
    {
        let cloud = match api.task(&task).await {
            Ok(cloud) => cloud,
            Err(NodeError::Api {
                status: 401 | 403 | 404 | 410,
                ..
            }) => {
                store.acknowledged(&task)?;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if matches!(
            cloud.status.as_str(),
            "completed" | "cancelled" | "expired" | "failed" | "interrupted"
        ) {
            store.acknowledged(&task)?;
            continue;
        }
        if let Some(saved) = saved {
            let saved: ResultRequest = serde_json::from_str(&saved)?;
            deliver(api, store, &task, &saved).await?;
            continue;
        }
        if cloud.status == "queued" {
            let start: StartRequest = serde_json::from_str(&start)?;
            api.reject(&task,&RejectRequest {request_id:start.request_id,task_revision:start.task_revision,code:"node_restart_before_start".into(),message:"Node restarted before the model invocation began; resend manually if needed.".into()}).await?;
            store.acknowledged(&task)?;
            continue;
        }
        // An intent may already have reached the cloud. Reuse its exact start command;
        // reconciliation NEVER opens a provider stream.
        if !matches!(cloud.status.as_str(), "queued" | "running") {
            store.acknowledged(&task)?;
            continue;
        }
        let receipt: StartReceipt = match receipt {
            Some(json) => serde_json::from_str(&json)?,
            None => {
                api.start(&task, &serde_json::from_str::<StartRequest>(&start)?)
                    .await?
            }
        };
        let saved = result(&receipt, "uncertain", draft, Some("node_restart_uncertain"));
        store.final_result(&task, &saved)?;
        deliver(api, store, &task, &saved).await?;
    }
    Ok(())
}
async fn deliver(api: &NodeClient, store: &Store, task: &str, saved: &ResultRequest) -> Result<()> {
    match api.result(task, saved).await {
        Ok(_) => store.acknowledged(task)?,
        Err(NodeError::Api { status, .. })
            if (400..500).contains(&status) && status != 408 && status != 429 =>
        {
            store.db.execute(
                "UPDATE tasks SET state='delivery_rejected' WHERE id=?1",
                [task],
            )?;
            eprintln!(
                "Task {task}: saved result rejected by cloud (HTTP {status}); retained in node.sqlite tasks.result_json for diagnosis, without retry."
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
fn generation_failure(error: &anyhow::Error) -> (&'static str, &'static str) {
    use ai_client::AiError;
    if error.downcast_ref::<NodeError>().is_some() {
        return ("interrupted", "node_connection_failed");
    }
    if error.downcast_ref::<rusqlite::Error>().is_some() {
        return ("failed", "node_storage_failed");
    }
    if let Some(error) = error.downcast_ref::<AiError>() {
        let code = match error {
            AiError::ApiError { status, .. } if matches!(status.as_u16(), 401 | 403) => {
                "provider_authentication_failed"
            }
            AiError::ApiError { status, .. } if status.as_u16() == 429 => "provider_rate_limited",
            AiError::ApiError { .. } => "provider_api_failed",
            AiError::Reqwest(error) if error.is_timeout() => "provider_timeout",
            AiError::Reqwest(_) => "provider_connection_failed",
            AiError::Json(_)
            | AiError::InvalidResponse(_)
            | AiError::StreamError(_)
            | AiError::UnexpectedEof => "provider_protocol_failed",
            AiError::ImageInputUnsupported => "image_input_unsupported",
            _ => "provider_configuration_failed",
        };
        return ("failed", code);
    }
    let code = match error.to_string().as_str() {
        "source_cancelled" => "source_cancelled",
        "node_authorization_lost" => "node_authorization_lost",
        "node_disconnected" => "node_disconnected",
        "output_limit" => "output_limit",
        "tools_not_enabled" => "tools_not_enabled",
        "provider_stream_incomplete" => "provider_protocol_failed",
        _ => "node_execution_failed",
    };
    (
        if matches!(
            code,
            "source_cancelled" | "node_authorization_lost" | "node_disconnected"
        ) {
            "interrupted"
        } else {
            "failed"
        },
        code,
    )
}
fn generation_outcome(
    outcome: std::result::Result<Result<()>, tokio::time::error::Elapsed>,
    text: &str,
) -> (&'static str, Option<&'static str>) {
    match outcome {
        Ok(Ok(())) if text.trim().is_empty() => ("failed", Some("empty_response")),
        Ok(Ok(())) => ("completed", None),
        Ok(Err(error)) => {
            let (status, code) = generation_failure(&error);
            (status, Some(code))
        }
        Err(_) => ("interrupted", Some("generation_timeout")),
    }
}
// The wait must retain the socket, node dependencies, and frozen run scope together.
#[allow(clippy::too_many_arguments)]
async fn while_connected_with<T>(
    socket: &mut NodeConnection,
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
    scopes: &[MaterialScope],
    task: &str,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::pin!(work);
    let mut invalidations = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            biased;
            frame=socket.recv()=>{let frame=consciousness::handle_frame(frame?,socket,api,store,config,binding,scopes).await?;match frame {Some(ServerFrame::Cancel {task_id}) if task_id==task=>bail!("source_cancelled"),Some(ServerFrame::Error {..})=>bail!("node_authorization_lost"),None=>bail!("node_disconnected"),_=>{}}},
            _=invalidations.tick()=>{consciousness::poll_state(api,store,binding,scopes).await?;},
            result=&mut work=>return result,
        }
    }
}
async fn cycle(
    config: &Config,
    store: &Store,
    binding: &Binding,
    model_slot: &tokio::sync::Mutex<()>,
) -> Result<()> {
    let api = client(config, &binding.secret)?;
    store.db.execute(
        "UPDATE conscious_scopes SET announced='' WHERE ai=?1",
        [&binding.ai],
    )?;
    consciousness::sync(&api, store, &binding.ai).await?;
    consciousness::recover_summaries(&api, store, &binding.ai).await?;
    recover(&api, store, binding).await?;
    let mut socket = api.connect().await?;
    socket
        .send(&ClientFrame::Hello {
            protocol_version: 3,
            node_id: config.node_id.clone(),
            ai_id: binding.ai.clone(),
            binding_version: binding.version.clone(),
        })
        .await?;
    match socket.recv().await? {
        Some(ServerFrame::Ready {
            protocol_version: 3,
        }) => {}
        _ => bail!("node handshake failed"),
    }
    loop {
        consciousness::sync(&api, store, &binding.ai).await?;
        consciousness::summaries_tick(&api, store, config, binding, &mut socket, model_slot)
            .await?;
        let tasks =
            while_connected_with(&mut socket, &api, store, config, binding, &[], "", async {
                Ok(api.tasks().await?)
            })
            .await?;
        for task in tasks {
            if task.status != "queued" || store.state(&task.id)?.is_some() {
                continue;
            }
            let Ok(_model_slot) = model_slot.try_lock() else {
                break;
            };
            let profile = store.profile(&binding.ai)?;
            let prepared = async {
                let router = ModelRouter::new(&config.providers)?;
                let llm = AiClient::from_config(&config.providers)?.snapshot().await;
                let material = api.materials(&task.id).await?;
                let sources = material.sources.clone();
                let conversation_revision = material.conversation.revision.clone();
                let (request, tokens, derived_sources, material_scopes) =
                    context::prepare_scoped(&api, &router, &profile, material, Some(store)).await?;
                Ok::<_, anyhow::Error>((
                    llm,
                    request,
                    tokens,
                    sources,
                    conversation_revision,
                    derived_sources,
                    material_scopes,
                ))
            };
            let prepared = while_connected_with(
                &mut socket,
                &api,
                store,
                config,
                binding,
                &[],
                &task.id,
                prepared,
            )
            .await;
            let (
                llm,
                request,
                tokens,
                sources,
                conversation_revision,
                derived_sources,
                material_scopes,
            ) = match prepared {
                Ok(value) => value,
                Err(error) => {
                    let message = error.to_string();
                    api.reject(
                        &task.id,
                        &RejectRequest {
                            request_id: id(),
                            task_revision: task.revision,
                            code: if message == "image_input_unsupported" {
                                "image_input_unsupported"
                            } else {
                                "preparation_failed"
                            }
                            .into(),
                            message,
                        },
                    )
                    .await?;
                    continue;
                }
            };
            let start = StartRequest {
                request_id: id(),
                task_revision: task.revision.clone(),
                conversation_revision,
                derived_sources,
                material_scopes: material_scopes.clone(),
                sources,
                configuration_revision: profile.revision,
            };
            for scope in &material_scopes {
                api.announce(scope).await?;
            }
            anyhow::ensure!(
                store.scopes_current(&binding.ai, &material_scopes)?,
                "source_cancelled"
            );
            let budget_day = chrono::Utc::now().format("%Y-%m-%d").to_string();
            if let Err(error) = store.reserve(
                &task.id,
                &binding.ai,
                &start,
                &budget_day,
                tokens,
                store::BudgetLimits {
                    calls: config.daily_calls,
                    tokens: config.daily_tokens,
                },
            ) {
                if error.downcast_ref::<store::BudgetDenied>().is_none() {
                    return Err(error);
                }
                api.reject(
                    &task.id,
                    &RejectRequest {
                        request_id: id(),
                        task_revision: task.revision,
                        code: "node_budget_exhausted".into(),
                        message: error.to_string(),
                    },
                )
                .await?;
                continue;
            }
            let started = while_connected_with(
                &mut socket,
                &api,
                store,
                config,
                binding,
                &material_scopes,
                &task.id,
                async { Ok(api.start(&task.id, &start).await) },
            )
            .await?;
            let receipt = match started {
                Ok(receipt) => receipt,
                Err(error) if error.is_queue_wait() => {
                    store.defer_unstarted(&task.id, &budget_day, tokens)?;
                    break;
                }
                Err(error) => return Err(error.into()),
            };
            store.started(&task.id, &receipt)?;
            let mut text = String::new();
            let mut usage = None;
            let mut sequence = 0;
            let mut last = tokio::time::Instant::now();
            let work = async {
                let mut invalidations = tokio::time::interval(Duration::from_secs(1));
                let mut stream = while_connected_with(
                    &mut socket,
                    &api,
                    store,
                    config,
                    binding,
                    &material_scopes,
                    &task.id,
                    async { Ok(llm.complete_stream(request).await?) },
                )
                .await?;
                loop {
                    tokio::select! {
                        frame=socket.recv()=>{let frame=consciousness::handle_frame(frame?,&mut socket,&api,store,config,binding,&material_scopes).await?;match frame {Some(ServerFrame::Cancel {task_id}) if task_id==task.id=>bail!("source_cancelled"),Some(ServerFrame::Error {..})=>bail!("node_authorization_lost"),None=>bail!("node_disconnected"),_=>{}}},
                        _=invalidations.tick()=>{consciousness::poll_state(&api,store,binding,&material_scopes).await?;},
                        chunk=stream.next()=>match chunk {
                            Some(Ok(ChatChunk::Delta {content}))=>{
                                if text.len()+content.len()>256*1024 {bail!("output_limit");} text.push_str(&content);
                                if last.elapsed()>=Duration::from_millis(500) {store.draft(&task.id,&text)?;sequence+=1;socket.send(&ClientFrame::Delta {task_id:task.id.clone(),run_id:receipt.run_id.clone(),message_id:receipt.message_id.clone(),confirmation_id:receipt.confirmation_id.clone(),sequence,text:text.clone()}).await?;last=tokio::time::Instant::now();}
                            },
                            Some(Ok(ChatChunk::Done {usage:reported}))=>{usage=reported;return Ok::<_,anyhow::Error>(());},
                            Some(Ok(ChatChunk::ToolCall {..}))=>bail!("tools_not_enabled"),
                            Some(Ok(ChatChunk::ReasoningDelta {..}))=>{},Some(Err(error))=>return Err(error.into()),None=>bail!("provider_stream_incomplete")
                        }
                    }
                }
            };
            let outcome = tokio::time::timeout(Duration::from_secs(120), work).await;
            let (status, code) = generation_outcome(outcome, &text);
            if let Some(code) = code {
                eprintln!("Generation ended: {code}");
            }
            let (status, code) = if !store.scopes_current(&binding.ai, &material_scopes)? {
                ("interrupted", Some("source_cancelled"))
            } else {
                (status, code)
            };
            let mut saved = result(&receipt, status, text, code);
            saved.usage = usage.map(|u| NodeUsage {
                input_tokens: u.prompt_tokens as u64,
                output_tokens: u.completion_tokens as u64,
                cached_tokens: u.cached_tokens as u64,
            });
            store.final_result(&task.id, &saved)?;
            deliver(&api, store, &task.id, &saved).await?;
        }
        tokio::select! {
            frame=socket.recv()=>{let frame=consciousness::handle_frame(frame?,&mut socket,&api,store,config,binding,&[]).await?;match frame {Some(ServerFrame::Error {..})|None=>bail!("node connection closed"),_=>{}}},
            _=tokio::time::sleep(Duration::from_secs(2))=>{}
        }
    }
}
#[cfg(test)]
#[test]
fn only_explicit_unstarted_queue_conflicts_allow_deferral() {
    for code in ["conversation_busy", "task_not_ready"] {
        assert!(
            NodeError::Api {
                status: 409,
                body: serde_json::json!({"code":code}).to_string()
            }
            .is_queue_wait()
        );
    }
    for (status, body) in [
        (409, "{}"),
        (409, r#"{"code":"source_changed"}"#),
        (500, r#"{"code":"conversation_busy"}"#),
    ] {
        assert!(
            !NodeError::Api {
                status,
                body: body.into()
            }
            .is_queue_wait()
        );
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingPair {
    pairing: PairingCreated,
    secret: String,
}
async fn pair(config: &Config, store: &Store) -> Result<()> {
    let pending: Option<String> = store
        .db
        .query_row("SELECT value FROM meta WHERE key='pending_pair'", [], |r| {
            r.get(0)
        })
        .optional()?;
    let mut pending = pending
        .map(|json| serde_json::from_str::<PendingPair>(&json))
        .transpose()?;
    loop {
        if pending.is_none() {
            let mut bytes = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            let pairing = client(config, &secret)?
                .pair(&PairingRequest {
                    node_id: config.node_id.clone(),
                    node_name: config.node_name.clone(),
                })
                .await?;
            let fresh = PendingPair { pairing, secret };
            store.db.execute("INSERT INTO meta VALUES('pending_pair',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&fresh)?])?;
            pending = Some(fresh);
        }
        let current = pending.as_ref().expect("pairing established");
        println!(
            "Authorize pairing code {} in My account / My AI accounts (expires {}).",
            current.pairing.user_code, current.pairing.expires_at
        );
        let api = client(config, &current.secret)?;
        loop {
            match api.pairing_status(&current.pairing.pairing_id).await {
                Ok(status) if status.state == "authorized" => {
                    store.bind(
                        &Binding {
                            ai: status.ai_id.context("authorized pairing missing AI")?,
                            version: status
                                .binding_version
                                .context("authorized pairing missing version")?,
                            secret: current.secret.clone(),
                        },
                        &config.profile,
                    )?;
                    store
                        .db
                        .execute("DELETE FROM meta WHERE key='pending_pair'", [])?;
                    println!("AI binding saved.");
                    return Ok(());
                }
                Ok(status) if status.state == "pending" => {}
                Ok(_) => bail!("unknown pairing state"),
                Err(NodeError::Api {
                    status: 404 | 410, ..
                }) => {
                    store
                        .db
                        .execute("DELETE FROM meta WHERE key='pending_pair'", [])?;
                    pending = None;
                    break;
                }
                Err(error) => return Err(error.into()),
            }
            if current
                .pairing
                .expires_at
                .parse::<chrono::DateTime<chrono::Utc>>()?
                <= chrono::Utc::now()
            {
                store
                    .db
                    .execute("DELETE FROM meta WHERE key='pending_pair'", [])?;
                pending = None;
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn mock(
        replies: Vec<(&'static str, String)>,
    ) -> (NodeClient, tokio::task::JoinHandle<()>) {
        mock_status(
            replies
                .into_iter()
                .map(|(path, body)| (path, body, 200))
                .collect(),
        )
        .await
    }
    async fn mock_status(
        replies: Vec<(&'static str, String, u16)>,
    ) -> (NodeClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = NodeClient::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            "test",
            None,
        )
        .unwrap();
        let worker = tokio::spawn(async move {
            for (path, body, status) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut buffer = [0u8; 2048];
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                assert!(headers.lines().next().unwrap().contains(path));
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while request.len() < header_end + length {
                    let mut buffer = [0u8; 2048];
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                }
                if path.ends_with("/result") {
                    let value: ResultRequest =
                        serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
                    assert!(value.status == "uncertain" || value.status == "completed");
                }
                socket.write_all(format!("HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
            }
        });
        (api, worker)
    }
    fn task(status: &str) -> NodeTask {
        NodeTask {
            id: "task".into(),
            ai_id: "ai".into(),
            conversation_id: "conversation".into(),
            binding_version: "1".into(),
            revision: "1".into(),
            trigger_message_id: "trigger".into(),
            status: status.into(),
            expires_at: "2099-01-01T00:00:00Z".into(),
        }
    }
    #[tokio::test]
    async fn restart_intent_reconciles_without_model_and_final_retries_same_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.db");
        let start = StartRequest {
            request_id: "original-start".into(),
            task_revision: "1".into(),
            conversation_revision: "1".into(),
            derived_sources: vec![],
            material_scopes: vec![],
            sources: vec![],
            configuration_revision: "1".into(),
        };
        {
            Store::open(&path)
                .unwrap()
                .reserve(
                    "task",
                    "ai",
                    &start,
                    "2026-10-22",
                    100,
                    store::BudgetLimits {
                        calls: 3,
                        tokens: 1000,
                    },
                )
                .unwrap();
        }
        let receipt = StartReceipt {
            task_id: "task".into(),
            run_id: "run".into(),
            message_id: "message".into(),
            confirmation_id: "confirmation".into(),
            binding_version: "1".into(),
        };
        let (api, worker) = mock(vec![
            (
                "GET /api/node/tasks/task",
                serde_json::to_string(&task("running")).unwrap(),
            ),
            (
                "POST /api/node/tasks/task/start",
                serde_json::to_string(&receipt).unwrap(),
            ),
            (
                "POST /api/node/tasks/task/result",
                serde_json::to_string(&task("uncertain")).unwrap(),
            ),
        ])
        .await;
        let store = Store::open(&path).unwrap();
        let binding = Binding {
            ai: "ai".into(),
            version: "1".into(),
            secret: "test".into(),
        };
        recover(&api, &store, &binding).await.unwrap();
        worker.await.unwrap();
        assert_eq!(
            store.state("task").unwrap().as_deref(),
            Some("acknowledged")
        );
        // Replaying a persisted final never invokes start again.
        let saved = result(&receipt, "completed", "saved answer".into(), None);
        store.final_result("task", &saved).unwrap();
        let (api, worker) = mock(vec![
            (
                "GET /api/node/tasks/task",
                serde_json::to_string(&task("uncertain")).unwrap(),
            ),
            (
                "POST /api/node/tasks/task/result",
                serde_json::to_string(&task("completed")).unwrap(),
            ),
        ])
        .await;
        recover(&api, &store, &binding).await.unwrap();
        worker.await.unwrap();
        store.final_result("task", &saved).unwrap();
        let (api, worker) = mock(vec![(
            "GET /api/node/tasks/task",
            serde_json::to_string(&task("completed")).unwrap(),
        )])
        .await;
        recover(&api, &store, &binding).await.unwrap();
        worker.await.unwrap();
        store.final_result("task", &saved).unwrap();
        let (api, worker) = mock(vec![(
            "GET /api/node/tasks/task",
            serde_json::to_string(&task("cancelled")).unwrap(),
        )])
        .await;
        recover(&api, &store, &binding).await.unwrap();
        worker.await.unwrap();
        assert_eq!(
            store.state("task").unwrap().as_deref(),
            Some("acknowledged")
        );
        store.final_result("task", &saved).unwrap();
        let (api, worker) = mock_status(vec![
            (
                "GET /api/node/tasks/task",
                serde_json::to_string(&task("running")).unwrap(),
                200,
            ),
            (
                "POST /api/node/tasks/task/result",
                "invalid result".into(),
                409,
            ),
        ])
        .await;
        recover(&api, &store, &binding).await.unwrap();
        worker.await.unwrap();
        assert_eq!(
            store.state("task").unwrap().as_deref(),
            Some("delivery_rejected")
        );
        assert!(store.pending("ai").unwrap().is_empty());
        store.db.execute("UPDATE tasks SET state='intent',result_json=NULL,receipt_json=NULL WHERE id='task'",[]).unwrap();
        let (api, worker) = mock(vec![
            (
                "GET /api/node/tasks/task",
                serde_json::to_string(&task("queued")).unwrap(),
            ),
            (
                "POST /api/node/tasks/task/reject",
                serde_json::to_string(&task("failed")).unwrap(),
            ),
        ])
        .await;
        recover(&api, &store, &binding).await.unwrap();
        worker.await.unwrap();
        assert_eq!(
            store.state("task").unwrap().as_deref(),
            Some("acknowledged")
        );
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    #[test]
    fn empty_completed_response_is_failed_and_provider_errors_are_sanitized() {
        assert_eq!(
            generation_outcome(Ok(Ok(())), "  \n"),
            ("failed", Some("empty_response"))
        );
        assert_eq!(
            generation_outcome(Ok(Ok(())), "answer"),
            ("completed", None)
        );
        let invalid = anyhow::Error::from(ai_client::AiError::InvalidResponse(
            "private provider response must never escape".into(),
        ));
        assert_eq!(
            generation_failure(&invalid),
            ("failed", "provider_protocol_failed")
        );
        let authentication = anyhow::Error::from(ai_client::AiError::ApiError {
            status: 401.try_into().unwrap(),
            message: "private credential".into(),
            kind: None,
            code: None,
        });
        assert_eq!(
            generation_failure(&authentication),
            ("failed", "provider_authentication_failed")
        );
        assert_eq!(
            generation_failure(&anyhow::anyhow!("source_cancelled")),
            ("interrupted", "source_cancelled")
        );
    }
}
