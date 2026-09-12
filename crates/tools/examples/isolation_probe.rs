fn main() {
    tools::process::worker_entry();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let registry = tools::ToolRegistry::with_builtins();
        assert_eq!(
            registry
                .run("add", serde_json::json!({"a":2,"b":3}))
                .await
                .unwrap()["sum"],
            5.0
        );
        assert_eq!(
            registry
                .run("echo", serde_json::json!({"text":"isolated"}))
                .await
                .unwrap()["echo"],
            "isolated"
        );
        assert!(registry.run("shell", serde_json::json!({})).await.is_err());
        println!("isolated add/echo and unknown-tool denial passed");
    });
}
