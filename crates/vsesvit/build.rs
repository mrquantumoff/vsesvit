//! Embeds the app icon and version info in `vsesvit.exe`, so Explorer, the taskbar and shortcuts
//! show it. The `.ico` is rendered from the SVG by `cargo xtask icons` and committed.

fn main() {
    const ICON: &str = "../../packaging/icons/dev.mrquantumoff.vsesvit.ico";
    println!("cargo::rerun-if-changed={ICON}");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ICON).set("ProductName", "Vsesvit").set("FileDescription", "Vsesvit");
        if let Err(e) = res.compile() {
            println!("cargo::error=embedding the icon: {e}");
        }
    }
}
