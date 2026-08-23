//! Hot-reload watcher for the configuration document.
//!
//! Watches the parent directory (robust against editor rename-replace saves),
//! debounces bursts, and re-applies the document through
//! [`Runtime::reload_config`]. Reload semantics mirror DeepSeek Harness's
//! settings hot-publish:
//!
//! - the whole next configuration is parsed and validated **before** anything
//!   swaps, so a refused document leaves the running configuration serving;
//! - every outcome is reported on the caller's event channel; a failed reload
//!   names the reason and the previous configuration keeps serving;
//! - in-flight requests are unaffected (the LLM client freezes a resolved
//!   selection before its first await; a swap affects only the next request).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::error::{Result, RuntimeError};
use crate::runtime::Runtime;

/// Outcome of one applied watch cycle.
#[derive(Debug, Clone)]
pub enum ConfigWatchEvent {
    /// The document changed and the new configuration is now serving.
    Reloaded,
    /// The document changed but the reload was refused; the previous
    /// configuration keeps serving.
    Failed(String),
}

/// Write-settle window: a burst of events (editors often emit several) is
/// collapsed into one reload.
const DEBOUNCE: Duration = Duration::from_millis(100);

/// Bridge notify's watcher thread into a tokio channel.
struct NotifyBridge(mpsc::UnboundedSender<notify::Result<Event>>);

impl notify::EventHandler for NotifyBridge {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        // UnboundedSender::send is non-async and safe from any thread.
        let _ = self.0.send(event);
    }
}

/// Whether a notify event indicates the watched document may have changed.
///
/// Only write-indicating kinds qualify. `Access` events are deliberately
/// excluded: the reload itself reads the document, and inotify reports a plain
/// read as `Access(Open)` — reacting to it would make every reload trigger
/// itself in a self-sustaining loop.
fn is_write_event(event: &Event, file_name: &std::ffi::OsStr) -> bool {
    use notify::EventKind;
    let kind_matches = matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
    );
    kind_matches && event.paths.iter().any(|p| p.file_name() == Some(file_name))
}

/// Spawn a configuration watcher for `path` and re-apply the document through
/// `runtime` on every change. Returns the watcher task handle; the caller
/// keeps it for the process lifetime (dropping it does not stop the task).
pub fn spawn_config_watcher(
    runtime: Arc<Runtime>,
    path: impl Into<PathBuf>,
    tx: mpsc::UnboundedSender<ConfigWatchEvent>,
) -> Result<tokio::task::JoinHandle<()>> {
    let path = path.into();
    let parent = path
        .parent()
        .ok_or_else(|| {
            RuntimeError::ConfigError(format!("config path has no parent: {}", path.display()))
        })?
        .to_path_buf();
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            RuntimeError::ConfigError(format!("config path has no file name: {}", path.display()))
        })?
        .to_os_string();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<notify::Result<Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(NotifyBridge(event_tx))?;
    // Watch the parent directory, not the file: editors replace the file via
    // rename, which some platforms report only on the directory.
    watcher.watch(&parent, RecursiveMode::NonRecursive)?;

    let handle = tokio::spawn(async move {
        // Keep the watcher alive for the task's lifetime. Dropping it stops
        // file events and drops the event sender, which would close this loop.
        let _watcher = watcher;
        let mut pending = false;
        let mut timer = Box::pin(tokio::time::sleep(DEBOUNCE));
        loop {
            tokio::select! {
                maybe = event_rx.recv() => {
                    match maybe {
                        Some(Ok(event)) => {
                            if is_write_event(&event, &file_name) {
                                pending = true;
                                timer.as_mut().reset(tokio::time::Instant::now() + DEBOUNCE);
                            }
                        }
                        Some(Err(err)) => {
                            let _ = tx.send(ConfigWatchEvent::Failed(format!("file watcher error: {err}")));
                        }
                        None => break,
                    }
                }
                _ = &mut timer => {
                    if pending {
                        pending = false;
                        match runtime.reload_config(&path).await {
                            Ok(()) => {
                                let _ = tx.send(ConfigWatchEvent::Reloaded);
                            }
                            Err(err) => {
                                let _ = tx.send(ConfigWatchEvent::Failed(err.to_string()));
                            }
                        }
                    }
                    // Rearm: a fired Sleep stays ready forever, and an
                    // always-ready arm would spin the select loop and re-trigger
                    // reloads while later events of the same burst keep
                    // arriving. Rearming turns it back into a one-shot window.
                    timer.as_mut().reset(tokio::time::Instant::now() + DEBOUNCE);
                }
            }
        }
    });
    Ok(handle)
}

/// Convenience for tests and quick programs: spawn the watcher, returning the
/// handle plus the event receiver.
pub fn spawn_config_watcher_channel(
    runtime: Arc<Runtime>,
    path: impl Into<PathBuf>,
) -> Result<(
    tokio::task::JoinHandle<()>,
    mpsc::UnboundedReceiver<ConfigWatchEvent>,
)> {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = spawn_config_watcher(runtime, path, tx)?;
    Ok((handle, rx))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn notify_fires_on_file_write() {
        let dir = std::env::temp_dir().join(format!("llmn-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("llmn.toml");
        std::fs::write(&file, "a = 1\n").unwrap();

        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })
        .expect("watcher builds");
        watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .expect("watch dir");

        std::fs::write(&file, "a = 2\n").unwrap();

        let event = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("notify event within 3s");
        let event = event.expect("notify result ok");
        let relevant = event
            .paths
            .iter()
            .any(|p| p.file_name() == Some("llmn.toml".as_ref()));
        assert!(relevant, "event paths: {:?}", event.paths);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod reload_tests {
    use std::time::Duration;

    use super::*;
    use crate::runtime::Runtime;

    #[tokio::test]
    async fn hot_reload_fires_and_applies() {
        let dir = std::env::temp_dir().join(format!("llmn-hot-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("llmn.toml");
        std::fs::write(
            &file,
            r#"
[providers.deepseek]
protocol = "openai"
api_key = "k"
base_url = "https://api.deepseek.com"
default_model = "chat"
[providers.deepseek.models.chat]
model = "deepseek-chat"
"#,
        )
        .unwrap();

        let runtime = Arc::new(Runtime::from_config(&file).expect("runtime builds"));
        assert_eq!(
            runtime.default_model().await.unwrap().model,
            "deepseek-chat"
        );

        let (handle, mut rx) =
            spawn_config_watcher_channel(runtime.clone(), file.clone()).expect("watcher spawns");
        // give notify/inotify a moment to arm the watch
        tokio::time::sleep(Duration::from_millis(300)).await;

        std::fs::write(
            &file,
            r#"
[providers.deepseek]
protocol = "openai"
api_key = "k"
base_url = "https://api.deepseek.com"
default_model = "reasoner"
[providers.deepseek.models.chat]
model = "deepseek-chat"
[providers.deepseek.models.reasoner]
model = "deepseek-reasoner"
"#,
        )
        .unwrap();

        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ConfigWatchEvent::Reloaded)) => {}
            Ok(Some(ConfigWatchEvent::Failed(msg))) => {
                panic!("reload failed: {msg}");
            }
            Ok(None) => panic!("event channel closed"),
            Err(_) => panic!("no reload event within 5s"),
        }
        assert_eq!(
            runtime.default_model().await.unwrap().model,
            "deepseek-reasoner"
        );
        assert_eq!(runtime.list_models().await.len(), 2);

        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
