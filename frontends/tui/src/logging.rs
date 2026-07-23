use tracing_appender::non_blocking::WorkerGuard;

const LOG_DIR: &str = "logs";
const LOG_FILE: &str = "tui.log";

pub fn init_logging() -> WorkerGuard {
    let file = tracing_appender::rolling::never(LOG_DIR, LOG_FILE);
    let (non_blocking, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .init();
    guard
}
