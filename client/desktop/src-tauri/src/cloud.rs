use crate::secure_credentials::{self, Session, Unlock, Vault};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use reqwest::{Client, Method, Url, header};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::Emitter;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct CloudState {
    shared: Arc<Mutex<Inner>>,
}
type AuthEventSink = dyn Fn(AuthLost) -> Result<(), String> + Send + Sync;
struct Inner {
    client: Result<Client, String>,
    server: Option<Url>,
    cookie: Option<String>,
    generation: u64,
    local_human: Option<Uuid>,
    subscriptions: HashMap<String, Subscription>,
    owner_versions: HashMap<String, u64>,
    vault: Vault,
    credential_owner: Option<Uuid>,
    persistent_until: Option<u64>,
    auth_events: Option<Arc<AuthEventSink>>,
    #[cfg(test)]
    emitted_auth_lost: Vec<AuthLost>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthLost {
    server: String,
    account_id: Option<Uuid>,
    message: Option<String>,
}
struct Subscription {
    owner: String,
    cancel: CancellationToken,
}
#[derive(Clone)]
struct RequestContext {
    client: Client,
    url: Url,
    cookie: Option<String>,
    generation: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudResponse {
    status: u16,
    body: String,
    content_type: String,
    file_name: Option<String>,
    binary: bool,
}
#[derive(Serialize)]
pub struct SaveResult {
    saved: bool,
}
impl CloudState {
    fn client() -> Result<Client, String> {
        let mut client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15));
        if let Some(path) = std::env::var_os("CLOUD_CA_CERT_FILE") {
            let pem = std::fs::read(path).map_err(|_| "无法读取指定的云端 CA 证书")?;
            let cert = reqwest::Certificate::from_pem(&pem).map_err(|_| "云端 CA 证书无效")?;
            client = client.add_root_certificate(cert);
        }
        client.build().map_err(|_| "无法初始化云端连接".into())
    }
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Inner {
                client: Self::client(),
                server: None,
                cookie: None,
                generation: 0,
                local_human: None,
                subscriptions: HashMap::new(),
                owner_versions: HashMap::new(),
                vault: Vault::new(),
                credential_owner: None,
                persistent_until: None,
                auth_events: None,
                #[cfg(test)]
                emitted_auth_lost: Vec::new(),
            })),
        }
    }
    pub(crate) fn with_events(app: tauri::AppHandle) -> Self {
        let state = Self::new();
        state.shared.lock().unwrap().auth_events = Some(Arc::new(move |event| {
            app.emit("cloud-auth-lost", event)
                .map_err(|_| "无法通知界面登录已失效".into())
        }));
        state
    }
    pub fn release_owner(&self, owner: &str) {
        let mut s = self.shared.lock().unwrap();
        *s.owner_versions.entry(owner.into()).or_default() += 1;
        s.subscriptions.retain(|_, v| {
            if v.owner == owner {
                v.cancel.cancel();
                false
            } else {
                true
            }
        });
    }
    fn context(&self, path: &str, auth_change: bool) -> Result<RequestContext, String> {
        let mut s = self.shared.lock().unwrap();
        expire_session(&mut s)?;
        let server = s.server.as_ref().ok_or("请先设置云端服务器")?;
        let url = target(server, path)?;
        if auth_change {
            advance(&mut s);
        }
        Ok(RequestContext {
            client: s.client.clone()?,
            url,
            cookie: s.cookie.clone(),
            generation: s.generation,
        })
    }
    pub(crate) async fn local_identity(&self) -> Result<(String, Uuid), String> {
        let ctx = self.context("/api/me", false)?;
        let response = request(&ctx, Method::GET)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(connection_error)?;
        self.reject_session(&ctx, response.status())?;
        if !response.status().is_success() {
            return Err("请先登录云端真人账号".into());
        }
        let account: serde_json::Value = response.json().await.map_err(|_| "云端身份响应无效")?;
        if account["kind"] != "human" {
            return Err("本机节点管理需要实际登录的真人账号".into());
        }
        let owner = account["id"]
            .as_str()
            .and_then(|v| v.parse::<Uuid>().ok())
            .ok_or("云端身份响应无效")?;
        let s = self.shared.lock().unwrap();
        if s.generation != ctx.generation {
            return Err("登录身份已变化，请重试".into());
        }
        Ok((ctx.url.origin().ascii_serialization(), owner))
    }
    #[cfg(test)]
    fn authorized_local_identity(&self) -> Result<(String, Uuid), String> {
        self.authorized_local_session()
            .map(|(server, human, _)| (server, human))
    }
    pub(crate) fn authorized_local_session(&self) -> Result<(String, Uuid, u64), String> {
        let mut s = self.shared.lock().unwrap();
        expire_session(&mut s)?;
        Ok((
            s.server
                .as_ref()
                .ok_or("请先设置云端服务器")?
                .origin()
                .ascii_serialization(),
            s.local_human
                .ok_or("请先在线登录真人账号，再使用同机私聊")?,
            s.generation,
        ))
    }
    pub(crate) fn session_generation(&self) -> u64 {
        self.shared.lock().unwrap().generation
    }
    pub(crate) async fn direct_grant(
        &self,
        conversation: &str,
        binding: &connector::direct::DirectBinding,
    ) -> Result<String, String> {
        let ctx = self.context(
            &format!("/api/chat/conversations/{conversation}/direct"),
            false,
        )?;
        let response = request(&ctx, Method::POST)
            .timeout(Duration::from_secs(60))
            .json(&connector::direct::DirectGrantRequest {
                node_id: binding.node_id.clone(),
                binding_version: binding.binding_version.clone(),
            })
            .send()
            .await
            .map_err(connection_error)?;
        self.reject_session(&ctx, response.status())?;
        if !response.status().is_success() {
            return Err("同机私聊授权失败，请检查登录、AI 管理权及当前节点绑定".into());
        }
        let grant: connector::direct::DirectGrant =
            response.json().await.map_err(|_| "同机授权响应无效")?;
        if self.shared.lock().unwrap().generation != ctx.generation {
            return Err("登录身份已变化".into());
        }
        Ok(grant.token)
    }
    fn cookies(
        &self,
        ctx: &RequestContext,
        response: &reqwest::Response,
        remember_login: bool,
    ) -> Result<Option<u64>, String> {
        let mut s = self.shared.lock().unwrap();
        if s.generation != ctx.generation {
            return Ok(None);
        }
        for h in response.headers().get_all(header::SET_COOKIE) {
            if let Ok(v) = h.to_str()
                && let Some(value) = v
                    .split(';')
                    .next()
                    .and_then(|p| p.strip_prefix("nest_session="))
            {
                if value.is_empty() {
                    invalidate(&mut s)?;
                } else {
                    let max_age = v
                        .split(';')
                        .find_map(|part| part.trim().strip_prefix("Max-Age=")?.parse::<u64>().ok());
                    // The cookie fixes the deadline; /me and normal traffic never refresh it.
                    let expires = max_age.filter(|v| remember_login && *v > 0).map(|seconds| {
                        secure_credentials::now_ms() + seconds.min(2_592_000) * 1000
                    });
                    let server = s.server.as_ref().unwrap().origin().ascii_serialization();
                    s.vault.remove_session(&server)?;
                    if s.cookie.as_deref() != Some(&format!("nest_session={value}")) {
                        advance(&mut s);
                    }
                    s.cookie = Some(format!("nest_session={value}"));
                    s.persistent_until = expires;
                }
            }
        }
        Ok(Some(s.generation))
    }
    fn reject_session(
        &self,
        ctx: &RequestContext,
        status: reqwest::StatusCode,
    ) -> Result<(), String> {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            let mut s = self.shared.lock().unwrap();
            if s.generation == ctx.generation {
                invalidate(&mut s)?;
            }
        }
        Ok(())
    }
    fn confirm_account(&self, generation: u64, account: &serde_json::Value) -> Result<(), String> {
        let mut s = self.shared.lock().unwrap();
        if s.generation != generation {
            return Ok(());
        }
        if account["kind"] != "human" {
            return Err("云端身份响应无效".into());
        }
        let owner: Uuid = account["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .ok_or("云端身份响应无效")?;
        let server = s.server.as_ref().unwrap().origin().ascii_serialization();
        s.credential_owner = Some(owner);
        if let Some(expires_at) = s.persistent_until {
            s.vault.save_session(
                &server,
                &Session {
                    account: owner,
                    cookie: s.cookie.clone().ok_or("缺少云端会话")?,
                    expires_at,
                },
            )?;
        }
        s.local_human = Some(owner);
        Ok(())
    }
}
fn expire_session(s: &mut Inner) -> Result<(), String> {
    if s.persistent_until
        .is_some_and(|v| v <= secure_credentials::now_ms())
    {
        invalidate(s)?;
    }
    Ok(())
}
fn invalidate(s: &mut Inner) -> Result<(), String> {
    let event = s.server.as_ref().map(|server| AuthLost {
        server: server.origin().ascii_serialization(),
        account_id: s.credential_owner,
        message: None,
    });
    let cleanup = if let Some(server) = &s.server {
        s.vault
            .clear(&server.origin().ascii_serialization(), s.credential_owner)
    } else {
        Ok(())
    };
    advance(s);
    s.cookie = None;
    s.credential_owner = None;
    s.persistent_until = None;
    if let Some(mut event) = event {
        event.message = cleanup.as_ref().err().cloned();
        #[cfg(test)]
        s.emitted_auth_lost.push(event.clone());
        if let Some(emit) = &s.auth_events {
            emit(event)?;
        }
    }
    cleanup
}
fn advance(s: &mut Inner) {
    s.generation += 1;
    s.local_human = None;
    for (_, v) in s.subscriptions.drain() {
        v.cancel.cancel();
    }
}
fn server_url(value: &str) -> Result<Url, String> {
    let u = Url::parse(value.trim()).map_err(|_| "请输入完整服务器地址")?;
    let loopback = u
        .host_str()
        .is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h == "[::1]" || h == "::1");
    if !(u.scheme() == "https" || u.scheme() == "http" && loopback)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
    {
        return Err("服务器须为 HTTPS origin；仅本机开发允许 HTTP".into());
    }
    Ok(u)
}
fn target(server: &Url, path: &str) -> Result<Url, String> {
    if !path.starts_with("/api/") || path.contains('\\') || path.contains('#') {
        return Err("仅允许当前服务器的 /api 请求".into());
    }
    let u = server.join(path).map_err(|_| "请求路径无效")?;
    if u.origin() != server.origin() || !u.path().starts_with("/api/") {
        return Err("请求路径无效".into());
    }
    Ok(u)
}
fn request(ctx: &RequestContext, method: Method) -> reqwest::RequestBuilder {
    let mut b = ctx
        .client
        .request(method, ctx.url.clone())
        .header("x-llmn-client", "1");
    if let Some(c) = &ctx.cookie {
        b = b.header(header::COOKIE, c);
    }
    b
}
fn filename(response: &reqwest::Response) -> Option<String> {
    let h = response
        .headers()
        .get(header::CONTENT_DISPOSITION)?
        .to_str()
        .ok()?;
    let encoded = h
        .split(';')
        .find_map(|v| v.trim().strip_prefix("filename*=UTF-8''"));
    let decoded = encoded.and_then(|v| percent_encoding::percent_decode_str(v).decode_utf8().ok());
    let quoted = h.split(';').find_map(|v| {
        v.trim()
            .strip_prefix("filename=\"")
            .and_then(|v| v.strip_suffix('"'))
    });
    let n = decoded.as_deref().or(quoted)?;
    if n.is_empty() || n.contains(['/', '\\', '\0', '\r', '\n']) || n == "." || n == ".." {
        return None;
    }
    Some(n.to_owned())
}
#[tauri::command]
pub async fn cloud_set_server(
    state: tauri::State<'_, CloudState>,
    url: String,
) -> Result<(), String> {
    let server = server_url(&url)?;
    {
        let mut s = state.shared.lock().unwrap();
        if s.server.as_ref() != Some(&server) {
            let saved = s
                .vault
                .load_session(&server.origin().ascii_serialization())?;
            advance(&mut s);
            s.server = Some(server);
            s.cookie = saved.as_ref().map(|v| v.cookie.clone());
            s.credential_owner = saved.as_ref().map(|v| v.account);
            s.persistent_until = saved.map(|v| v.expires_at);
        } else if s.cookie.is_none() {
            return Ok(());
        }
        if s.cookie.is_none() || s.local_human.is_some() {
            return Ok(());
        }
    }
    let ctx = state.context("/api/me", false)?;
    let response = request(&ctx, Method::GET)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(connection_error)?;
    state.reject_session(&ctx, response.status())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(());
    }
    if !response.status().is_success() {
        return Err("无法验证保存的登录会话，请重试".into());
    }
    let account = response.json().await.map_err(|_| "云端身份响应无效")?;
    state.confirm_account(ctx.generation, &account)
}
#[tauri::command]
pub fn cloud_unlock_support() -> bool {
    secure_credentials::supported()
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSaved {
    expires_at: u64,
}
#[tauri::command]
pub fn cloud_unlock_save(
    state: tauri::State<'_, CloudState>,
    device_id: String,
    secret: String,
    expected_server: String,
    expected_account_id: String,
) -> Result<UnlockSaved, String> {
    let device = parse_device(&device_id)?;
    let mut s = state.shared.lock().unwrap();
    let (server, owner) = expected_unlock_scope(&mut s, &expected_server, &expected_account_id)?;
    Ok(UnlockSaved {
        expires_at: s.vault.save_unlock(&server, owner, device, secret)?,
    })
}
#[tauri::command]
pub fn cloud_unlock_load(
    state: tauri::State<'_, CloudState>,
    device_id: String,
    expected_server: String,
    expected_account_id: String,
) -> Result<Option<Unlock>, String> {
    let device = parse_device(&device_id)?;
    let mut s = state.shared.lock().unwrap();
    let (server, owner) = expected_unlock_scope(&mut s, &expected_server, &expected_account_id)?;
    s.vault.load_unlock(&server, owner, device)
}
#[tauri::command]
pub fn cloud_unlock_remove(
    state: tauri::State<'_, CloudState>,
    device_id: String,
    expected_server: String,
    expected_account_id: String,
) -> Result<(), String> {
    let device = parse_device(&device_id)?;
    let mut s = state.shared.lock().unwrap();
    let (server, owner) = expected_unlock_scope(&mut s, &expected_server, &expected_account_id)?;
    s.vault.remove_unlock(&server, owner, device)
}
fn parse_device(value: &str) -> Result<Uuid, String> {
    value
        .parse::<Uuid>()
        .ok()
        .filter(|v| !v.is_nil())
        .ok_or_else(|| "设备 ID 无效".into())
}
fn unlock_scope(s: &mut Inner) -> Result<(String, Uuid), String> {
    expire_session(s)?;
    let owner = s.local_human.ok_or("请先在线验证当前登录身份")?;
    Ok((
        s.server
            .as_ref()
            .ok_or("请先设置云端服务器")?
            .origin()
            .ascii_serialization(),
        owner,
    ))
}
fn expected_unlock_scope(
    s: &mut Inner,
    expected_server: &str,
    expected_account: &str,
) -> Result<(String, Uuid), String> {
    let expected_server = server_url(expected_server)?.origin().ascii_serialization();
    let expected_account = expected_account
        .parse::<Uuid>()
        .map_err(|_| "登录账号 ID 无效")?;
    let scope = unlock_scope(s)?;
    if scope.0 != expected_server || scope.1 != expected_account {
        return Err("登录身份已变化，请重试".into());
    }
    Ok(scope)
}
#[tauri::command]
pub async fn cloud_request(
    state: tauri::State<'_, CloudState>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<CloudResponse, String> {
    cloud_request_inner(&state, method, path, body).await
}
async fn cloud_request_inner(
    state: &CloudState,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<CloudResponse, String> {
    let method = request_method(&method)?;
    let auth_change = method == Method::POST
        && matches!(
            path.as_str(),
            "/api/auth/login" | "/api/auth/verify" | "/api/auth/logout"
        );
    let remember_login = method == Method::POST && remember_login_requested(&path, body.as_ref());
    let ctx = state.context(&path, auth_change)?;
    let mut b = request(&ctx, method).timeout(Duration::from_secs(60));
    if let Some(body) = body {
        b = b.json(&body);
    }
    let response = b.send().await.map_err(connection_error)?;
    let response_generation = match state.cookies(&ctx, &response, remember_login) {
        Ok(generation) => generation,
        Err(error)
            if path == "/api/auth/logout"
                || response.status() == reqwest::StatusCode::UNAUTHORIZED =>
        {
            return Ok(credential_cleanup_failure(error));
        }
        Err(error) => return Err(error),
    };
    if let Err(error) = state.reject_session(&ctx, response.status()) {
        return Ok(credential_cleanup_failure(error));
    }
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let file_name = filename(&response);
    let binary = !(content_type.starts_with("text/") || content_type.contains("json"));
    let bytes = response.bytes().await.map_err(|_| "云端响应读取失败")?;
    if matches!(path.as_str(), "/api/auth/reset" | "/api/auth/logout") && status < 300 {
        let mut s = state.shared.lock().unwrap();
        if s.generation == ctx.generation
            && let Err(error) = invalidate(&mut s)
        {
            return Ok(credential_cleanup_failure(error));
        }
    }
    let body = if binary {
        STANDARD.encode(bytes)
    } else {
        String::from_utf8(bytes.to_vec()).map_err(|_| "云端文本编码无效")?
    };
    if status < 300
        && let Some(response_generation) = response_generation
        && matches!(
            path.as_str(),
            "/api/me" | "/api/auth/login" | "/api/auth/verify"
        )
    {
        let account: serde_json::Value =
            serde_json::from_str(&body).map_err(|_| "云端身份响应无效")?;
        let account = if path == "/api/me" {
            &account
        } else {
            &account["account"]
        };
        state.confirm_account(response_generation, account)?;
    }
    Ok(CloudResponse {
        status,
        body,
        content_type,
        file_name,
        binary,
    })
}
fn remember_login_requested(path: &str, body: Option<&serde_json::Value>) -> bool {
    secure_credentials::supported()
        && matches!(path, "/api/auth/login" | "/api/auth/verify")
        && body
            .and_then(|v| v.get("rememberLogin"))
            .and_then(|v| v.as_bool())
            == Some(true)
}
fn credential_cleanup_failure(message: String) -> CloudResponse {
    CloudResponse {
        status: 401,
        body: serde_json::json!({"message":message,"code":"native_credential_cleanup_failed"})
            .to_string(),
        content_type: "application/json".into(),
        file_name: None,
        binary: false,
    }
}
fn request_method(value: &str) -> Result<Method, String> {
    let method = Method::from_bytes(value.as_bytes()).map_err(|_| "HTTP 方法无效")?;
    if ![
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
    ]
    .contains(&method)
    {
        return Err("HTTP 方法不支持".into());
    }
    Ok(method)
}
fn connection_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        return "云端连接超时，请检查服务器地址和网络".into();
    }
    use std::error::Error;
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string().to_ascii_lowercase();
        if [
            "certificate",
            "cert",
            "issuer",
            "revocation",
            "security package",
        ]
        .iter()
        .any(|v| text.contains(v))
        {
            return "云端 TLS 证书校验失败，请检查证书链、有效期及服务器名称".into();
        }
        source = cause.source();
    }
    "无法连接云端服务器，请检查服务器地址、网络和 TLS 配置".into()
}
#[tauri::command]
pub async fn cloud_subscribe(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, CloudState>,
    path: String,
    on_chunk: Channel<String>,
) -> Result<String, String> {
    let owner = window.label().to_owned();
    let owner_version = *state
        .shared
        .lock()
        .unwrap()
        .owner_versions
        .entry(owner.clone())
        .or_default();
    let ctx = state.context(&path, false)?;
    let response = tokio::time::timeout(
        Duration::from_secs(30),
        request(&ctx, Method::GET)
            .header(header::ACCEPT, "text/event-stream")
            .send(),
    )
    .await
    .map_err(|_| "云端订阅超时")?
    .map_err(connection_error)?;
    if !response.status().is_success()
        || !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"))
    {
        state.reject_session(&ctx, response.status())?;
        return Err("云端消息订阅失败，请重新登录或重试".into());
    }
    let id = Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    {
        let mut s = state.shared.lock().unwrap();
        if s.generation != ctx.generation {
            return Err("会话已变化，请重新订阅".into());
        }
        if s.owner_versions.get(&owner).copied().unwrap_or_default() != owner_version {
            return Err("页面已重新加载".into());
        }
        s.subscriptions.insert(
            id.clone(),
            Subscription {
                owner,
                cancel: cancel.clone(),
            },
        );
    }
    let state = state.inner().clone();
    let task_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut utf8 = Utf8Chunks::default();
        loop {
            tokio::select! {biased; _ = cancel.cancelled()=>break,next=stream.next()=>{match next{Some(Ok(b))=>match utf8.push(&b){Ok(text)=>{if !text.is_empty()&&on_chunk.send(text).is_err(){break;}},Err(_)=>{let _=on_chunk.send(String::new());break;}},_=>{let _=on_chunk.send(String::new());break;}}}}
        }
        state.shared.lock().unwrap().subscriptions.remove(&task_id);
    });
    Ok(id)
}
#[tauri::command]
pub fn cloud_unsubscribe(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, CloudState>,
    id: String,
) {
    let mut s = state.shared.lock().unwrap();
    if s.subscriptions
        .get(&id)
        .is_some_and(|v| v.owner == window.label())
        && let Some(v) = s.subscriptions.remove(&id)
    {
        v.cancel.cancel();
    }
}
#[derive(Default)]
struct Utf8Chunks {
    pending: Vec<u8>,
}
impl Utf8Chunks {
    fn push(&mut self, bytes: &[u8]) -> Result<String, ()> {
        self.pending.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(v) => v.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => return Err(()),
        };
        let text = std::str::from_utf8(&self.pending[..valid])
            .map_err(|_| ())?
            .to_owned();
        self.pending.drain(..valid);
        Ok(text)
    }
}
#[tauri::command]
pub async fn cloud_save_download(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, CloudState>,
    path: String,
) -> Result<SaveResult, String> {
    let ctx = state.context(&path, false)?;
    let preview = request(&ctx, Method::GET)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|_| "下载检查失败")?;
    if !preview.status().is_success() {
        return Err("文件不可访问，请检查权限并重试".into());
    }
    let file_name = filename(&preview).unwrap_or_else(|| "download".into());
    drop(preview);
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("保存云端文件")
        .set_file_name(&file_name)
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(destination) = rx.await.map_err(|_| "保存窗口已关闭")? else {
        return Ok(SaveResult { saved: false });
    };
    let destination = destination.into_path().map_err(|_| "保存位置无效")?;
    let current = state.context(&path, false)?;
    if current.generation != ctx.generation {
        return Err("会话已变化，请重新保存".into());
    }
    let response = request(&current, Method::GET)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|_| "下载失败")?;
    if !response.status().is_success() {
        return Err("文件权限或内容已变化，请重试".into());
    }
    let bytes = response.bytes().await.map_err(|_| "下载内容不完整")?;
    {
        let s = state.shared.lock().unwrap();
        if s.generation != ctx.generation {
            return Err("会话已变化，请重新保存".into());
        }
    }
    persist_download(destination, bytes.to_vec()).await?;
    Ok(SaveResult { saved: true })
}

/// Save already decrypted client content; never sends it back to the cloud.
#[tauri::command]
pub async fn cloud_save_blob(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, CloudState>,
    name: String,
    data: String,
) -> Result<SaveResult, String> {
    let (_, _, generation) = state.authorized_local_session()?;
    if name.is_empty()
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
    {
        return Err("文件名无效".into());
    }
    let bytes = STANDARD.decode(data).map_err(|_| "保存内容格式无效")?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("保存文件")
        .set_file_name(&name)
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(destination) = rx.await.map_err(|_| "保存窗口已关闭")? else {
        return Ok(SaveResult { saved: false });
    };
    let destination = destination.into_path().map_err(|_| "保存位置无效")?;
    if state.session_generation() != generation {
        return Err("会话已变化，请重新保存".into());
    }
    persist_download(destination, bytes).await?;
    Ok(SaveResult { saved: true })
}

async fn persist_download(destination: std::path::PathBuf, bytes: Vec<u8>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let mut tmp = tempfile::NamedTempFile::new_in(destination.parent().ok_or("保存位置无效")?)
            .map_err(|_| "无法创建临时文件")?;
        tmp.write_all(&bytes).map_err(|_| "文件写入失败")?;
        tmp.as_file().sync_all().map_err(|_| "文件保存失败")?;
        tmp.persist(destination).map_err(|_| "无法替换目标文件")?;
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "文件保存任务失败")?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remember_login_requires_explicit_supported_auth_request() {
        for body in [
            serde_json::json!({}),
            serde_json::json!({"rememberLogin":false}),
            serde_json::json!({"rememberLogin":"true"}),
        ] {
            assert!(!remember_login_requested("/api/auth/login", Some(&body)));
        }
        assert!(!remember_login_requested("/api/auth/verify", None));
        let body = serde_json::json!({"rememberLogin":true});
        assert_eq!(
            remember_login_requested("/api/auth/login", Some(&body)),
            secure_credentials::supported()
        );
        assert_eq!(
            remember_login_requested("/api/auth/verify", Some(&body)),
            secure_credentials::supported()
        );
        assert!(!remember_login_requested("/api/me", Some(&body)));
    }
    #[cfg(windows)]
    #[test]
    fn old_server_persistent_cookie_does_not_imply_remember_consent() {
        for remember in [false, true] {
            let state = CloudState::new();
            let server = "https://example.test";
            state.shared.lock().unwrap().server = Some(server_url(server).unwrap());
            let ctx = state.context("/api/auth/login", true).unwrap();
            let response = reqwest::Response::from(
                tauri::http::Response::builder()
                    .header(
                        header::SET_COOKIE,
                        "nest_session=test-token; HttpOnly; Max-Age=2592000; Path=/",
                    )
                    .body("")
                    .unwrap(),
            );
            let generation = state.cookies(&ctx, &response, remember).unwrap().unwrap();
            state
                .confirm_account(
                    generation,
                    &serde_json::json!({"id":Uuid::new_v4(),"kind":"human"}),
                )
                .unwrap();
            let s = state.shared.lock().unwrap();
            assert!(s.cookie.is_some());
            assert_eq!(s.vault.load_session(server).unwrap().is_some(), remember);
        }
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn cleanup_failure_keeps_401_and_emits_scoped_auth_lost_for_request_and_logout() {
        use std::os::windows::fs::OpenOptionsExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for logout in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let server = format!("http://{}", listener.local_addr().unwrap());
            let state = CloudState::new();
            let owner = Uuid::new_v4();
            let device = Uuid::new_v4();
            let session_file = {
                let mut s = state.shared.lock().unwrap();
                s.server = Some(server_url(&server).unwrap());
                s.cookie = Some("nest_session=test-token".into());
                s.local_human = Some(owner);
                s.credential_owner = Some(owner);
                s.vault
                    .save_session(
                        &server,
                        &Session {
                            account: owner,
                            cookie: "nest_session=test-token".into(),
                            expires_at: secure_credentials::now_ms()
                                + secure_credentials::THIRTY_DAYS_MS,
                        },
                    )
                    .unwrap();
                s.vault
                    .save_unlock(&server, owner, device, "test-secret".into())
                    .unwrap();
                s.vault.test_session_path(&server)
            };
            let file_lock = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(session_file)
                .unwrap();
            let serving = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut input = [0; 2048];
                let mut headers = Vec::new();
                loop {
                    let count = stream.read(&mut input).await.unwrap();
                    assert!(count > 0);
                    headers.extend_from_slice(&input[..count]);
                    if headers.windows(4).any(|v| v == b"\r\n\r\n") {
                        break;
                    }
                }
                let response = if logout {
                    "HTTP/1.1 204 No Content\r\nSet-Cookie: nest_session=; Max-Age=0; Path=/\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                stream.write_all(response.as_bytes()).await.unwrap();
            });
            let response = cloud_request_inner(
                &state,
                if logout { "POST" } else { "GET" }.into(),
                if logout {
                    "/api/auth/logout"
                } else {
                    "/api/me"
                }
                .into(),
                None,
            )
            .await
            .unwrap();
            serving.await.unwrap();
            assert_eq!(response.status, 401);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&response.body).unwrap()["code"],
                "native_credential_cleanup_failed"
            );
            let s = state.shared.lock().unwrap();
            assert!(s.cookie.is_none() && s.local_human.is_none());
            assert!(
                s.vault
                    .load_unlock(&server, owner, device)
                    .unwrap()
                    .is_none()
            );
            let event = s.emitted_auth_lost.last().unwrap();
            assert_eq!(event.server, server);
            assert_eq!(event.account_id, Some(owner));
            assert!(event.message.is_some());
            drop(file_lock);
            s.vault.clear(&server, Some(owner)).unwrap();
        }
    }
    #[test]
    fn unlock_requires_current_confirmed_human_and_uuid() {
        let state = CloudState::new();
        let mut s = state.shared.lock().unwrap();
        assert!(unlock_scope(&mut s).is_err());
        s.server = Some(server_url("https://example.test").unwrap());
        s.credential_owner = Some(Uuid::new_v4());
        // A saved credential alone does not authorize releasing the SDK secret.
        assert!(unlock_scope(&mut s).is_err());
        let human = Uuid::new_v4();
        s.local_human = Some(human);
        assert_eq!(
            unlock_scope(&mut s).unwrap(),
            ("https://example.test".into(), human)
        );
        assert!(expected_unlock_scope(&mut s, "https://other.test", &human.to_string()).is_err());
        assert!(
            expected_unlock_scope(&mut s, "https://example.test", &Uuid::new_v4().to_string())
                .is_err()
        );
        assert!(expected_unlock_scope(&mut s, "https://example.test/", &human.to_string()).is_ok());
        assert!(parse_device("../device").is_err());
        assert!(parse_device(&Uuid::nil().to_string()).is_err());
        assert!(parse_device(&Uuid::new_v4().to_string()).is_ok());
    }
    #[cfg(windows)]
    #[test]
    fn persisted_session_keeps_deadline_and_401_clears_only_current_account() {
        let state = CloudState::new();
        let human = Uuid::new_v4();
        let other = Uuid::new_v4();
        let device = Uuid::new_v4();
        let server = "https://example.test";
        {
            let mut s = state.shared.lock().unwrap();
            s.server = Some(server_url(server).unwrap());
            s.cookie = Some("nest_session=test-cookie".into());
            s.persistent_until =
                Some(secure_credentials::now_ms() + secure_credentials::THIRTY_DAYS_MS);
            s.vault
                .save_unlock(server, human, device, "test-one".into())
                .unwrap();
            s.vault
                .save_unlock(server, other, device, "test-two".into())
                .unwrap();
        }
        let account = serde_json::json!({"id":human,"kind":"human"});
        state.confirm_account(0, &account).unwrap();
        let deadline = state
            .shared
            .lock()
            .unwrap()
            .vault
            .load_session(server)
            .unwrap()
            .unwrap()
            .expires_at;
        state.confirm_account(0, &account).unwrap();
        assert_eq!(
            state
                .shared
                .lock()
                .unwrap()
                .vault
                .load_session(server)
                .unwrap()
                .unwrap()
                .expires_at,
            deadline
        );
        let ctx = state.context("/api/me", false).unwrap();
        state
            .reject_session(&ctx, reqwest::StatusCode::SERVICE_UNAVAILABLE)
            .unwrap();
        assert!(
            state
                .shared
                .lock()
                .unwrap()
                .vault
                .load_session(server)
                .unwrap()
                .is_some()
        );
        state
            .reject_session(&ctx, reqwest::StatusCode::UNAUTHORIZED)
            .unwrap();
        let s = state.shared.lock().unwrap();
        assert!(s.vault.load_session(server).unwrap().is_none());
        assert!(
            s.vault
                .load_unlock(server, human, device)
                .unwrap()
                .is_none()
        );
        assert!(
            s.vault
                .load_unlock(server, other, device)
                .unwrap()
                .is_some()
        );
    }
    #[cfg(windows)]
    #[test]
    fn expiry_revokes_local_unlock_authority_and_clears_material() {
        let state = CloudState::new();
        let human = Uuid::new_v4();
        let device = Uuid::new_v4();
        let server = "https://example.test";
        {
            let mut s = state.shared.lock().unwrap();
            s.server = Some(server_url(server).unwrap());
            s.local_human = Some(human);
            s.credential_owner = Some(human);
            s.persistent_until = Some(secure_credentials::now_ms() - 1);
            s.vault
                .save_unlock(server, human, device, "test-one".into())
                .unwrap();
        }
        assert!(state.authorized_local_session().is_err());
        let mut s = state.shared.lock().unwrap();
        assert!(unlock_scope(&mut s).is_err());
        assert!(
            s.vault
                .load_unlock(server, human, device)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn local_authority_survives_connection_errors_but_not_session_change() {
        let state = CloudState::new();
        let human = Uuid::new_v4();
        {
            let mut s = state.shared.lock().unwrap();
            s.server = Some(server_url("https://example.com").unwrap());
            s.cookie = Some("nest_session=private".into());
            s.local_human = Some(human);
        }
        assert_eq!(state.authorized_local_identity().unwrap().1, human);
        let ctx = state.context("/api/me", false).unwrap();
        state
            .reject_session(&ctx, reqwest::StatusCode::SERVICE_UNAVAILABLE)
            .unwrap();
        assert_eq!(state.authorized_local_identity().unwrap().1, human);
        state
            .reject_session(&ctx, reqwest::StatusCode::UNAUTHORIZED)
            .unwrap();
        assert!(state.authorized_local_identity().is_err());
        assert!(CloudState::new().authorized_local_identity().is_err());
    }
    #[tokio::test]
    #[ignore = "requires explicit CLOUD_TLS_TEST_URL and CLOUD_CA_CERT_FILE"]
    async fn connects_to_explicit_test_https_with_configured_ca() {
        let url = std::env::var("CLOUD_TLS_TEST_URL").expect("explicit test HTTPS URL required");
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert!(
            parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.username().is_empty()
                && parsed.password().is_none(),
            "TLS probe must not contain credentials or query parameters"
        );
        let client = CloudState::client().unwrap();
        let response = client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await;
        if let Err(error) = &response {
            // TLS diagnostic only: strip the URL and never use credentials in this probe.
            use std::error::Error;
            let mut cause = error.source();
            while let Some(next) = cause {
                eprintln!("TLS probe cause: {next}");
                cause = next.source();
            }
        }
        assert_eq!(
            response.map_err(connection_error).unwrap().status(),
            reqwest::StatusCode::OK
        );
    }
    #[test]
    fn supports_put_for_read_and_pin_commands() {
        assert_eq!(request_method("PUT").unwrap(), Method::PUT);
        assert!(request_method("CONNECT").is_err());
    }
    #[test]
    fn connection_errors_do_not_expose_request_secrets() {
        let error = Client::new()
            .get("https://[invalid/?token=private-token")
            .build()
            .unwrap_err();
        let message = connection_error(error);
        assert!(!message.contains("private-token"));
        assert!(!message.contains("https://"));
    }
    #[test]
    fn restricts_origins_and_normalized_paths() {
        assert!(server_url("http://127.0.0.1:8080").is_ok());
        assert!(server_url("http://example.com").is_err());
        assert!(server_url("https://user:secret@example.com").is_err());
        let s = server_url("https://example.com").unwrap();
        for p in [
            "https://evil.test/api/x",
            "//evil.test/api/x",
            "/api/../secret",
            "/api/%2e%2e/secret",
            "/api/x#fragment",
            "/api\\foo",
        ] {
            assert!(target(&s, p).is_err(), "{p}");
        }
        assert!(target(&s, "/api/events?after=2").is_ok());
    }
    #[test]
    fn sse_decodes_split_unicode_without_replacement() {
        let mut d = Utf8Chunks::default();
        let bytes = "data: 中文🐈\n\n".as_bytes();
        let mut text = String::new();
        for b in bytes {
            text.push_str(&d.push(&[*b]).unwrap());
        }
        assert_eq!(text, "data: 中文🐈\n\n");
        assert!(d.pending.is_empty());
        assert!(d.push(&[255]).is_err());
    }
    #[test]
    fn keeps_cookie_native_and_discards_late_auth_responses() {
        let state = CloudState {
            shared: Arc::new(Mutex::new(Inner {
                client: Ok(Client::new()),
                server: Some(server_url("https://example.com").unwrap()),
                cookie: Some("nest_session=old".into()),
                generation: 0,
                local_human: None,
                subscriptions: HashMap::new(),
                owner_versions: HashMap::new(),
                vault: Vault::new(),
                credential_owner: None,
                persistent_until: None,
                auth_events: None,
                emitted_auth_lost: Vec::new(),
            })),
        };
        let older = state.context("/api/auth/login", true).unwrap();
        let newer = state.context("/api/auth/login", true).unwrap();
        let built = request(&newer, Method::GET).build().unwrap();
        assert_eq!(built.headers()["x-llmn-client"], "1");
        assert_eq!(built.headers()[header::COOKIE], "nest_session=old");
        let response = |token: &str| {
            reqwest::Response::from(
                tauri::http::Response::builder()
                    .header(
                        header::SET_COOKIE,
                        format!("nest_session={token}; HttpOnly; Path=/"),
                    )
                    .body("")
                    .unwrap(),
            )
        };
        assert!(
            state
                .cookies(&newer, &response("new"), false)
                .unwrap()
                .is_some()
        );
        assert!(
            state
                .cookies(&older, &response("stale"), false)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            state.shared.lock().unwrap().cookie.as_deref(),
            Some("nest_session=new")
        );
    }
}
