fn main() {
    println!("cargo:rerun-if-changed=assets/printer-420p-v6.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new().set_icon("assets/printer-420p-v6.ico").compile().expect("failed to embed application icon");
    }
}
