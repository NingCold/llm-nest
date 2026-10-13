//! Let the owning WebView finish IndexedDB writes before shutting down its host.
use crate::BackendState;
use std::sync::Mutex;
use tauri::Emitter;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct DraftCloseGuard(Mutex<GuardState>);
#[derive(Default)]
struct GuardState {
    page: Option<Uuid>,
    pending: Option<(Uuid, oneshot::Sender<Result<(), String>>)>,
}
impl DraftCloseGuard {
    fn register(&self, page: Uuid) {
        let mut state = self.0.lock().unwrap();
        state.pending = None;
        state.page = Some(page);
    }
    pub fn release(&self, page: Option<Uuid>) {
        let mut state = self.0.lock().unwrap();
        if page.is_none() || state.page == page {
            state.page = None;
            state.pending = None;
        }
    }
    fn begin(&self) -> Option<(Uuid, oneshot::Receiver<Result<(), String>>)> {
        let mut state = self.0.lock().unwrap();
        state.page?;
        let id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        state.pending = Some((id, tx));
        Some((id, rx))
    }
    fn finish(&self, page: Uuid, request: Uuid, error: Option<String>) {
        let mut state = self.0.lock().unwrap();
        if state.page == Some(page) && state.pending.as_ref().is_some_and(|(id, _)| *id == request)
        {
            let (_, tx) = state.pending.take().unwrap();
            let _ = tx.send(error.map_or(Ok(()), Err));
        }
    }
    pub async fn prepare(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let Some((request, response)) = self.begin() else {
            return Ok(());
        };
        if let Err(error) = app.emit_to("main", "drafts-prepare-close", request.to_string()) {
            self.0.lock().unwrap().pending = None;
            return Err(error.to_string());
        }
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), response).await;
        self.0.lock().unwrap().pending = None;
        result
            .map_err(|_| "等待草稿保存超时，请重试关闭".to_owned())?
            .map_err(|_| "页面已重新加载，请重试关闭".to_owned())?
    }
}

fn main_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("只有主窗口可以确认草稿保存".into());
    }
    Ok(())
}
#[tauri::command]
pub fn desktop_register_draft_guard(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, BackendState>,
    page: Uuid,
) -> Result<(), String> {
    main_window(&window)?;
    state.drafts.register(page);
    Ok(())
}
#[tauri::command]
pub fn desktop_unregister_draft_guard(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, BackendState>,
    page: Uuid,
) -> Result<(), String> {
    main_window(&window)?;
    state.drafts.release(Some(page));
    Ok(())
}
#[tauri::command]
pub fn desktop_finish_draft_close(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, BackendState>,
    page: Uuid,
    request: Uuid,
    error: Option<String>,
) -> Result<(), String> {
    main_window(&window)?;
    state.drafts.finish(page, request, error);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn close_waits_for_matching_page_and_request_and_can_retry_failure() {
        let guard = DraftCloseGuard::default();
        assert!(guard.begin().is_none());
        let page = Uuid::new_v4();
        guard.register(page);
        let (request, mut response) = guard.begin().unwrap();
        guard.finish(Uuid::new_v4(), request, None);
        guard.finish(page, Uuid::new_v4(), None);
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        guard.finish(page, request, Some("disk full".into()));
        assert_eq!(response.await.unwrap(), Err("disk full".into()));
        let (request, response) = guard.begin().unwrap();
        guard.finish(page, request, None);
        assert!(response.await.unwrap().is_ok());
    }
    #[tokio::test]
    async fn reload_releases_waiter_and_stale_cleanup_cannot_unregister_new_page() {
        let guard = DraftCloseGuard::default();
        let old = Uuid::new_v4();
        guard.register(old);
        let (_, response) = guard.begin().unwrap();
        guard.release(None);
        assert!(response.await.is_err());
        let new = Uuid::new_v4();
        guard.register(new);
        guard.release(Some(old));
        assert!(guard.begin().is_some());
        guard.release(Some(new));
        assert!(guard.begin().is_none());
    }
}
