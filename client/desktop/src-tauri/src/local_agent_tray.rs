use crate::local_agent::LocalAgentState;
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};
pub enum CloseChoice {
    Continue,
    Stop,
    Cancel,
}
pub async fn background_choice(
    app: &tauri::AppHandle,
    state: &LocalAgentState,
) -> Result<CloseChoice, String> {
    if state.background_explained() {
        return Ok(CloseChoice::Continue);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().message("关闭窗口后，本机 AI 会继续在后台运行。可从托盘重新打开窗口或停止本机 Agent。电脑休眠或关机时本机 AI 离线；远程节点不受影响。\n\n是否继续后台运行？").title("本机 AI 后台运行").buttons(MessageDialogButtons::YesNoCancelCustom("继续后台运行".into(),"停止本机 Agent 并退出".into(),"取消关闭".into())).show_with_result(move|result|{let _=tx.send(result);});
    let result = rx.await.map_err(|_| "关闭说明窗口未完成")?;
    let choice = match result {
        MessageDialogResult::Yes => CloseChoice::Continue,
        MessageDialogResult::No => CloseChoice::Stop,
        MessageDialogResult::Custom(ref v) if v == "继续后台运行" => CloseChoice::Continue,
        MessageDialogResult::Custom(ref v) if v == "停止本机 Agent 并退出" => {
            CloseChoice::Stop
        }
        _ => CloseChoice::Cancel,
    };
    if !matches!(choice, CloseChoice::Cancel) {
        state.explain_background()?;
    }
    Ok(choice)
}
pub fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "local-open", "打开 LLM Nest", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "local-stop", "停止本机 Agent", true, None::<&str>)?;
    let quit = MenuItem::with_id(
        app,
        "local-quit",
        "停止本机 Agent 并退出",
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&open, &stop, &quit])?;
    let mut tray = TrayIconBuilder::with_id("local-agent")
        .tooltip("LLM Nest · 本机 Agent")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "local-open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "local-stop" => {
                let a = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = a.state::<LocalAgentState>().stop_all(&a).await {
                        a.dialog()
                            .message(e)
                            .title("本机 Agent 未停止")
                            .show(|_| {});
                    }
                });
            }
            "local-quit" => crate::request_shutdown(app, true),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
