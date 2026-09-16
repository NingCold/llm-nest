use crate::{BackendState, config_path::AppPaths};

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DirectoryKind {
    Config,
    Data,
}

#[tauri::command]
pub fn get_app_paths(state: tauri::State<'_, BackendState>) -> Result<AppPaths, String> {
    state.paths.clone()
}

/// Only backend-selected directories can be opened; no arbitrary path/command input.
#[tauri::command]
pub async fn open_app_directory(
    state: tauri::State<'_, BackendState>,
    kind: DirectoryKind,
) -> Result<(), String> {
    let paths = state.paths.as_ref().map_err(Clone::clone)?;
    let path = match kind {
        DirectoryKind::Config => paths.config_file.parent().ok_or("配置文件没有父目录")?,
        DirectoryKind::Data => &paths.data_dir,
    };
    // Do not create an empty destination before a pending migration has succeeded.
    if !path.is_dir() {
        return Err(format!("目录尚未创建或无法访问：{}", path.display()));
    }
    open_directory(path)
        .await
        .map_err(|error| format!("无法打开目录 {}：{error}", path.display()))
}

#[cfg(not(target_os = "linux"))]
async fn open_directory(path: &std::path::Path) -> std::io::Result<()> {
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || open::that_detached(path))
        .await
        .map_err(std::io::Error::other)?
}

#[cfg(target_os = "linux")]
async fn open_directory(path: &std::path::Path) -> std::io::Result<()> {
    use std::{io, process::Stdio, time::Duration};
    let appdir = std::env::var_os("APPDIR").map(std::path::PathBuf::from);
    let mut last_error = io::Error::other("没有可用的文件管理器");
    for mut command in open::commands(path) {
        // Host launchers must not load GTK/GIO/libraries from an AppImage.
        // Keep desktop-session variables and the host parts of search paths.
        if let Some(appdir) = &appdir {
            for name in [
                "PATH",
                "LD_LIBRARY_PATH",
                "GIO_MODULE_DIR",
                "GIO_EXTRA_MODULES",
                "GTK_PATH",
                "GTK_EXE_PREFIX",
                "GTK_DATA_PREFIX",
                "GDK_PIXBUF_MODULEDIR",
                "GDK_PIXBUF_MODULE_FILE",
                "XDG_DATA_DIRS",
            ] {
                if let Some(value) = std::env::var_os(name) {
                    if let Some(value) = host_search_path(&value, appdir) {
                        command.env(name, value);
                    } else {
                        command.env_remove(name);
                    }
                }
            }
        }
        let mut command = tokio::process::Command::from(command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        match tokio::time::timeout(Duration::from_secs(10), command.status()).await {
            Ok(Ok(status)) if status.success() => return Ok(()),
            Ok(Ok(status)) => {
                last_error = io::Error::other(format!("文件管理器启动失败：{status}"))
            }
            Ok(Err(error)) => last_error = error,
            // Do not launch another instance when the first handler may still be opening.
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "文件管理器启动超时",
                ));
            }
        }
    }
    Err(last_error)
}

#[cfg(target_os = "linux")]
fn host_search_path(
    value: &std::ffi::OsStr,
    appdir: &std::path::Path,
) -> Option<std::ffi::OsString> {
    let paths: Vec<_> = std::env::split_paths(value)
        .filter(|path| !path.starts_with(appdir))
        .collect();
    (!paths.is_empty()).then(|| std::env::join_paths(paths).expect("existing search path"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{ffi::OsStr, path::Path};

    #[test]
    fn file_manager_keeps_host_paths_and_excludes_appimage_paths() {
        let appdir = Path::new("/tmp/.mount_llmn");
        assert_eq!(
            host_search_path(OsStr::new("/tmp/.mount_llmn/usr/lib"), appdir),
            None
        );
        assert_eq!(
            host_search_path(
                OsStr::new("/tmp/.mount_llmn/usr/bin:/usr/local/bin:/usr/bin"),
                appdir
            )
            .unwrap(),
            "/usr/local/bin:/usr/bin"
        );
        assert_eq!(
            host_search_path(OsStr::new("/tmp/.mount_llmn_other:/opt/host-gio"), appdir).unwrap(),
            "/tmp/.mount_llmn_other:/opt/host-gio"
        );
    }
}
