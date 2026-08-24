//! Web server entry point: `cargo run -p web-server` (or the `web-server`
//! binary) serves the LLM Nest runtime over HTTP at 127.0.0.1:8787.
//!
//! Environment overrides:
//! - `LLMN_CONFIG` – path to `llmn.toml` (default: `config/llmn.toml` next to
//!   the working directory or the executable)
//! - `LLMN_DATA_DIR` – session storage dir (default: platform data dir + llmn)
//! - `LLMN_WEB_DIST` – directory with the built web frontend (default:
//!   `frontends/web/dist`)
//! - `LLMN_PORT` – listen port (default: 8787)

use std::path::PathBuf;
use std::sync::Arc;

use runtime::runtime::Runtime;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

fn find_config_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("LLMN_CONFIG") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Ok(p);
        }
        return Err(format!("LLMN_CONFIG not found: {}", p.display()));
    }
    let candidates = [PathBuf::from("config/llmn.toml"), {
        let mut p = std::env::current_exe().map_err(|e| e.to_string())?;
        p.pop();
        p.pop();
        p.pop();
        p.push("config/llmn.toml");
        p
    }];
    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    Err(format!(
        "config/llmn.toml not found, tried: {:?}",
        candidates
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = find_config_path()?;
    let runtime = Runtime::from_config_persistent(&config_path, storage::default_data_dir())
        .map_err(|e| format!("failed to load config: {e}"))?;
    let runtime = Arc::new(runtime);

    let chat = Arc::new(chat::ChatFeature::new());
    let chat_clone = chat.clone();
    let rt = runtime.clone();
    tokio::spawn(async move {
        rt.register_feature(chat_clone).await;
        rt.initialize_features()
            .await
            .map_err(|e| eprintln!("feature init failed: {e}"))
    });

    let state = Arc::new(web_server::AppState {
        runtime: (*runtime).clone(),
        chat,
        cancel_map: Arc::new(Mutex::new(std::collections::HashMap::<
            String,
            CancellationToken,
        >::new())),
    });

    let app = web_server::router(state);
    let port: u16 = std::env::var("LLMN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(
        "web-server listening on http://{addr} (config: {})",
        config_path.display()
    );
    axum::serve(listener, app).await?;
    Ok(())
}
