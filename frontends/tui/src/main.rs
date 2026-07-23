mod app;
mod command;
mod event;
mod logging;
mod runner;
mod ui;
mod widgets;

use anyhow::Result;
use chat::ChatService;

fn main() -> Result<()> {
    let _guard = logging::init_logging();
    let svc = ChatService::from_config("config/llmn.toml")?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        svc.new_session(None).await?;
        let mut app = app::App::new(svc);
        app.refresh_sessions().await;
        runner::run(app).await
    })
}
