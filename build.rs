//! Embeds the app icon (high-resolution sizes only: 48–256 px) into the exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/starlite.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/starlite.ico");
        res.set("ProductName", "Starlite");
        res.set("FileDescription", "Starlite");
        if let Err(e) = res.compile() {
            println!("cargo:warning=couldn't embed the app icon: {e}");
        }
    }
}
