//! Native ownership of independent Agents; renderer never chooses a path or identity.
use crate::cloud::CloudState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};
use tauri::Manager;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, BufReader, Lines},
    sync::Mutex as AsyncMutex,
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Owner {
    server: String,
    human: Uuid,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    config: Value,
    secrets: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pairing_id: String,
    user_code: String,
    expires_at: String,
}
#[derive(Clone, Default)]
struct PairState {
    pairing: Option<Pairing>,
    active: bool,
    error: Option<String>,
    cancel: Option<tokio_util::sync::CancellationToken>,
}
pub struct LocalAgentState {
    root: PathBuf,
    operation: AsyncMutex<()>,
    pairs: Mutex<HashMap<PathBuf, PairState>>,
    owners: Mutex<Vec<Owner>>,
    initial_error: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupOptions {
    node_name: String,
    provider: String,
    model: String,
    protocol: Option<String>,
    base_url: Option<String>,
    persona: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    reasoning_effort: Option<String>,
    api_key: Option<String>,
    api_key_env: Option<String>,
}
impl LocalAgentState {
    pub(crate) fn direct_directory(
        &self,
        cloud: &CloudState,
    ) -> Result<(Uuid, PathBuf, u64), String> {
        let (server, human, generation) = cloud.authorized_local_session()?;
        Ok((human, self.dir(&Owner { server, human }), generation))
    }
    pub fn new() -> Self {
        Self::at_root(storage::default_data_dir().join("desktop-agents"))
    }
    fn at_root(root: PathBuf) -> Self {
        let (owners, initial_error) = match std::fs::read(root.join("owners.json")) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(v) => (v, None),
                Err(_) => (
                    Vec::new(),
                    Some("本机 Agent 索引损坏，请检查本机数据目录".into()),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), None),
            Err(e) => (Vec::new(), Some(io_error(e))),
        };
        Self {
            root,
            operation: AsyncMutex::new(()),
            pairs: Mutex::new(HashMap::new()),
            owners: Mutex::new(owners),
            initial_error,
        }
    }
    fn dir(&self, owner: &Owner) -> PathBuf {
        self.root
            .join(format!("{:x}", Sha256::digest(owner.server.as_bytes())))
            .join(owner.human.to_string())
    }
    async fn current(&self, cloud: &CloudState) -> Result<(Owner, PathBuf), String> {
        if let Some(e) = &self.initial_error {
            return Err(e.clone());
        }
        let (server, human) = cloud.local_identity().await?;
        let o = Owner { server, human };
        let p = self.dir(&o);
        Ok((o, p))
    }
    fn remember(&self, o: &Owner) -> Result<(), String> {
        let mut owners = self.owners.lock().unwrap();
        if !owners.contains(o) {
            let mut next = owners.clone();
            next.push(o.clone());
            std::fs::create_dir_all(&self.root).map_err(io_error)?;
            atomic_json(&self.root.join("owners.json"), &next)?;
            *owners = next;
        }
        Ok(())
    }
    pub fn background_explained(&self) -> bool {
        self.root.join("background-explained").is_file()
    }
    pub fn explain_background(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(io_error)?;
        atomic_bytes(&self.root.join("background-explained"), b"1")
    }
    pub async fn any_running(&self, app: &tauri::AppHandle) -> Result<bool, String> {
        let _op = self.operation.lock().await;
        if let Some(e) = &self.initial_error {
            return Err(e.clone());
        }
        if self.pairs.lock().unwrap().values().any(|p| p.active) {
            return Ok(true);
        }
        let owners = self.owners.lock().unwrap().clone();
        for o in owners {
            let p = self.dir(&o);
            if p.join("node.toml").exists() && live(app, &p).await?["running"] == true {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub async fn stop_all(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let _op = self.operation.lock().await;
        if let Some(e) = &self.initial_error {
            return Err(e.clone());
        }
        for p in self.pairs.lock().unwrap().values() {
            if let Some(c) = &p.cancel {
                c.cancel();
            }
        }
        for _ in 0..30 {
            if !self.pairs.lock().unwrap().values().any(|p| p.active) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if self.pairs.lock().unwrap().values().any(|p| p.active) {
            return Err("本机配对尚未停止，请重试退出".into());
        }
        let owners = self.owners.lock().unwrap().clone();
        for o in owners {
            let p = self.dir(&o);
            if p.join("node.toml").exists() {
                stop(app, &p).await?;
            }
        }
        Ok(())
    }
}
fn io_error(_: std::io::Error) -> String {
    "本机 Agent 文件操作失败，请检查本机目录权限和可用空间".into()
}
fn manifest(p: &Path) -> Result<Option<Manifest>, String> {
    match std::fs::read(p.join("desktop-config.json")) {
        Ok(v) => serde_json::from_slice(&v)
            .map(Some)
            .map_err(|_| "本机 Agent 配置损坏，请检查本机节点目录".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(e)),
    }
}
fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(io_error)?;
    f.write_all(bytes).map_err(io_error)?;
    f.as_file().sync_all().map_err(io_error)?;
    f.persist(path).map_err(|e| io_error(e.error))?;
    Ok(())
}
fn atomic_json(path: &Path, v: &impl Serialize) -> Result<(), String> {
    atomic_bytes(
        path,
        &serde_json::to_vec(v).map_err(|_| "本机配置无法编码")?,
    )
}
fn executable(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "llmn-agent.exe"
    } else {
        "llmn-agent"
    };
    let mut paths = vec![
        app.path()
            .resource_dir()
            .map_err(|_| "无法定位 Agent 资源目录")?
            .join("agent")
            .join(name),
    ];
    if cfg!(debug_assertions) {
        if let Some(p) = std::env::var_os("LLMN_AGENT_EXECUTABLE") {
            paths.insert(0, p.into());
        }
        if let Ok(p) = std::env::current_exe() {
            paths.push(p.parent().unwrap().join(name));
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        paths.push(root.join("target/x86_64-pc-windows-msvc/debug").join(name));
        paths.push(root.join("target/debug").join(name));
    }
    paths
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "未找到独立 Agent，请先构建 Agent 或重新安装完整桌面程序".into())
}
fn command(
    app: &tauri::AppHandle,
    verb: &str,
    p: &Path,
    m: Option<&Manifest>,
) -> Result<tokio::process::Command, String> {
    command_at(&executable(app)?, verb, p, m)
}
fn command_at(
    exe: &Path,
    verb: &str,
    p: &Path,
    m: Option<&Manifest>,
) -> Result<tokio::process::Command, String> {
    let mut c = tokio::process::Command::new(exe);
    c.arg(verb)
        .arg(p)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Some(v) = std::env::var_os(key) {
            c.env(key, v);
        }
    }
    if let Some(m) = m {
        for provider in m.config["providers"]
            .as_object()
            .ok_or("本机供应商配置无效")?
            .values()
        {
            let name = provider["api_key"]["env"]
                .as_str()
                .ok_or("本机模型凭据须引用环境变量")?;
            let value = m
                .secrets
                .get(name)
                .cloned()
                .or_else(|| std::env::var(name).ok())
                .ok_or_else(|| format!("请设置模型环境变量 {name} 或在本机填写 Key"))?;
            c.env(name, value);
        }
    }
    #[cfg(windows)]
    {
        c.creation_flags(0x08000000);
    }
    Ok(c)
}
async fn output(
    app: &tauri::AppHandle,
    verb: &str,
    p: &Path,
    m: Option<&Manifest>,
) -> Result<Value, String> {
    let mut child_command = command(app, verb, p, m)?;
    if verb == "check" {
        child_command.stderr(Stdio::piped());
    }
    let result = tokio::time::timeout(Duration::from_secs(20), child_command.output())
        .await
        .map_err(|_| "本机 Agent 操作超时")?
        .map_err(io_error)?;
    if !result.status.success() {
        if verb == "check" {
            // check never resolves credentials or calls a provider; TOML contains environment names only.
            let detail = String::from_utf8_lossy(&result.stderr)
                .chars()
                .take(4096)
                .collect::<String>();
            return Err(format!("本机配置校验失败：{}", detail.trim()));
        }
        return Err(format!(
            "本机 Agent {verb} 失败，请检查配置、环境变量或节点是否已由其他进程占用"
        ));
    }
    if result.stdout.len() > 1024 * 1024 {
        return Err("本机 Agent 响应过大".into());
    }
    serde_json::from_slice(&result.stdout).map_err(|_| "本机 Agent 响应格式无效".into())
}
async fn init(app: &tauri::AppHandle, p: &Path) -> Result<(), String> {
    let r = command(app, "init", p, None)?
        .output()
        .await
        .map_err(io_error)?;
    if r.status.success() {
        Ok(())
    } else {
        Err("无法创建受限的本机 Agent 目录".into())
    }
}
async fn live(app: &tauri::AppHandle, p: &Path) -> Result<Value, String> {
    output(app, "status", p, None).await
}
async fn stop(app: &tauri::AppHandle, p: &Path) -> Result<(), String> {
    if live(app, p).await?["running"] != true {
        return Ok(());
    }
    output(app, "stop", p, None).await?;
    for _ in 0..50 {
        if live(app, p).await?["running"] != true {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("本机 Agent 尚未停止，请稍后重试；不会结束其他进程".into())
}
fn materialize(p: &Path, m: &Manifest) -> Result<(), String> {
    let value: toml::Value =
        serde_json::from_value(m.config.clone()).map_err(|_| "本机节点配置无法编码")?;
    atomic_bytes(
        &p.join("node.toml"),
        toml::to_string_pretty(&value)
            .map_err(|_| "本机节点配置无法编码")?
            .as_bytes(),
    )
}
fn safe_config(m: &Manifest) -> Result<Value, String> {
    let p = &m.config["profile"];
    let provider = p["selection"]["provider"]
        .as_str()
        .ok_or("本机模型配置损坏")?;
    let route = &m.config["providers"][provider];
    let key = route["api_key"]["env"]
        .as_str()
        .ok_or("本机模型凭据引用损坏")?;
    Ok(
        json!({"nodeName":m.config["node_name"],"provider":provider,"model":p["selection"]["model"],"protocol":route["protocol"],"baseUrl":route["base_url"],"persona":p["persona"],"temperature":p["temperature"],"maxTokens":p["max_tokens"],"reasoningEffort":p["selection"]["reasoning_effort"],"apiKeyEnv":if m.secrets.contains_key(key){None}else{Some(key)},"hasSavedKey":m.secrets.contains_key(key)}),
    )
}
async fn status(
    app: &tauri::AppHandle,
    state: &LocalAgentState,
    p: &Path,
) -> Result<Value, String> {
    let m = manifest(p)?;
    let pair = state
        .pairs
        .lock()
        .unwrap()
        .get(p)
        .cloned()
        .unwrap_or_default();
    let v = if p.join("node.toml").exists() {
        live(app, p).await?
    } else {
        json!({"running":false,"aiIds":[]})
    };
    Ok(
        json!({"configured":m.is_some(),"running":!pair.active&&v["running"]==true,"nodeId":m.as_ref().map(|m|m.config["node_id"].clone()),"pairing":pair.pairing,"aiIds":v["aiIds"],"error":pair.error,"configuration":m.as_ref().map(safe_config).transpose()?}),
    )
}
#[tauri::command]
pub async fn local_agent_status(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
) -> Result<Value, String> {
    let _op = state.operation.lock().await;
    let (_, p) = state.current(&cloud).await?;
    status(&app, &state, &p).await
}
fn build_manifest(
    old: Option<Manifest>,
    o: SetupOptions,
    owner: &Owner,
) -> Result<Manifest, String> {
    if o.node_name.trim().is_empty() || o.node_name.chars().count() > 100 {
        return Err("节点名称须为1至100字".into());
    }
    let builtin = ai_client::catalog::builtin_provider(&o.provider).ok_or_else(|| {
        format!(
            "请选择内置供应商：{}",
            ai_client::catalog::BUILTIN_PROVIDERS
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>()
                .join("、")
        )
    })?;
    if !builtin.models.iter().any(|m| m.id == o.model) {
        return Err(format!(
            "请选择该供应商的模型：{}",
            builtin
                .models
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    if o.api_key.is_some() && o.api_key_env.is_some() {
        return Err("Key 与环境变量只能选择一种".into());
    }
    let mut m=old.unwrap_or_else(||Manifest{config:json!({"server":owner.server,"node_id":Uuid::new_v4(),"daily_calls":48,"daily_tokens":1048576,"providers":{}}),secrets:BTreeMap::new()});
    let previous_provider = m.config["profile"]["selection"]["provider"]
        .as_str()
        .map(str::to_owned);
    let env = if let Some(key) = o.api_key {
        if key.trim().is_empty() || key.len() > 16384 {
            return Err("模型 Key 为空或过长".into());
        }
        let name = format!("LLMN_DESKTOP_KEY_{}", Uuid::new_v4().simple());
        m.secrets.insert(name.clone(), key);
        name
    } else if let Some(name) = o.api_key_env {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
        {
            return Err("环境变量名称无效".into());
        }
        if std::env::var_os(&name).is_none() {
            return Err(format!("当前桌面进程未读取到环境变量 {name}"));
        }
        name
    } else if previous_provider.as_deref() == Some(&o.provider) {
        m.config["providers"][&o.provider]["api_key"]["env"]
            .as_str()
            .ok_or("请填写模型 Key 或环境变量")?
            .to_owned()
    } else {
        return Err("新供应商需要填写模型 Key 或环境变量".into());
    };
    let mut route = m.config["providers"]
        .get(&o.provider)
        .cloned()
        .unwrap_or_else(|| json!({}));
    route["api_key"] = json!({"env":env});
    if let Some(protocol) = o.protocol {
        if protocol.trim().is_empty() {
            route.as_object_mut().unwrap().remove("protocol");
        } else {
            route["protocol"] = json!(protocol);
        }
    }
    if let Some(url) = o.base_url {
        if url.trim().is_empty() {
            route.as_object_mut().unwrap().remove("base_url");
        } else {
            route["base_url"] = json!(url);
        }
    }
    m.config["providers"][&o.provider] = route;
    m.config["node_name"] = json!(o.node_name);
    let mut profile =
        m.config.get("profile").cloned().unwrap_or_else(
            || json!({"persona":"你是一位友善、诚实的 AI 联系人。","selection":{}}),
        );
    let selection_changed =
        profile["selection"]["provider"] != o.provider || profile["selection"]["model"] != o.model;
    if selection_changed && o.reasoning_effort.is_none() {
        profile["selection"]
            .as_object_mut()
            .unwrap()
            .remove("reasoning_effort");
    }
    profile["revision"] = json!(Uuid::new_v4());
    profile["selection"]["provider"] = json!(o.provider);
    profile["selection"]["model"] = json!(o.model);
    if let Some(v) = o.persona {
        profile["persona"] = json!(v);
    }
    if let Some(v) = o.reasoning_effort {
        profile["selection"]["reasoning_effort"] = json!(v);
    }
    if let Some(v) = o.temperature {
        profile["temperature"] = json!(v);
    }
    if let Some(v) = o.max_tokens {
        profile["max_tokens"] = json!(v);
    }
    m.config["profile"] = profile;
    if let Some(p) = std::env::var_os("CLOUD_CA_CERT_FILE") {
        m.config["ca_file"] = json!(p.to_string_lossy());
    }
    let used = m.config["providers"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|v| v["api_key"]["env"].as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    m.secrets.retain(|k, _| used.contains(k));
    Ok(m)
}
#[tauri::command]
pub async fn local_agent_setup(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
    options: SetupOptions,
) -> Result<Value, String> {
    let _op = state.operation.lock().await;
    let (owner, p) = state.current(&cloud).await?;
    if p.exists()
        && (live(&app, &p).await?["running"] == true
            || state
                .pairs
                .lock()
                .unwrap()
                .get(&p)
                .is_some_and(|p| p.active))
    {
        return Err("请先停止本机 Agent 并等待配对结束，再修改本机配置".into());
    }
    let m = build_manifest(manifest(&p)?, options, &owner)?;
    std::fs::create_dir_all(p.parent().unwrap()).map_err(io_error)?;
    let staging = tempfile::tempdir_in(p.parent().unwrap()).map_err(io_error)?;
    let candidate = staging.path().join("node");
    init(&app, &candidate).await?;
    materialize(&candidate, &m)?;
    output(&app, "check", &candidate, Some(&m)).await?;
    if !p.exists() {
        init(&app, &p).await?;
    }
    state.remember(&owner)?;
    atomic_json(&p.join("desktop-config.json"), &m)?;
    status(&app, &state, &p).await
}
#[tauri::command]
pub async fn local_agent_pair(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
) -> Result<Value, String> {
    let _op = state.operation.lock().await;
    let (_, p) = state.current(&cloud).await?;
    let m = manifest(&p)?.ok_or("请先保存本机模型配置")?;
    if live(&app, &p).await?["running"] == true
        || state
            .pairs
            .lock()
            .unwrap()
            .get(&p)
            .is_some_and(|p| p.active)
    {
        return Err("请先停止本机 Agent 或等待当前配对结束".into());
    }
    materialize(&p, &m)?;
    let mut pair_command = command(&app, "pair-json", &p, Some(&m))?;
    pair_command.kill_on_drop(true);
    let mut child = pair_command.spawn().map_err(io_error)?;
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let first = tokio::time::timeout(Duration::from_secs(20), lines.next_line())
        .await
        .map_err(|_| "获取配对码超时")?
        .map_err(io_error)?
        .ok_or("节点未返回配对码，请检查云端连接")?;
    let frame: Value = serde_json::from_str(&first).map_err(|_| "节点配对响应无效")?;
    if frame["type"] != "pairing" {
        return Err("节点未返回预期配对帧".into());
    }
    let pairing: Pairing = serde_json::from_value(frame).map_err(|_| "节点配对响应无效")?;
    let cancel = tokio_util::sync::CancellationToken::new();
    state.pairs.lock().unwrap().insert(
        p.clone(),
        PairState {
            pairing: Some(pairing),
            active: true,
            error: None,
            cancel: Some(cancel.clone()),
        },
    );
    let app2 = app.clone();
    let p2 = p.clone();
    tauri::async_runtime::spawn(async move {
        let error = supervise_pair(child, lines, cancel, |pairing| {
            app2.state::<LocalAgentState>()
                .pairs
                .lock()
                .unwrap()
                .get_mut(&p2)
                .unwrap()
                .pairing = Some(pairing);
        })
        .await
        .err();
        app2.state::<LocalAgentState>()
            .pairs
            .lock()
            .unwrap()
            .insert(
                p2,
                PairState {
                    pairing: None,
                    active: false,
                    error,
                    cancel: None,
                },
            );
    });
    status(&app, &state, &p).await
}
async fn supervise_pair<R: AsyncBufRead + Unpin>(
    mut child: tokio::process::Child,
    mut lines: Lines<R>,
    cancel: tokio_util::sync::CancellationToken,
    mut update: impl FnMut(Pairing),
) -> Result<(), String> {
    let result = tokio::select! {
        result = async {
            loop {
                let line = lines.next_line().await.map_err(io_error)?
                    .ok_or("本机配对未完成，请重新获取配对码")?;
                let frame: Value = serde_json::from_str(&line).map_err(|_| "节点配对响应无效")?;
                match frame["type"].as_str() {
                    Some("pairing") => update(serde_json::from_value(frame).map_err(|_| "节点配对响应无效")?),
                    Some("paired") => break,
                    _ => return Err("节点未返回预期配对帧".into()),
                }
            }
            if child.wait().await.map_err(io_error)?.success() {
                Ok(())
            } else {
                Err("本机配对未完成，请重新获取配对码".into())
            }
        } => Some(result),
        _ = cancel.cancelled() => None,
    };
    if result.as_ref().is_none_or(|result| result.is_err()) {
        // This is our still-owned pairing child, never a persisted process identifier.
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result.unwrap_or(Ok(()))
}
#[tauri::command]
pub async fn local_agent_start(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
) -> Result<Value, String> {
    let _op = state.operation.lock().await;
    let (_, p) = state.current(&cloud).await?;
    let m = manifest(&p)?.ok_or("请先配置本机 Agent")?;
    if state
        .pairs
        .lock()
        .unwrap()
        .get(&p)
        .is_some_and(|p| p.active)
    {
        return Err("请等待配对完成".into());
    }
    if live(&app, &p).await?["running"] != true {
        materialize(&p, &m)?;
        output(&app, "check", &p, Some(&m)).await?;
        let mut c = command(&app, "run-managed", &p, Some(&m))?;
        c.kill_on_drop(false);
        let mut child = c.spawn().map_err(io_error)?;
        // A status lock probe can beat the child's exclusive startup lock. Wait for
        // the child's acknowledgement before allowing any status process to run.
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        if let Err(error) = wait_ready(&mut lines).await {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
        tauri::async_runtime::spawn(async move {
            let _ = child.wait().await;
        });
    }
    status(&app, &state, &p).await
}
async fn wait_ready<R: AsyncBufRead + Unpin>(lines: &mut Lines<R>) -> Result<(), String> {
    let line = tokio::time::timeout(Duration::from_secs(20), lines.next_line())
        .await
        .map_err(|_| "本机 Agent 启动确认超时")?
        .map_err(io_error)?
        .ok_or("本机 Agent 未能启动，请检查配对与配置")?;
    let frame: Value = serde_json::from_str(&line).map_err(|_| "本机 Agent 启动响应无效")?;
    if frame["type"] != "ready"
        || !frame["instanceId"]
            .as_str()
            .is_some_and(|id| Uuid::parse_str(id).is_ok())
    {
        return Err("本机 Agent 未返回启动确认".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn local_agent_stop(
    app: tauri::AppHandle,
    cloud: tauri::State<'_, CloudState>,
    state: tauri::State<'_, LocalAgentState>,
) -> Result<Value, String> {
    let _op = state.operation.lock().await;
    let (_, p) = state.current(&cloud).await?;
    if p.join("node.toml").exists() {
        stop(&app, &p).await?;
    }
    status(&app, &state, &p).await
}
#[tauri::command]
pub fn local_agent_models() -> Value {
    json!({"providers":ai_client::catalog::BUILTIN_PROVIDERS.iter().map(|p|json!({"id":p.id,"name":p.display_name,"models":p.models.iter().map(|m|json!({"id":m.id,"name":m.display_name,"contextWindow":m.context_window,"reasoningEfforts":m.reasoning.map(|r|r.levels).unwrap_or(&[])})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> Owner {
        Owner {
            server: "https://cloud.example".into(),
            human: Uuid::new_v4(),
        }
    }
    fn options() -> SetupOptions {
        serde_json::from_value(json!({"nodeName":"本机节点","provider":"deepseek","model":"deepseek-chat","apiKey":"test-key-private-only"})).unwrap()
    }
    #[test]
    fn config_keys_stay_private_and_child_only_never_renderer_or_argv() {
        assert!(serde_json::from_value::<SetupOptions>(json!({"nodeName":"x","provider":"deepseek","model":"deepseek-chat","server":"https://untrusted.example","dataDir":"C:/arbitrary"})).is_err());
        let dir = tempfile::tempdir().unwrap();
        let m = build_manifest(None, options(), &owner()).unwrap();
        atomic_json(&dir.path().join("desktop-config.json"), &m).unwrap();
        materialize(dir.path(), &m).unwrap();
        let restored = manifest(dir.path()).unwrap().unwrap();
        assert!(
            !safe_config(&restored)
                .unwrap()
                .to_string()
                .contains("test-key-private-only")
        );
        assert!(
            !std::fs::read_to_string(dir.path().join("node.toml"))
                .unwrap()
                .contains("test-key-private-only")
        );
        let c = command_at(
            Path::new("llmn-agent"),
            "run-managed",
            dir.path(),
            Some(&restored),
        )
        .unwrap();
        assert!(
            c.as_std()
                .get_args()
                .all(|a| !a.to_string_lossy().contains("test-key-private-only"))
        );
        assert!(c.as_std().get_envs().any(|(k, v)| {
            k.to_string_lossy().starts_with("LLMN_DESKTOP_KEY_")
                && v == Some(std::ffi::OsStr::new("test-key-private-only"))
        }));
        assert!(std::env::var_os(restored.secrets.keys().next().unwrap()).is_none());
    }
    #[test]
    fn server_and_actual_human_isolate_dirs_and_index_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let state = LocalAgentState::at_root(dir.path().into());
        let first = owner();
        let other = Owner {
            human: Uuid::new_v4(),
            ..first.clone()
        };
        let server = Owner {
            server: "https://other.example".into(),
            ..first.clone()
        };
        assert_ne!(state.dir(&first), state.dir(&other));
        assert_ne!(state.dir(&first), state.dir(&server));
        state.remember(&first).unwrap();
        state.remember(&first).unwrap();
        state.remember(&other).unwrap();
        let restored = LocalAgentState::at_root(dir.path().into());
        assert_eq!(restored.owners.lock().unwrap().len(), 2);
        let failed = tempfile::tempdir().unwrap();
        let fail_state = LocalAgentState::at_root(failed.path().into());
        std::fs::create_dir(failed.path().join("owners.json")).unwrap();
        assert!(fail_state.remember(&first).is_err());
        assert!(fail_state.owners.lock().unwrap().is_empty());
        std::fs::write(dir.path().join("owners.json"), b"broken").unwrap();
        assert!(
            LocalAgentState::at_root(dir.path().into())
                .initial_error
                .is_some()
        );
    }
    #[test]
    fn empty_key_preserves_same_provider_and_omitted_settings_but_never_cross_provider() {
        let o = owner();
        let mut initial = options();
        initial.protocol = Some("openai".into());
        initial.base_url = Some("https://model.example/v1".into());
        initial.persona = Some("role".into());
        initial.max_tokens = Some(2048);
        let m = build_manifest(None, initial, &o).unwrap();
        let key = m.config["providers"]["deepseek"]["api_key"].clone();
        let mut next = options();
        next.api_key = None;
        let m = build_manifest(Some(m), next, &o).unwrap();
        assert_eq!(m.config["providers"]["deepseek"]["api_key"], key);
        assert_eq!(
            m.config["providers"]["deepseek"]["base_url"],
            "https://model.example/v1"
        );
        assert_eq!(m.config["profile"]["persona"], "role");
        assert_eq!(m.config["profile"]["max_tokens"], 2048);
        let mut other = options();
        other.provider = "openai".into();
        other.model = ai_client::catalog::builtin_provider("openai")
            .unwrap()
            .default_model
            .into();
        other.api_key = None;
        assert!(build_manifest(Some(m), other, &o).is_err());
    }
    #[test]
    fn explicit_empty_route_clears_overrides_and_new_selection_does_not_inherit_old_effort() {
        let o = owner();
        let mut first = options();
        first.model = "deepseek-reasoner".into();
        first.reasoning_effort = Some("high".into());
        first.protocol = Some("openai".into());
        first.base_url = Some("https://model.example/v1".into());
        let m = build_manifest(None, first, &o).unwrap();
        let mut same = options();
        same.model = "deepseek-reasoner".into();
        same.api_key = None;
        let m = build_manifest(Some(m), same, &o).unwrap();
        assert_eq!(m.config["profile"]["selection"]["reasoning_effort"], "high");
        let mut changed = options();
        changed.api_key = None;
        changed.protocol = Some("".into());
        changed.base_url = Some("".into());
        let m = build_manifest(Some(m), changed, &o).unwrap();
        assert!(m.config["providers"]["deepseek"].get("protocol").is_none());
        assert!(m.config["providers"]["deepseek"].get("base_url").is_none());
        assert!(
            m.config["profile"]["selection"]
                .get("reasoning_effort")
                .is_none()
        );
    }
    #[test]
    fn keys_replacement_prunes_old_secret_and_invalid_manifest_fails_instead_of_fallback() {
        let o = owner();
        let first = build_manifest(None, options(), &o).unwrap();
        let old = first.secrets.keys().next().unwrap().clone();
        let mut next = options();
        next.api_key = Some("replacement-fixture-key".into());
        let second = build_manifest(Some(first), next, &o).unwrap();
        assert!(!second.secrets.contains_key(&old));
        assert_eq!(second.secrets.len(), 1);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("desktop-config.json"), b"{broken").unwrap();
        assert!(manifest(dir.path()).is_err());
    }
    #[test]
    #[ignore]
    fn slow_child_fixture() {
        let Some(dir) = std::env::var_os("LLMN_CHILD_TEST_DIR") else {
            return;
        };
        let path = PathBuf::from(dir);
        std::fs::write(path.join("started"), b"1").unwrap();
        std::thread::sleep(Duration::from_secs(2));
        std::fs::write(path.join("finished"), b"1").unwrap();
    }
    #[tokio::test]
    async fn abandoned_short_command_terminates_owned_child_and_releases_it() {
        let dir = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut c = command_at(
            &exe,
            "--exact",
            Path::new("local_agent::tests::slow_child_fixture"),
            None,
        )
        .unwrap();
        c.args(["--ignored", "--nocapture"])
            .env("LLMN_CHILD_TEST_DIR", dir.path())
            .stdout(Stdio::null());
        let child = c.spawn().unwrap();
        for _ in 0..100 {
            if dir.path().join("started").exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(dir.path().join("started").exists());
        drop(child);
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(!dir.path().join("finished").exists());
    }
    fn pairing_child(dir: Option<&Path>) -> tokio::process::Child {
        let mut c = command_at(
            &std::env::current_exe().unwrap(),
            "--exact",
            Path::new("local_agent::tests::slow_child_fixture"),
            None,
        )
        .unwrap();
        c.args(["--ignored", "--nocapture"]).stdout(Stdio::null());
        if let Some(dir) = dir {
            c.env("LLMN_CHILD_TEST_DIR", dir);
        }
        c.spawn().unwrap()
    }
    #[tokio::test]
    async fn expired_pairing_updates_code_then_paired_completes_normally() {
        let (writer, reader) = tokio::io::duplex(2048);
        let mut writer = writer;
        use tokio::io::AsyncWriteExt;
        writer.write_all(b"{\"type\":\"pairing\",\"pairingId\":\"renewed\",\"userCode\":\"NEW-CODE\",\"expiresAt\":\"later\"}\n{\"type\":\"paired\",\"aiId\":\"ai\",\"bindingVersion\":1}\n").await.unwrap();
        let mut codes = Vec::new();
        supervise_pair(
            pairing_child(None),
            BufReader::new(reader).lines(),
            tokio_util::sync::CancellationToken::new(),
            |p| codes.push(p.user_code),
        )
        .await
        .unwrap();
        assert_eq!(codes, ["NEW-CODE"]);
    }
    #[tokio::test]
    async fn renewed_pairing_stays_cancellable_in_read_and_after_paired() {
        use tokio::io::AsyncWriteExt;
        for paired in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let child = pairing_child(Some(dir.path()));
            for _ in 0..100 {
                if dir.path().join("started").exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(dir.path().join("started").exists());
            let (mut writer, reader) = tokio::io::duplex(2048);
            writer.write_all(b"{\"type\":\"pairing\",\"pairingId\":\"renewed\",\"userCode\":\"NEW-CODE\",\"expiresAt\":\"later\"}\n").await.unwrap();
            if paired {
                writer.write_all(b"{\"type\":\"paired\"}\n").await.unwrap();
            }
            let cancel = tokio_util::sync::CancellationToken::new();
            let task_cancel = cancel.clone();
            let updated = std::sync::Arc::new(tokio::sync::Notify::new());
            let task_updated = updated.clone();
            let task = tokio::spawn(async move {
                supervise_pair(child, BufReader::new(reader).lines(), task_cancel, |_| {
                    task_updated.notify_one();
                })
                .await
            });
            updated.notified().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel.cancel();
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(!dir.path().join("finished").exists());
        }
    }
    #[tokio::test]
    async fn startup_requires_child_ready_acknowledgement_before_state_probe() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, reader) = tokio::io::duplex(1024);
        let mut lines = BufReader::new(reader).lines();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), wait_ready(&mut lines))
                .await
                .is_err()
        );
        writer
            .write_all(
                format!(
                    "{{\"type\":\"ready\",\"instanceId\":\"{}\"}}\n",
                    Uuid::new_v4()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        wait_ready(&mut lines).await.unwrap();
        writer.write_all(b"{\"type\":\"status\"}\n").await.unwrap();
        assert!(wait_ready(&mut lines).await.is_err());
        drop(writer);
        assert!(wait_ready(&mut lines).await.is_err());
    }
}
