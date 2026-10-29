fn main() {
    // tauri-build embeds the ICO into the Windows executable. Without this
    // dependency an incremental build can keep the old PE icon resource even
    // while generate_context! already uses the new icon for the window.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=capabilities");
    tauri_build::build()
}
