use super::*;
use futures_util::{SinkExt, StreamExt};
use reqwest::{
    Method,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("invalid node endpoint: {0}")]
    Endpoint(String),
    #[error("invalid node credential")]
    Credential,
    #[error("TLS configuration: {0}")]
    Tls(#[from] native_tls::Error),
    #[error("node HTTP transport: {0}")]
    Http(#[from] reqwest::Error),
    #[error("node API HTTP {status}: {body}")]
    Api { status: u16, body: String },
    #[error("node WebSocket: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("node image attachment exceeds 4 MiB")]
    AttachmentTooLarge,
    #[error("node protocol JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl NodeError {
    pub fn is_queue_wait(&self) -> bool {
        match self {
            NodeError::Api { status: 409, body } => serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|body| {
                    body.get("code")
                        .and_then(|code| code.as_str())
                        .map(|code| matches!(code, "conversation_busy" | "task_not_ready"))
                })
                .unwrap_or(false),
            _ => false,
        }
    }
}

/// Network client only; it never retries model work or stores credentials.
#[derive(Clone)]
pub struct NodeClient {
    base: Url,
    http: reqwest::Client,
    authorization: HeaderValue,
    tls: native_tls::TlsConnector,
}
impl NodeClient {
    /// `base_url` is the server origin (optionally with a deployment prefix), not `/api`.
    /// An explicit CA augments system trust for both HTTPS and WSS.
    pub fn new(base_url: &str, bearer: &str, ca_pem: Option<&[u8]>) -> Result<Self, NodeError> {
        let mut base = Url::parse(base_url).map_err(|e| NodeError::Endpoint(e.to_string()))?;
        let loopback = match base.host() {
            Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        if !(base.scheme() == "https" || base.scheme() == "http" && loopback)
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(NodeError::Endpoint("HTTPS required (HTTP allowed only on loopback); URL credentials/query/fragment forbidden".into()));
        }
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {bearer}"))
            .map_err(|_| NodeError::Credential)?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization.clone());
        headers.insert("x-llmn-client", HeaderValue::from_static("1"));
        let mut http = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30));
        let mut tls = native_tls::TlsConnector::builder();
        if let Some(pem) = ca_pem {
            http = http.add_root_certificate(reqwest::Certificate::from_pem(pem)?);
            tls.add_root_certificate(native_tls::Certificate::from_pem(pem)?);
        }
        Ok(Self {
            base,
            http: http.build()?,
            authorization,
            tls: tls.build()?,
        })
    }
    fn endpoint(&self, parts: &[&str]) -> Url {
        let mut url = self.base.clone();
        {
            let mut path = url.path_segments_mut().expect("validated HTTP URL");
            path.pop_if_empty().push("api");
            for part in parts {
                path.push(part);
            }
        }
        url
    }
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        parts: &[&str],
        body: Option<&impl Serialize>,
    ) -> Result<T, NodeError> {
        let mut request = self.http.request(method, self.endpoint(parts));
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(NodeError::Api {
                status: response.status().as_u16(),
                body: response.text().await?,
            });
        }
        Ok(response.json().await?)
    }
    pub async fn direct_prepare(
        &self,
        request: &crate::direct::DirectPrepare,
    ) -> Result<crate::direct::DirectBootstrap, NodeError> {
        self.request(Method::POST, &["node", "direct", "prepare"], Some(request))
            .await
    }
    pub async fn direct_sync(
        &self,
        request: &crate::direct::DirectSyncRequest,
    ) -> Result<crate::direct::DirectSyncReceipt, NodeError> {
        self.request(Method::POST, &["node", "direct", "sync"], Some(request))
            .await
    }
    pub async fn changes(&self, after: &str) -> Result<ConsciousnessChanges, NodeError> {
        let mut url = self.endpoint(&["node", "consciousness", "changes"]);
        url.query_pairs_mut().append_pair("after", after);
        let response = self.http.get(url).send().await?;
        if !response.status().is_success() {
            return Err(NodeError::Api {
                status: response.status().as_u16(),
                body: response.text().await?,
            });
        }
        Ok(response.json().await?)
    }
    pub async fn history(&self, cid: &str, after: &str) -> Result<NodeHistory, NodeError> {
        let mut url = self.endpoint(&["node", "conversations", cid, "history"]);
        url.query_pairs_mut()
            .append_pair("after", after)
            .append_pair("limit", "100");
        let response = self.http.get(url).send().await?;
        if !response.status().is_success() {
            return Err(NodeError::Api {
                status: response.status().as_u16(),
                body: response.text().await?,
            });
        }
        Ok(response.json().await?)
    }
    pub async fn validate(&self, body: &ValidateSources) -> Result<ValidationResult, NodeError> {
        self.request(
            Method::POST,
            &["node", "consciousness", "validate"],
            Some(body),
        )
        .await
    }
    pub async fn announce(&self, body: &MaterialScope) -> Result<serde_json::Value, NodeError> {
        self.request(
            Method::POST,
            &["node", "consciousness", "state"],
            Some(body),
        )
        .await
    }
    pub async fn summary_start(
        &self,
        body: &SummaryStart,
    ) -> Result<SummaryConfirmation, NodeError> {
        self.request(Method::POST, &["node", "summaries", "start"], Some(body))
            .await
    }
    pub async fn summary_finish(
        &self,
        id: &str,
        body: &SummaryFinish,
    ) -> Result<serde_json::Value, NodeError> {
        self.request(
            Method::POST,
            &["node", "summaries", id, "finish"],
            Some(body),
        )
        .await
    }
    pub async fn pair(&self, body: &PairingRequest) -> Result<PairingCreated, NodeError> {
        self.request(Method::POST, &["node", "pairings"], Some(body))
            .await
    }
    pub async fn pairing_status(&self, id: &str) -> Result<PairingStatus, NodeError> {
        self.request(Method::GET, &["node", "pairings", id], None::<&()>)
            .await
    }
    pub async fn tasks(&self) -> Result<Vec<NodeTask>, NodeError> {
        self.request(Method::GET, &["node", "tasks"], None::<&()>)
            .await
    }
    pub async fn task(&self, id: &str) -> Result<NodeTask, NodeError> {
        self.request(Method::GET, &["node", "tasks", id], None::<&()>)
            .await
    }
    pub async fn materials(&self, id: &str) -> Result<TaskMaterials, NodeError> {
        self.request(
            Method::GET,
            &["node", "tasks", id, "materials"],
            None::<&()>,
        )
        .await
    }
    pub async fn start(&self, id: &str, body: &StartRequest) -> Result<StartReceipt, NodeError> {
        self.request(Method::POST, &["node", "tasks", id, "start"], Some(body))
            .await
    }
    pub async fn result(&self, id: &str, body: &ResultRequest) -> Result<NodeTask, NodeError> {
        self.request(Method::POST, &["node", "tasks", id, "result"], Some(body))
            .await
    }
    pub async fn reject(&self, id: &str, body: &RejectRequest) -> Result<NodeTask, NodeError> {
        self.request(Method::POST, &["node", "tasks", id, "reject"], Some(body))
            .await
    }
    pub async fn attachment(
        &self,
        task_id: &str,
        message_id: &str,
        index: usize,
        revision: &str,
    ) -> Result<Vec<u8>, NodeError> {
        let mut url = self.endpoint(&[
            "node",
            "tasks",
            task_id,
            "messages",
            message_id,
            "attachments",
            &index.to_string(),
        ]);
        url.query_pairs_mut().append_pair("revision", revision);
        let response = self.http.get(url).send().await?;
        if !response.status().is_success() {
            return Err(NodeError::Api {
                status: response.status().as_u16(),
                body: response.text().await?,
            });
        }
        const IMAGE_LIMIT: usize = 4 * 1024 * 1024;
        if response
            .content_length()
            .is_some_and(|length| length > IMAGE_LIMIT as u64)
        {
            return Err(NodeError::AttachmentTooLarge);
        }
        let mut image = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk?;
            if chunk.len() > IMAGE_LIMIT - image.len() {
                return Err(NodeError::AttachmentTooLarge);
            }
            image.extend_from_slice(&chunk);
        }
        Ok(image)
    }
    /// Each new connection requires Hello, Ready, then an authoritative `tasks()` fetch.
    pub async fn connect(&self) -> Result<NodeConnection, NodeError> {
        let mut url = self.endpoint(&["node", "ws"]);
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).expect("HTTP to WS scheme");
        let mut request = url.as_str().into_client_request()?;
        request
            .headers_mut()
            .insert(AUTHORIZATION, self.authorization.clone());
        request
            .headers_mut()
            .insert("x-llmn-client", HeaderValue::from_static("1"));
        let config = WebSocketConfig::default()
            .max_message_size(Some(1024 * 1024))
            .max_frame_size(Some(1024 * 1024));
        let (socket, _) = connect_async_tls_with_config(
            request,
            Some(config),
            false,
            Some(Connector::NativeTls(self.tls.clone())),
        )
        .await?;
        Ok(NodeConnection { socket })
    }
}
pub struct NodeConnection {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}
impl NodeConnection {
    pub async fn send(&mut self, frame: &ClientFrame) -> Result<(), NodeError> {
        self.socket
            .send(Message::Text(serde_json::to_string(frame)?.into()))
            .await?;
        Ok(())
    }
    pub async fn recv(&mut self) -> Result<Option<ServerFrame>, NodeError> {
        while let Some(message) = self.socket.next().await {
            match message? {
                Message::Text(text) => return Ok(Some(serde_json::from_str(&text)?)),
                Message::Ping(_) => self.socket.flush().await?,
                Message::Close(_) => return Ok(None),
                Message::Binary(_) => {
                    return Err(NodeError::Endpoint(
                        "expected JSON text WebSocket frame".into(),
                    ));
                }
                _ => {}
            }
        }
        Ok(None)
    }
    pub async fn close(&mut self) -> Result<(), NodeError> {
        self.socket.close(None).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn http_auth_and_original_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 8192];
            let n = stream.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]).to_lowercase();
            assert!(request.starts_with("get /api/node/tasks "));
            assert!(request.contains("authorization: bearer private-secret"));
            assert!(request.contains("x-llmn-client: 1"));
            let body = r#"{"error":"binding_revoked","message":"replace node"}"#;
            stream.write_all(format!("HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
        });
        let error = NodeClient::new(&origin, "private-secret", None)
            .unwrap()
            .tasks()
            .await
            .unwrap_err();
        assert!(
            matches!(error, NodeError::Api { status: 403, ref body } if body.contains("binding_revoked"))
        );
        server.await.unwrap();
    }

    #[tokio::test]
    #[allow(clippy::result_large_err)] // tungstenite handshake callback requires its response error type.
    async fn websocket_hello_ready_and_reconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_hdr_async(
                    stream,
                    |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                     response| {
                        assert_eq!(request.uri().path(), "/api/node/ws");
                        assert_eq!(request.headers()["authorization"], "Bearer private-secret");
                        Ok(response)
                    },
                )
                .await
                .unwrap();
                let text = ws.next().await.unwrap().unwrap().into_text().unwrap();
                assert!(matches!(
                    serde_json::from_str::<ClientFrame>(&text).unwrap(),
                    ClientFrame::Hello {
                        protocol_version: 4,
                        ..
                    }
                ));
                ws.send(Message::Text(
                    serde_json::to_string(&ServerFrame::Ready {
                        protocol_version: 4,
                    })
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
                ws.close(None).await.unwrap();
            }
        });
        let client = NodeClient::new(&origin, "private-secret", None).unwrap();
        for _ in 0..2 {
            let mut connection = client.connect().await.unwrap();
            connection
                .send(&ClientFrame::Hello {
                    capabilities: vec![],
                    protocol_version: 4,
                    node_id: "n".into(),
                    ai_id: "a".into(),
                    binding_version: "1".into(),
                })
                .await
                .unwrap();
            assert_eq!(
                connection.recv().await.unwrap(),
                Some(ServerFrame::Ready {
                    protocol_version: 4
                })
            );
            assert_eq!(connection.recv().await.unwrap(), None);
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn image_download_limits_declared_and_streamed_size() {
        const LIMIT: usize = 4 * 1024 * 1024;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for case in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 8192];
                let count = stream.read(&mut request).await.unwrap();
                assert!(String::from_utf8_lossy(&request[..count]).starts_with(
                    "GET /api/node/tasks/task/messages/message/attachments/0?revision=1 "
                ));
                match case {
                    0 => {
                        // Reject the declared size immediately, without waiting for a body.
                        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", LIMIT + 1).as_bytes()).await.unwrap();
                    }
                    1 => {
                        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {LIMIT}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                        stream.write_all(&vec![7; LIMIT]).await.unwrap();
                    }
                    _ => {
                        stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
                        let chunk = vec![7; 64 * 1024];
                        for _ in 0..64 {
                            stream.write_all(b"10000\r\n").await.unwrap();
                            stream.write_all(&chunk).await.unwrap();
                            stream.write_all(b"\r\n").await.unwrap();
                        }
                        stream.write_all(b"1\r\nx\r\n0\r\n\r\n").await.unwrap();
                    }
                }
            }
        });
        let client = NodeClient::new(&origin, "private-secret", None).unwrap();
        assert!(matches!(
            client.attachment("task", "message", 0, "1").await,
            Err(NodeError::AttachmentTooLarge)
        ));
        let image = client.attachment("task", "message", 0, "1").await.unwrap();
        assert_eq!(image.len(), LIMIT);
        assert!(image.iter().all(|byte| *byte == 7));
        assert!(matches!(
            client.attachment("task", "message", 0, "1").await,
            Err(NodeError::AttachmentTooLarge)
        ));
        server.await.unwrap();
    }
}
