use ai_client::protocols::{openai, openai_responses, sse::SseDataStream};
use ai_client::{ChatChunk, ChatRequest, ModelRouter, ModelSelection, ProviderConfig, ProviderId};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn fixture(body: String) -> reqwest::Response {
    fixture_chunks(vec![body.into_bytes()]).await
}

async fn fixture_chunks(chunks: Vec<Vec<u8>>) -> reqwest::Response {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 8192];
        let _ = socket.read(&mut buf).await.unwrap();
        let len: usize = chunks.iter().map(Vec::len).sum();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            len
        );
        socket.write_all(header.as_bytes()).await.unwrap();
        for chunk in chunks {
            socket.write_all(&chunk).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        }
    });
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{addr}"))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn regression_openai_retains_all_tools_and_done() {
    let body = concat!(
        "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"add\",\"arguments\":\"{}\"}},{\"index\":1,\"id\":\"b\",\"function\":{\"name\":\"echo\",\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    // Separate network writes avoid the additional buffered-frame defect,
    // isolating the finished-before-queue-drain defect.
    let frames = body
        .split_inclusive("\n\n")
        .map(|s| s.as_bytes().to_vec())
        .collect();
    let chunks: Vec<_> = openai::sse::OpenAIStream::new(fixture_chunks(frames).await)
        .collect()
        .await;
    println!("two requested tools: {chunks:?}");
    assert_eq!(chunks.len(), 3);
    assert!(matches!(&chunks[1], Ok(ChatChunk::ToolCall { id, .. }) if id == "b"));
    assert!(matches!(&chunks[2], Ok(ChatChunk::Done { .. })));
    assert!(matches!(&chunks[0], Ok(ChatChunk::ToolCall { id, .. }) if id == "a"));
}

#[tokio::test]
async fn regression_openai_retains_usage_after_finish_reason() {
    let body = concat!(
        "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"id\":\"fixture\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5,\"total_tokens\":105}}\n\n",
        "data: [DONE]\n\n"
    );
    let chunks: Vec<_> = openai::sse::OpenAIStream::new(fixture(body.into()).await)
        .collect()
        .await;
    println!("usage tail: {chunks:?}");
    assert!(matches!(&chunks[0], Ok(ChatChunk::Done { usage: Some(u) }) if u.total_tokens == 105));
}

#[tokio::test]
async fn regression_openai_accepts_crlf_framing() {
    let body = "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n";
    let chunks: Vec<_> = openai::sse::OpenAIStream::new(fixture(body.into()).await)
        .collect()
        .await;
    println!("CRLF frames: {chunks:?}");
    assert!(matches!(&chunks[0], Ok(ChatChunk::Delta { content }) if content == "hello"));
    assert!(matches!(&chunks[1], Ok(ChatChunk::Done { .. })));
}

#[tokio::test]
async fn regression_shared_sse_drains_buffer_after_comment() {
    // One HTTP body read normally contains both complete frames.
    let chunks: Vec<_> = SseDataStream::new(fixture(": ping\n\ndata: hello\n\n".into()).await)
        .collect()
        .await;
    println!("comment followed by data in the same buffer: {chunks:?}");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref().unwrap(), "hello");
}

#[tokio::test]
async fn regression_shared_sse_preserves_split_unicode() {
    let bytes = "data: 你\n\n".as_bytes();
    let response = fixture_chunks(vec![bytes[..7].to_vec(), bytes[7..].to_vec()]).await;
    let chunks: Vec<_> = SseDataStream::new(response).collect().await;
    let text = chunks[0].as_ref().unwrap();
    println!("split Unicode: {text:?}");
    assert_eq!(text, "你");
}

#[test]
fn regression_alias_is_normalized_before_wire_conversion() {
    let cfg: ProviderConfig = serde_json::from_value(serde_json::json!({
        "protocol":"openai", "base_url":"http://localhost", "api_key":"fake",
        "models":{"alias":{"model":"actual-wire-model"}}
    }))
    .unwrap();
    let router = ModelRouter::new(&std::collections::HashMap::from([(
        ProviderId::new("custom"),
        cfg,
    )]))
    .unwrap();
    let selection = ModelSelection {
        provider: "custom".into(),
        model: "alias".into(),
        reasoning_effort: None,
    };
    let resolved = router.resolve(&selection).unwrap();
    assert_eq!(resolved.spec.wire, "actual-wire-model");
    let req = ChatRequest {
        selection,
        resolved: Some(resolved),
        messages: vec![],
        options: Default::default(),
        tools: vec![],
    };
    let wire = openai::convert::to_real_request(&req);
    println!("resolved wire=actual-wire-model, sent model={}", wire.model);
    assert_eq!(wire.model, "actual-wire-model");
}

#[test]
fn regression_responses_failure_is_error() {
    for kind in ["response.failed", "response.incomplete"] {
        let payload =
            serde_json::json!({"type":kind,"response":{"error":{"message":"upstream failure"}}});
        assert!(openai_responses::convert::parse_event(&payload.to_string()).is_err());
    }
}
