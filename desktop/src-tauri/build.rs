fn main() {
    // tauri-build embeds the ICO into the Windows executable. Without this
    // dependency an incremental build can keep the old PE icon resource even
    // while generate_context! already uses the new icon for the window.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=capabilities");
    // Tauri's main executable resources do not supply the example's activation context.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/sdk-open-benchmark.manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
    tauri_build::build()
}
