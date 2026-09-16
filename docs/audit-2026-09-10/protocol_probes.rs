//! Audit probes: assert the observed defects, not the desired behavior.
//! Temporarily copy into crates/ai-client/tests/audit_probe.rs and run
//! cargo test -p ai-client --test audit_probe -- --nocapture
//! Fixtures use a local HTTP listener; no provider credentials are used.
use ai_client::{ChatChunk, ChatRequest, ModelRouter, ModelSelection, ProviderConfig, ProviderId};
use ai_client::protocols::{openai, openai_responses, sse::SseDataStream};
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
        let header = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", len);
        socket.write_all(header.as_bytes()).await.unwrap();
        for chunk in chunks {
            socket.write_all(&chunk).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        }
    });
    reqwest::Client::builder().no_proxy().build().unwrap()
        .get(format!("http://{addr}")).send().await.unwrap()
}

#[tokio::test]
async fn audit_openai_drops_second_tool_and_done() {
    let body = concat!(
        "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"add\",\"arguments\":\"{}\"}},{\"index\":1,\"id\":\"b\",\"function\":{\"name\":\"echo\",\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    // Separate network writes avoid the additional buffered-frame defect,
    // isolating the finished-before-queue-drain defect.
    let frames = body.split_inclusive("\n\n").map(|s| s.as_bytes().to_vec()).collect();
    let chunks: Vec<_> = openai::sse::OpenAIStream::new(fixture_chunks(frames).await).collect().await;
    println!("two requested tools: {chunks:?}");
    assert_eq!(chunks.len(), 1);
    assert!(matches!(&chunks[0], Ok(ChatChunk::ToolCall { id, .. }) if id == "a"));
}

#[tokio::test]
async fn audit_openai_loses_usage_after_finish_reason() {
    let body = concat!(
        "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"id\":\"fixture\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5,\"total_tokens\":105}}\n\n",
        "data: [DONE]\n\n"
    );
    let chunks: Vec<_> = openai::sse::OpenAIStream::new(fixture(body.into()).await).collect().await;
    println!("usage tail: {chunks:?}");
    assert!(matches!(&chunks[0], Ok(ChatChunk::Done { usage: None })));
}

#[tokio::test]
async fn audit_openai_rejects_crlf_framing() {
    let body = "data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n";
    let chunks: Vec<_> = openai::sse::OpenAIStream::new(fixture(body.into()).await).collect().await;
    println!("CRLF frames: {chunks:?}");
    assert!(chunks.is_empty());
}

#[tokio::test]
async fn audit_shared_sse_drops_buffer_after_comment() {
    // One HTTP body read normally contains both complete frames.
    let chunks: Vec<_> = SseDataStream::new(fixture(": ping\n\ndata: hello\n\n".into()).await).collect().await;
    println!("comment followed by data in the same buffer: {chunks:?}");
    assert!(chunks.is_empty());
}

#[tokio::test]
async fn audit_shared_sse_corrupts_split_unicode() {
    let bytes = "data: 你\n\n".as_bytes();
    let response = fixture_chunks(vec![bytes[..7].to_vec(), bytes[7..].to_vec()]).await;
    let chunks: Vec<_> = SseDataStream::new(response).collect().await;
    let text = chunks[0].as_ref().unwrap();
    println!("split Unicode: {text:?}");
    assert_ne!(text, "你");
    assert!(text.contains('\u{fffd}'));
}

#[test]
fn audit_alias_is_not_normalized_before_wire_conversion() {
    let cfg: ProviderConfig = serde_json::from_value(serde_json::json!({
        "protocol":"openai", "base_url":"http://localhost", "api_key":"fake",
        "models":{"alias":{"model":"actual-wire-model"}}
    })).unwrap();
    let router = ModelRouter::new(&std::collections::HashMap::from([(ProviderId::new("custom"), cfg)])).unwrap();
    let selection = ModelSelection { provider:"custom".into(), model:"alias".into(), reasoning_effort:None };
    let resolved = router.resolve(&selection).unwrap();
    assert_eq!(resolved.spec.wire, "actual-wire-model");
    let req = ChatRequest { selection, resolved:Some(resolved), messages:vec![], options:Default::default(), tools:vec![] };
    let wire = openai::convert::to_real_request(&req);
    println!("resolved wire=actual-wire-model, sent model={}", wire.model);
    assert_eq!(wire.model, "alias");
}

#[test]
fn audit_responses_failure_becomes_done() {
    let event = openai_responses::convert::parse_event("{\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"upstream failure\"}}}").unwrap();
    assert!(matches!(event, Some(openai_responses::convert::StreamEvent::Done { .. })));
    println!("response.failed was mapped to Done");
}
