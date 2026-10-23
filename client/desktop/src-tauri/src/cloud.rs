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
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct CloudState {
    shared: Arc<Mutex<Inner>>,
}
struct Inner {
    client: Result<Client, String>,
    server: Option<Url>,
    cookie: Option<String>,
    generation: u64,
    subscriptions: HashMap<String, Subscription>,
    owner_versions: HashMap<String, u64>,
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
                subscriptions: HashMap::new(),
                owner_versions: HashMap::new(),
            })),
        }
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
        self.reject_session(&ctx, response.status());
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
    fn cookies(&self, ctx: &RequestContext, response: &reqwest::Response) {
        let mut s = self.shared.lock().unwrap();
        if s.generation != ctx.generation {
            return;
        }
        for h in response.headers().get_all(header::SET_COOKIE) {
            if let Ok(v) = h.to_str()
                && let Some(value) = v
                    .split(';')
                    .next()
                    .and_then(|p| p.strip_prefix("nest_session="))
            {
                let cookie = if value.is_empty() {
                    None
                } else {
                    Some(format!("nest_session={value}"))
                };
                if s.cookie != cookie {
                    advance(&mut s);
                    s.cookie = cookie;
                }
            }
        }
    }
    fn reject_session(&self, ctx: &RequestContext, status: reqwest::StatusCode) {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            let mut s = self.shared.lock().unwrap();
            if s.generation == ctx.generation && s.cookie.is_some() {
                advance(&mut s);
                s.cookie = None;
            }
        }
    }
}
fn advance(s: &mut Inner) {
    s.generation += 1;
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
pub fn cloud_set_server(state: tauri::State<'_, CloudState>, url: String) -> Result<(), String> {
    let server = server_url(&url)?;
    let mut s = state.shared.lock().unwrap();
    if s.server.as_ref() == Some(&server) {
        return Ok(());
    }
    advance(&mut s);
    s.server = Some(server);
    s.cookie = None;
    Ok(())
}
#[tauri::command]
pub async fn cloud_request(
    state: tauri::State<'_, CloudState>,
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
    let ctx = state.context(&path, auth_change)?;
    let mut b = request(&ctx, method).timeout(Duration::from_secs(60));
    if let Some(body) = body {
        b = b.json(&body);
    }
    let response = b.send().await.map_err(connection_error)?;
    state.cookies(&ctx, &response);
    state.reject_session(&ctx, response.status());
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
    if path == "/api/auth/reset" && status < 300 {
        let mut s = state.shared.lock().unwrap();
        if s.generation == ctx.generation {
            advance(&mut s);
            s.cookie = None;
        }
    }
    let body = if binary {
        STANDARD.encode(bytes)
    } else {
        String::from_utf8(bytes.to_vec()).map_err(|_| "云端文本编码无效")?
    };
    Ok(CloudResponse {
        status,
        body,
        content_type,
        file_name,
        binary,
    })
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
        state.reject_session(&ctx, response.status());
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
    tokio::task::spawn_blocking(move || {
        let mut tmp = tempfile::NamedTempFile::new_in(destination.parent().ok_or("保存位置无效")?)
            .map_err(|_| "无法创建临时文件")?;
        tmp.write_all(&bytes).map_err(|_| "文件写入失败")?;
        tmp.as_file().sync_all().map_err(|_| "文件保存失败")?;
        tmp.persist(destination).map_err(|_| "无法替换目标文件")?;
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "文件保存任务失败")??;
    Ok(SaveResult { saved: true })
}
#[cfg(test)]
mod tests {
    use super::*;
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
                subscriptions: HashMap::new(),
                owner_versions: HashMap::new(),
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
        state.cookies(&newer, &response("new"));
        state.cookies(&older, &response("stale"));
        assert_eq!(
            state.shared.lock().unwrap().cookie.as_deref(),
            Some("nest_session=new")
        );
    }
}
