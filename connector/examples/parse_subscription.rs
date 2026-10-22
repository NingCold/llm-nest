use connector::SubscriptionEvent;

fn main() -> Result<(), serde_json::Error> {
    let payload = r#"{"kind":"generation","event":{"type":"execution.notice","conversationId":"00000000-0000-4000-8000-000000000001","error":{"code":"model_unavailable","message":"Choose a model"}}}"#;
    let event: SubscriptionEvent = serde_json::from_str(payload)?;
    if let SubscriptionEvent::Generation { event } = event {
        println!("{event:?}");
    }
    Ok(())
}
