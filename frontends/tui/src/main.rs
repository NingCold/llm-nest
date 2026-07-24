mod app;
mod command;
mod event;
mod logging;
mod runner;
mod ui;
mod widgets;

use std::sync::Arc;

use anyhow::Result;
use chat::ChatFeature;
use runtime::runtime::Runtime;

fn main() -> Result<()> {
    let _guard = logging::init_logging();
    let runtime = Runtime::from_config("config/llmn.toml")?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let chat = Arc::new(ChatFeature::new());
        runtime.register_feature(chat.clone()).await;
        runtime.initialize_features().await?;

        let mut app = app::App::new(runtime);
        app.refresh_sessions().await;
        runner::run(app, chat).await
    })
}