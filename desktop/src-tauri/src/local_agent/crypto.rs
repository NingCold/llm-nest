use super::*;
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoStatus {
    device_id: Option<String>,
    fingerprint: Option<String>,
    curve: Option<String>,
    master: Option<String>,
    identity_ready: bool,
    recovery_saved: bool,
    peer_trusted: bool,
    ready: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedDevice {
    device_id: String,
    fingerprint: String,
    curve: String,
    master: String,
}
pub(super) fn ensure_session(
    cloud: &CloudState,
    owner: &Owner,
    generation: u64,
) -> Result<(), String> {
    let (server, human, current) = cloud.authorized_local_session()?;
    if server != owner.server || human != owner.human || current != generation {
        return Err("登录身份已变化，请重新操作".into());
    }
    Ok(())
}
pub(super) fn ensure_stopped(state: &LocalAgentState, p: &Path) -> Result<(), String> {
    if running(p)? || state.pairs.lock().unwrap().get(p).is_some_and(|p| p.active) {
        return Err("请先停止本机 Agent 并等待配对结束".into());
    }
    Ok(())
}
async fn context(
    app: &tauri::AppHandle,
    cloud: &CloudState,
    state: &LocalAgentState,
    ai: &str,
) -> Result<(Owner, PathBuf, u64), String> {
    Uuid::parse_str(ai).map_err(|_| "AI 账号 ID 无效")?;
    let (server, human, generation) = cloud.authorized_local_session()?;
    let owner = Owner { server, human };
    let p = state.dir(&owner);
    ensure_stopped(state, &p)?;
    let m = manifest(&p)?.ok_or("请先配置并绑定本机 Agent")?;
    let v = live(app, &p).await?;
    if !v["aiIds"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|v| v == ai))
    {
        return Err("此本机 Agent 尚未绑定指定 AI".into());
    }
    let node = m.config["node_id"].as_str().ok_or("本机节点配置无效")?;
    cloud.verify_local_ai(ai, node, generation).await?;
    ensure_session(cloud, &owner, generation)?;
    Ok((owner, p, generation))
}
async fn invoke(
    app: &tauri::AppHandle,
    cloud: &CloudState,
    owner: &Owner,
    generation: u64,
    p: &Path,
    ai: &str,
    operation: (&str, &[String]),
) -> Result<Vec<u8>, String> {
    let (verb, args) = operation;
    ensure_session(cloud, owner, generation)?;
    let mut c = command(app, verb, p, None)?;
    c.arg(ai).args(args);
    run_guarded(c, verb, || ensure_session(cloud, owner, generation)).await
}
async fn run_guarded(
    mut command: tokio::process::Command,
    verb: &str,
    current: impl Fn() -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    current()?;
    let mut child = command.spawn().map_err(io_error)?;
    let stdout = child.stdout.take().ok_or("无法读取本机 Agent 响应")?;
    let read = async {
        use tokio::io::AsyncReadExt;
        let mut bytes = Vec::new();
        stdout
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .await
            .map_err(io_error)?;
        Ok::<_, String>(bytes)
    };
    let work = async {
        let (result, bytes) =
            tokio::try_join!(async { child.wait().await.map_err(io_error) }, read)?;
        if !result.success() {
            return Err(match verb {
                "crypto-init" => "创建身份失败；若 AI 已有加密身份，请导入恢复文件，勿重新建立身份",
                "crypto-recover" => "恢复失败，请确认恢复文件属于此 AI 且内容完整；原身份未被重建",
                "crypto-trust" => "设备核验失败，请重新核对双方指纹及当前管理权",
                _ => "加密操作失败，请检查本机绑定、网络及加密身份后重试",
            }
            .into());
        }
        if bytes.len() > 1_048_576 {
            return Err("本机加密响应过大".into());
        }
        Ok(bytes)
    };
    tokio::pin!(work);
    let deadline = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(deadline);
    loop {
        tokio::select! { result = &mut work => { current()?; return result; }, _ = &mut deadline => return Err("本机加密操作超时".into()), _ = tokio::time::sleep(Duration::from_millis(50)) => { current()?; } }
    }
}
async fn read_status(
    app: &tauri::AppHandle,
    cloud: &CloudState,
    owner: &Owner,
    generation: u64,
    p: &Path,
    ai: &str,
    device: &str,
) -> Result<CryptoStatus, String> {
    let bytes = invoke(
        app,
        cloud,
        owner,
        generation,
        p,
        ai,
        ("crypto-status", &[owner.human.to_string(), device.into()]),
    )
    .await?;
    parse_status(&bytes)
}
fn parse_status(bytes: &[u8]) -> Result<CryptoStatus, String> {
    let invalid = || "本机 Agent 加密状态不兼容，请更新随附 Agent 后重试".to_owned();
    let v: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let string = |key: &str| -> Result<Option<String>, String> {
        match v.get(key) {
            Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            _ => Err(invalid()),
        }
    };
    let boolean = |key: &str| v[key].as_bool().ok_or_else(invalid);
    let master = match v.get("master") {
        Some(Value::Null) => None,
        Some(value) => Some(
            value["keys"]
                .as_object()
                .and_then(|keys| keys.values().find_map(Value::as_str))
                .ok_or_else(invalid)?
                .to_owned(),
        ),
        None => return Err(invalid()),
    };
    let status = CryptoStatus {
        device_id: string("deviceId")?,
        fingerprint: string("ed25519")?,
        curve: string("curve25519")?,
        master,
        identity_ready: boolean("identityReady")?,
        recovery_saved: boolean("recoverySaved")?,
        peer_trusted: boolean("peerTrusted")?,
        ready: boolean("ready")?,
    };
    let valid_device = if status.device_id.is_some() {
        status.fingerprint.is_some() && status.curve.is_some()
    } else {
        status.fingerprint.is_none() && status.curve.is_none() && status.master.is_none()
    };
    if !valid_device
        || (status.identity_ready && (status.device_id.is_none() || status.master.is_none()))
    {
        return Err(invalid());
    }
    Ok(status)
}

#[tauri::command]
pub async fn local_agent_crypto_status(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
    ai_id: String,
    device_id: String,
) -> Result<CryptoStatus, String> {
    let _op = state.operation.lock().await;
    let (owner, p, generation) = context(&app, &cloud, &state, &ai_id).await?;
    read_status(&app, &cloud, &owner, generation, &p, &ai_id, &device_id).await
}
#[tauri::command]
pub async fn local_agent_crypto_initialize(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
    ai_id: String,
    device_id: String,
) -> Result<CryptoStatus, String> {
    let _op = state.operation.lock().await;
    let (owner, p, generation) = context(&app, &cloud, &state, &ai_id).await?;
    invoke(
        &app,
        &cloud,
        &owner,
        generation,
        &p,
        &ai_id,
        ("crypto-init", &[]),
    )
    .await?;
    read_status(&app, &cloud, &owner, generation, &p, &ai_id, &device_id).await
}
pub(super) struct ExportChallenge {
    owner: Owner,
    generation: u64,
    device: String,
    ai: String,
    path: String,
    nonce: Uuid,
    issued: std::time::Instant,
}
impl ExportChallenge {
    fn statement(&self) -> String {
        format!(
            "llmnest.local-ai.recovery-export.v1\n{}",
            json!([
                self.owner.server,
                self.owner.human,
                self.generation.to_string(),
                self.ai,
                self.device,
                self.nonce
            ])
        )
    }
    fn validate(
        &self,
        owner: &Owner,
        generation: u64,
        device: &str,
        ai: &str,
        fingerprint: &str,
        signature: &str,
    ) -> Result<(), String> {
        use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
        use ed25519_dalek::{Signature, VerifyingKey};
        let invalid = || "导出授权无效或已过期，请重新选择保存位置".to_owned();
        if &self.owner != owner
            || self.generation != generation
            || self.device != device
            || self.ai != ai
            || self.issued.elapsed() >= Duration::from_secs(90)
        {
            return Err(invalid());
        }
        let key: [u8; 32] = STANDARD_NO_PAD
            .decode(fingerprint)
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or_else(invalid)?;
        let bytes = STANDARD_NO_PAD.decode(signature).map_err(|_| invalid())?;
        let signature = Signature::try_from(bytes.as_slice()).map_err(|_| invalid())?;
        VerifyingKey::from_bytes(&key)
            .and_then(|k| k.verify_strict(self.statement().as_bytes(), &signature))
            .map_err(|_| invalid())
    }
}
async fn recovery_path(
    window: &tauri::WebviewWindow,
    export: bool,
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let dialog = window
        .dialog()
        .file()
        .set_parent(window)
        .add_filter("AI 恢复文件", &["json"]);
    if export {
        dialog
            .set_title("保存 AI 私有恢复文件")
            .set_file_name("ai-recovery.json")
            .save_file(move |p| {
                let _ = tx.send(p);
            });
    } else {
        dialog.set_title("恢复 AI 加密身份").pick_file(move |p| {
            let _ = tx.send(p);
        });
    }
    let Some(path) = rx.await.map_err(|_| "恢复文件窗口已关闭")? else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|_| "恢复文件位置无效")?;
    if export && path.exists() {
        return Err("目标文件已存在，请选择新的文件名；不会覆盖恢复材料".into());
    }
    Ok(Some(
        path.to_str().ok_or("恢复文件路径无法编码")?.to_owned(),
    ))
}
#[tauri::command]
pub async fn local_agent_crypto_export_challenge(
    window: tauri::WebviewWindow,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
    ai_id: String,
    device_id: String,
) -> Result<Option<String>, String> {
    let _op = state.operation.lock().await;
    state.export.lock().unwrap().take();
    let (owner, _, generation) = context(window.app_handle(), &cloud, &state, &ai_id).await?;
    cloud
        .local_export_key(owner.human, &device_id, generation)
        .await?;
    let path = recovery_path(&window, true).await?;
    ensure_session(&cloud, &owner, generation)?;
    let Some(path) = path else {
        return Ok(None);
    };
    let challenge = ExportChallenge {
        owner,
        generation,
        device: device_id,
        ai: ai_id,
        path,
        nonce: Uuid::new_v4(),
        issued: std::time::Instant::now(),
    };
    let statement = challenge.statement();
    *state.export.lock().unwrap() = Some(challenge);
    Ok(Some(statement))
}
#[tauri::command]
pub async fn local_agent_crypto_export(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
    ai_id: String,
    device_id: String,
    signature: String,
) -> Result<bool, String> {
    let _op = state.operation.lock().await;
    let challenge = state
        .export
        .lock()
        .unwrap()
        .take()
        .ok_or("请重新选择恢复文件保存位置")?;
    let (owner, p, generation) = context(&app, &cloud, &state, &ai_id).await?;
    let fingerprint = cloud
        .local_export_key(owner.human, &device_id, generation)
        .await?;
    challenge.validate(
        &owner,
        generation,
        &device_id,
        &ai_id,
        &fingerprint,
        &signature,
    )?;
    invoke(
        &app,
        &cloud,
        &owner,
        generation,
        &p,
        &ai_id,
        ("crypto-export-recovery", &[challenge.path]),
    )
    .await?;
    Ok(true)
}
#[tauri::command]
pub async fn local_agent_crypto_recover(
    window: tauri::WebviewWindow,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
    ai_id: String,
) -> Result<bool, String> {
    let _op = state.operation.lock().await;
    let app = window.app_handle();
    let (owner, p, generation) = context(app, &cloud, &state, &ai_id).await?;
    let path = recovery_path(&window, false).await?;
    ensure_session(&cloud, &owner, generation)?;
    let Some(path) = path else {
        return Ok(false);
    };
    context(app, &cloud, &state, &ai_id).await?;
    invoke(
        app,
        &cloud,
        &owner,
        generation,
        &p,
        &ai_id,
        ("crypto-recover", &[path]),
    )
    .await?;
    Ok(true)
}
#[tauri::command]
pub async fn local_agent_crypto_trust(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
    ai_id: String,
    device: TrustedDevice,
) -> Result<CryptoStatus, String> {
    let _op = state.operation.lock().await;
    let (owner, p, generation) = context(&app, &cloud, &state, &ai_id).await?;
    if [
        &device.device_id,
        &device.fingerprint,
        &device.curve,
        &device.master,
    ]
    .iter()
    .any(|s| s.is_empty() || s.len() > 512)
    {
        return Err("设备及完整指纹不能为空或过长".into());
    }
    invoke(
        &app,
        &cloud,
        &owner,
        generation,
        &p,
        &ai_id,
        (
            "crypto-trust",
            &[
                owner.human.to_string(),
                device.device_id.clone(),
                device.fingerprint,
                device.curve,
                device.master,
            ],
        ),
    )
    .await?;
    read_status(
        &app,
        &cloud,
        &owner,
        generation,
        &p,
        &ai_id,
        &device.device_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_export_statement_signed_by_matrix_sdk() {
        // Public output of OlmMachine.sign in the bundled Matrix WASM SDK.
        let owner = Owner {
            server: "https://fixture.invalid".into(),
            human: "11111111-1111-4111-8111-111111111111".parse().unwrap(),
        };
        let ai = "22222222-2222-4222-8222-222222222222";
        let challenge = ExportChallenge {
            owner: owner.clone(),
            generation: 7,
            device: "fixture".into(),
            ai: ai.into(),
            path: "chosen-path".into(),
            nonce: "33333333-3333-4333-8333-333333333333".parse().unwrap(),
            issued: std::time::Instant::now(),
        };
        assert!(challenge.validate(&owner, 7, "fixture", ai, "dI2nvwphKdXPpQA5Ld7a5k6VrGA4ZWK7Awmbhxkfoyg",
            "7kJJI4b+Uvap8V4W7n894rOG8psKbehy6o+AR5uvt5MsTkybmlba1ZPg3pX5JZ4EbR1wdSSW3hhERMXNK1qQDw").is_ok());
    }
    #[test]
    fn export_requires_the_registered_device_signature_for_this_request() {
        use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
        use ed25519_dalek::{Signer, SigningKey};
        let key = SigningKey::from_bytes(&[1; 32]);
        let fingerprint = STANDARD_NO_PAD.encode(key.verifying_key().as_bytes());
        let owner = Owner {
            server: "https://test.invalid".into(),
            human: Uuid::new_v4(),
        };
        let mut challenge = ExportChallenge {
            owner: owner.clone(),
            generation: 3,
            device: "d".into(),
            ai: "a".into(),
            path: "chosen-path".into(),
            nonce: Uuid::new_v4(),
            issued: std::time::Instant::now(),
        };
        let proof = STANDARD_NO_PAD.encode(key.sign(challenge.statement().as_bytes()).to_bytes());
        assert!(
            challenge
                .validate(&owner, 3, "d", "a", &fingerprint, &proof)
                .is_ok()
        );
        assert!(
            challenge
                .validate(&owner, 4, "d", "a", &fingerprint, &proof)
                .is_err()
        );
        assert!(
            challenge
                .validate(&owner, 3, "other", "a", &fingerprint, &proof)
                .is_err()
        );
        assert!(
            challenge
                .validate(&owner, 3, "d", "other", &fingerprint, &proof)
                .is_err()
        );
        let other = SigningKey::from_bytes(&[2; 32]);
        assert!(
            challenge
                .validate(
                    &owner,
                    3,
                    "d",
                    "a",
                    &STANDARD_NO_PAD.encode(other.verifying_key().as_bytes()),
                    &proof
                )
                .is_err()
        );
        challenge.nonce = Uuid::new_v4();
        assert!(
            challenge
                .validate(&owner, 3, "d", "a", &fingerprint, &proof)
                .is_err()
        );
        let fresh = STANDARD_NO_PAD.encode(key.sign(challenge.statement().as_bytes()).to_bytes());
        challenge.issued -= Duration::from_secs(91);
        assert!(
            challenge
                .validate(&owner, 3, "d", "a", &fingerprint, &fresh)
                .is_err()
        );
    }
    #[tokio::test]
    async fn session_change_stops_owned_crypto_child_before_followup() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = command_at(
            &std::env::current_exe().unwrap(),
            "--exact",
            Path::new("local_agent::tests::slow_child_fixture"),
            None,
        )
        .unwrap();
        command
            .args(["--ignored", "--nocapture"])
            .env("LLMN_CHILD_TEST_DIR", dir.path());
        let result = run_guarded(command, "crypto-init", || {
            if dir.path().join("started").exists() {
                Err("登录身份已变化".into())
            } else {
                Ok(())
            }
        })
        .await;
        assert_eq!(result.unwrap_err(), "登录身份已变化");
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(!dir.path().join("finished").exists());
    }
    #[test]
    fn status_requires_current_complete_public_cli_contract() {
        let empty = br#"{"deviceId":null,"ed25519":null,"curve25519":null,"master":null,"identityReady":false,"recoverySaved":false,"peerTrusted":false,"ready":false}"#;
        let status = parse_status(empty).unwrap();
        assert!(!status.identity_ready);
        assert!(status.master.is_none());
        let established = br#"{"deviceId":"device","ed25519":"ed","curve25519":"curve","master":{"keys":{"ed25519:master":"public-master"}},"identityReady":true,"recoverySaved":true,"peerTrusted":true,"ready":true}"#;
        let status = parse_status(established).unwrap();
        assert_eq!(status.master.as_deref(), Some("public-master"));
        assert!(status.recovery_saved && status.peer_trusted);
        let partial = br#"{"deviceId":"device","ed25519":"ed","curve25519":"curve","master":null,"identityReady":false,"recoverySaved":false,"peerTrusted":false,"ready":false}"#;
        let status = parse_status(partial).unwrap();
        assert_eq!(status.device_id.as_deref(), Some("device"));
        assert!(!status.identity_ready);
        let mut incomplete: Value = serde_json::from_slice(partial).unwrap();
        incomplete["master"] = json!({"keys":{"ed25519:master":"public-master"}});
        assert!(
            !parse_status(&serde_json::to_vec(&incomplete).unwrap())
                .unwrap()
                .identity_ready
        );
        incomplete["master"] = Value::Null;
        incomplete["identityReady"] = json!(true);
        assert!(parse_status(&serde_json::to_vec(&incomplete).unwrap()).is_err());
        incomplete["deviceId"] = Value::Null;
        incomplete["identityReady"] = json!(false);
        assert!(parse_status(&serde_json::to_vec(&incomplete).unwrap()).is_err());
        let mut value: Value = serde_json::from_slice(established).unwrap();
        value.as_object_mut().unwrap().remove("identityReady");
        assert!(
            parse_status(&serde_json::to_vec(&value).unwrap())
                .unwrap_err()
                .contains("更新")
        );
        value["identityReady"] = json!(true);
        value["ed25519"] = json!(33);
        assert!(parse_status(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
