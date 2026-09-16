#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    appimage::configure_gio_modules();
    tauri_frontend_lib::run()
}

#[cfg(target_os = "linux")]
mod appimage {
    use std::path::{Path, PathBuf};

    fn bundled_modules(appdir: &Path, modules: &Path) -> Option<PathBuf> {
        let appdir = appdir.canonicalize().ok()?;
        let modules = modules.canonicalize().ok()?;
        (modules.is_dir() && modules.starts_with(appdir)).then_some(modules)
    }

    pub(super) fn configure_gio_modules() {
        // linuxdeploy sets EXTRA_MODULES, but GLib also searches the host's
        // compiled-in module directory. A newer host GVfs can reference symbols
        // absent from the bundled GLib. Keep the bundled TLS module available.
        if std::env::var_os("GIO_MODULE_DIR").is_some() {
            return;
        }
        let (Some(appdir), Some(extra)) = (
            std::env::var_os("APPDIR"),
            std::env::var_os("GIO_EXTRA_MODULES"),
        ) else {
            return;
        };
        if let Some(modules) = bundled_modules(Path::new(&appdir), Path::new(&extra)) {
            // SAFETY: called at the start of main, before Tauri, GTK or any
            // application threads are started. The deb and empty-env worker
            // paths do not have APPDIR and never change the environment here.
            unsafe { std::env::set_var("GIO_MODULE_DIR", modules) };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_existing_modules_inside_the_appimage_are_selected() {
            let root = std::env::temp_dir().join(format!("llmn-gio-{}", uuid::Uuid::new_v4()));
            let appdir = root.join("AppDir");
            let bundled = appdir.join("usr/lib/gio/modules");
            let host = root.join("host-modules");
            std::fs::create_dir_all(&bundled).unwrap();
            std::fs::create_dir_all(&host).unwrap();
            assert_eq!(bundled_modules(&appdir, &bundled), Some(bundled.clone()));
            assert_eq!(bundled_modules(&appdir, &host), None);
            assert_eq!(bundled_modules(&appdir, &appdir.join("missing")), None);
            let symlink = appdir.join("outside");
            std::os::unix::fs::symlink(&host, &symlink).unwrap();
            assert_eq!(bundled_modules(&appdir, &symlink), None);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
