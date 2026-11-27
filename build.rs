//! On Windows, embeds the program's icon so Explorer, the taskbar and the Start menu show it.
fn main() {
    println!("cargo:rerun-if-changed=packaging/coverant.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let mut res = winresource::WindowsResource::new();
        res.set_icon(&dir.join("packaging/coverant.ico").to_string_lossy());
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
