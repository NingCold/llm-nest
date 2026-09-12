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
    runtime::worker_entry();
    let _guard = logging::init_logging();
    let runtime = Runtime::from_config_persistent("config/llmn.toml", storage::default_data_dir())?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let chat = Arc::new(ChatFeature::new());
        runtime.register_feature(chat.clone()).await;
        runtime.initialize_features().await?;

        let mut app = app::App::new(runtime);
        app.model = app.runtime.default_model().await;
        app.refresh_sessions().await;
        runner::run(app, chat).await
    })
}
