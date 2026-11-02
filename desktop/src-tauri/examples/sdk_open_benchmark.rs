//! Isolated synthetic SDK benchmark; does not initialize the product runtime.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use std::path::PathBuf;
use tauri::{WebviewUrl, WebviewWindowBuilder};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        (3..=4).contains(&args.len()),
        "Usage: sdk_open_benchmark PROFILE LOOPBACK_URL [devtools]"
    );
    let debug = args.get(3).is_some_and(|value| value == "devtools");
    assert!(args.len() == 3 || debug, "Expected devtools");
    let profile = PathBuf::from(&args[1]);
    assert!(profile.is_absolute(), "PROFILE must be an absolute path");
    assert_eq!(
        profile.file_name().and_then(|name| name.to_str()),
        Some("synthetic-profile"),
        "Use a dedicated synthetic-profile directory"
    );
    let url: tauri::Url = args[2].parse().expect("Invalid URL");
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert_eq!(url.path(), "/");
    std::fs::create_dir_all(&profile).expect("Create synthetic profile");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.llmnest.sdk-benchmark".into();
    tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let origin = url.origin();
            let window = WebviewWindowBuilder::new(app, "benchmark", WebviewUrl::External(url))
                .title("LLM Nest · 隔离性能测试")
                .data_directory(profile)
                .devtools(debug)
                .on_navigation(move |destination| {
                    if destination.origin() != origin {
                        return false;
                    }
                    if destination.path() == "/done" {
                        handle.exit(0);
                        return false;
                    }
                    true
                })
                .build()?;
            if debug {
                window.open_devtools();
            }
            Ok(())
        })
        .run(context)
        .expect("Run isolated benchmark");
}
