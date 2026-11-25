//! The icon, asking for no admin rights, and what the setup installs:
//! STARLITE_SETUP_PAYLOAD names a folder with Starlite.exe and
//! THIRD-PARTY-NOTICES.txt (copied in at build time).
fn main() {
    println!("cargo:rerun-if-changed=../assets/starlite.ico");
    println!("cargo:rerun-if-env-changed=STARLITE_SETUP_PAYLOAD");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let from = std::env::var("STARLITE_SETUP_PAYLOAD").ok().map(std::path::PathBuf::from);
    for name in ["Starlite.exe", "THIRD-PARTY-NOTICES.txt"] {
        let bytes = from.as_ref().and_then(|d| std::fs::read(d.join(name)).ok()).unwrap_or_default();
        if from.is_some() && bytes.is_empty() {
            panic!("STARLITE_SETUP_PAYLOAD has no {name}");
        }
        std::fs::write(out.join(name), bytes).unwrap();
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/starlite.ico");
        res.set("ProductName", "Starlite Setup");
        res.set("FileDescription", "Starlite Setup");
        res.set_manifest(r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
<requestedExecutionLevel level="asInvoker" uiAccess="false"/>
</requestedPrivileges></security></trustInfo>
<application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
<dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
</windowsSettings></application>
</assembly>"#);
        if let Err(e) = res.compile() {
            println!("cargo:warning=couldn't embed the icon: {e}");
        }
    }
}
