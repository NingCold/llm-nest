//! DWM corners for floating windows, an inset hairline for vertically docked ones.
//! Keep tao's shadow insets disabled and the WebView's HTML background opaque.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, Manager};

#[derive(Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Geometry {
    maximized: bool,
    fullscreen: bool,
    vertical_docked: bool,
    focused: bool,
    scale_factor: f64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameInfo {
    #[serde(flatten)]
    geometry: Geometry,
    native_border: bool,
    revision: u64,
}

#[derive(Default)]
pub struct FrameState {
    dark: AtomicBool,
    last: Mutex<Option<(bool, FrameInfo)>>,
}

pub fn setup(app: &tauri::App) {
    app.manage(FrameState {
        dark: AtomicBool::new(true),
        ..Default::default()
    });
    if let Some(webview) = app.get_webview_window("main") {
        let window = webview.as_ref().window();
        if let Err(error) = update(&window) {
            eprintln!("window frame setup failed: {error}");
        }
        let target = window.clone();
        window.on_window_event(move |event| {
            if matches!(
                event,
                tauri::WindowEvent::Focused(_)
                    | tauri::WindowEvent::Moved(_)
                    | tauri::WindowEvent::Resized(_)
                    | tauri::WindowEvent::ScaleFactorChanged { .. }
            ) && let Err(error) = update(&target)
            {
                eprintln!("window frame update failed: {error}");
            }
        });
    }
}

#[tauri::command]
pub fn set_window_appearance(
    window: tauri::Window,
    state: tauri::State<'_, FrameState>,
    dark: bool,
) -> Result<FrameInfo, String> {
    state.dark.store(dark, Ordering::Relaxed);
    update(&window)
}

fn update(window: &tauri::Window) -> Result<FrameInfo, String> {
    let maximized = window.is_maximized().map_err(|e| e.to_string())?;
    let fullscreen = window.is_fullscreen().map_err(|e| e.to_string())?;
    let geometry = Geometry {
        maximized,
        fullscreen,
        vertical_docked: !maximized
            && !fullscreen
            && !window.is_minimized().map_err(|e| e.to_string())?
            && vertical_docked(window)?,
        focused: window.is_focused().map_err(|e| e.to_string())?,
        scale_factor: window.scale_factor().map_err(|e| e.to_string())?,
    };
    // Do not hold a lock across Tauri's window queries (they may use the event loop).
    #[cfg(windows)]
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
    let state = window.state::<FrameState>();
    let mut last = state.last.lock().map_err(|e| e.to_string())?;
    let dark = state.dark.load(Ordering::Relaxed);
    if let Some((old_dark, info)) = last.as_ref()
        && *old_dark == dark
        && info.geometry == geometry
    {
        return Ok(info.clone());
    }
    // Resize/move events query geometry, but DWM and JS only receive actual changes.
    #[cfg(windows)]
    let native_border = apply(hwnd, dark, &geometry)?;
    #[cfg(not(windows))]
    let native_border = false;
    let info = FrameInfo {
        geometry,
        native_border,
        revision: last.as_ref().map_or(1, |(_, info)| info.revision + 1),
    };
    *last = Some((dark, info.clone()));
    drop(last);
    // Frontend revisions also guard against out-of-order event/IPC delivery.
    window
        .emit("window-frame-changed", &info)
        .map_err(|e| e.to_string())?;
    Ok(info)
}

#[cfg(windows)]
fn vertical_docked(window: &tauri::Window) -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::{
            Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
        },
    };
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
    let mut bounds = RECT::default();
    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: All handles belong to this live window; output buffers have the
    // sizes required by the synchronous Win32 APIs. Both rectangles are physical
    // screen coordinates, including negative origins and taskbar work-area insets.
    unsafe {
        let result = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut bounds as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        );
        if result < 0 {
            return Err(format!("DWM frame bounds: 0x{:08X}", result as u32));
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if GetMonitorInfoW(monitor, &mut monitor_info) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    let work = monitor_info.rcWork;
    Ok(spans_work_area(
        bounds.top,
        bounds.bottom,
        work.top,
        work.bottom,
    ))
}

#[cfg(not(windows))]
fn vertical_docked(_window: &tauri::Window) -> Result<bool, String> {
    Ok(false)
}

/// DWM bounds exclude invisible resize gutters. Allow only two physical pixels
/// for rounding/auto-hidden taskbars, not a DPI-scaled band near the screen edges.
#[cfg(any(windows, test))]
fn spans_work_area(top: i32, bottom: i32, work_top: i32, work_bottom: i32) -> bool {
    bottom > top
        && work_bottom > work_top
        && (i64::from(top) - i64::from(work_top)).abs() <= 2
        && (i64::from(bottom) - i64::from(work_bottom)).abs() <= 2
}

#[cfg(windows)]
fn apply(
    hwnd: windows_sys::Win32::Foundation::HWND,
    dark: bool,
    geometry: &Geometry,
) -> Result<bool, String> {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
        DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    let shade: u32 = match (dark, geometry.focused) {
        (true, true) => 0x68,
        (true, false) => 0x4c,
        (false, true) => 0x9a,
        (false, false) => 0xbc,
    };
    let (corner, border) = if geometry.vertical_docked {
        // Use exactly one client hairline here, avoiding a doubled native border.
        (DWMWCP_DONOTROUND, DWMWA_COLOR_NONE)
    } else {
        (DWMWCP_ROUND, shade * 0x010101) // COLORREF is 0x00BBGGRR.
    };
    for (attribute, value) in [
        (DWMWA_WINDOW_CORNER_PREFERENCE, corner as u32),
        (DWMWA_BORDER_COLOR, border),
    ] {
        // SAFETY: HWND is live; each documented attribute accepts a 32-bit value.
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                (&value as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if result == 0x80070057u32 as i32 {
            return Ok(false); // E_INVALIDARG: Windows 10 uses the CSS fallback.
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

#[cfg(test)]
mod tests {
    use super::spans_work_area;

    #[test]
    fn vertical_docking_uses_the_monitor_work_area() {
        assert!(spans_work_area(0, 1040, 0, 1040)); // Bottom taskbar.
        assert!(spans_work_area(48, 1080, 48, 1080)); // Top taskbar.
        assert!(spans_work_area(-1440, -40, -1440, -40)); // Monitor above primary.
        assert!(spans_work_area(1, 2159, 0, 2160)); // Physical pixels at high DPI.
        assert!(!spans_work_area(0, 1080, 0, 1040)); // Covers taskbar, not work area.
        assert!(!spans_work_area(100, 1040, 0, 1040)); // Only bottom touches.
        assert!(!spans_work_area(0, 800, 0, 1040)); // Only top touches.
        assert!(!spans_work_area(3, 1040, 0, 1040)); // Near edge is not docked.
        assert!(!spans_work_area(0, 0, 0, 0)); // Empty geometry during transitions.
        assert!(!spans_work_area(i32::MIN, i32::MAX, 0, 1040));
    }
}
