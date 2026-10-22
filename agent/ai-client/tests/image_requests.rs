use ai_client::{ChatRequest, ModelSelection, protocols};
use common::{ContentPart, GenerationOptions, Message, Role};
use serde_json::json;

#[test]
fn native_image_parts_reach_each_protocol_without_metadata_or_byte_arrays() {
    let request = ChatRequest {
        selection: ModelSelection {
            provider: "fixture".into(),
            model: "vision".into(),
            reasoning_effort: None,
        },
        messages: vec![Message::new(
            Role::User,
            vec![
                ContentPart::Text("what is this?".into()),
                ContentPart::Image {
                    mime: Some("image/webp".into()),
                    data: vec![1, 2, 3],
                },
                ContentPart::Text("compare".into()),
                ContentPart::Image {
                    mime: Some("image/png".into()),
                    data: vec![4, 5, 6],
                },
            ],
        )],
        options: GenerationOptions::default(),
        tools: vec![],
        resolved: None,
    };
    let openai =
        serde_json::to_value(protocols::openai::convert::to_real_request(&request)).unwrap();
    assert_eq!(
        openai["messages"][0]["content"][1],
        json!({"type":"image_url","image_url":{"url":"data:image/webp;base64,AQID"}})
    );
    let responses =
        serde_json::to_value(protocols::openai_responses::convert::to_request(&request)).unwrap();
    assert_eq!(
        responses["input"][0]["content"],
        json!([
            {"type":"input_text","text":"what is this?"}, {"type":"input_image","image_url":"data:image/webp;base64,AQID"},
            {"type":"input_text","text":"compare"}, {"type":"input_image","image_url":"data:image/png;base64,BAUG"}
        ])
    );
    let anthropic =
        serde_json::to_value(protocols::anthropic::convert::to_request(&request)).unwrap();
    assert_eq!(anthropic["messages"][0]["role"], "user");
    assert_eq!(
        anthropic["messages"][0]["content"][1],
        json!({"type":"image","source":{"type":"base64","media_type":"image/webp","data":"AQID"}})
    );
    let gemini = serde_json::to_value(protocols::gemini::convert::to_request(&request)).unwrap();
    assert_eq!(
        gemini["contents"][0]["parts"][1],
        json!({"inlineData":{"mimeType":"image/webp","data":"AQID"}})
    );
    assert_eq!(
        gemini["contents"][0]["parts"][3],
        json!({"inlineData":{"mimeType":"image/png","data":"BAUG"}})
    );
    for value in [openai, responses, anthropic, gemini] {
        let text = value.to_string();
        assert!(!text.contains("[1,2,3]"));
        assert!(!text.contains("message_id"));
    }
}
