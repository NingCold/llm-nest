//! Opt in to DWM's corner/border rendering without restoring tao's shadow insets.
//! Keep the WebView transparent and its HTML background opaque (see WindowControls).

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;

pub struct FrameState {
    dark: AtomicBool,
}

pub fn setup(app: &tauri::App) {
    app.manage(FrameState {
        dark: AtomicBool::new(true),
    });
    if let Some(webview) = app.get_webview_window("main") {
        let window = webview.as_ref().window();
        if let Err(error) = apply(&window, true) {
            eprintln!("window frame setup failed: {error}");
        }
        let target = window.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Focused(_)) {
                let dark = target.state::<FrameState>().dark.load(Ordering::Relaxed);
                if let Err(error) = apply(&target, dark) {
                    eprintln!("window frame update failed: {error}");
                }
            }
        });
    }
}

/// Returns false on systems without the Windows 11 DWM attributes.
#[tauri::command]
pub fn set_window_appearance(
    window: tauri::Window,
    state: tauri::State<'_, FrameState>,
    dark: bool,
) -> Result<bool, String> {
    state.dark.store(dark, Ordering::Relaxed);
    apply(&window, dark)
}

#[cfg(windows)]
fn apply(window: &tauri::Window, dark: bool) -> Result<bool, String> {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };

    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
    let focused = window.is_focused().map_err(|e| e.to_string())?;
    // COLORREF is 0x00BBGGRR. Neutral gray stays distinct from white adjacent windows.
    let shade: u32 = match (dark, focused) {
        (true, true) => 0x68,
        (true, false) => 0x4c,
        (false, true) => 0x9a,
        (false, false) => 0xbc,
    };
    let border = shade * 0x010101;
    for (attribute, value) in [
        (DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND as u32),
        (DWMWA_BORDER_COLOR, border),
    ] {
        // SAFETY: HWND belongs to this live Tauri window; each documented attribute
        // accepts a 32-bit value, kept alive for the synchronous call.
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                (&value as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if result == 0x80070057u32 as i32 {
            return Ok(false); // E_INVALIDARG: Windows 10 has no corner/border attributes.
        }
        if result < 0 {
            return Err(format!(
                "DWM attribute {attribute}: 0x{:08X}",
                result as u32
            ));
        }
    }
    Ok(true)
}

#[cfg(not(windows))]
fn apply(_window: &tauri::Window, _dark: bool) -> Result<bool, String> {
    Ok(false)
}
