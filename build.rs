#[path = "src/branding.rs"]
mod branding;

fn main() {
    println!("cargo:rerun-if-changed=assets/printer-420p-v6.ico");
    println!("cargo:rerun-if-changed=src/branding.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/printer-420p-v6.ico")
            .set("ProductName", branding::APP_NAME)
            .set("FileDescription", branding::APP_NAME)
            .set("InternalName", "label-studio-420p")
            .set("OriginalFilename", "label-studio-420p.exe")
            .compile().expect("failed to embed application icon");
    }
}
