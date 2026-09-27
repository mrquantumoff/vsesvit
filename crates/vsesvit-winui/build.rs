//! Puts `Microsoft.Web.WebView2.Core.dll` next to the executable.
//!
//! The Windows App Runtime registers the WebView2 WinRT classes but does not ship this DLL
//! (docs/design/research-winui.md, pitfall 1), so the shell loads an app-local copy. It comes
//! from the pinned Microsoft.Web.WebView2 package, verified by SHA-256 and cached under
//! `<target>/nuget`. Put a verified `.nupkg` there to build offline.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=tools/nuget.rs");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && let Err(e) = webview2_dll::copy_next_to_exe()
    {
        println!("cargo::error=Microsoft.Web.WebView2.Core.dll: {e}");
    }
}

#[cfg(windows)]
#[path = "tools/nuget.rs"]
#[allow(dead_code, reason = "shared with tools/bindgen, which uses the rest")]
mod nuget;

#[cfg(windows)]
mod webview2_dll {
    use std::path::{Path, PathBuf};

    use crate::nuget;

    const DLL: &str = "Microsoft.Web.WebView2.Core.dll";

    pub fn copy_next_to_exe() -> Result<(), String> {
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("x86_64") => "win-x64",
            Ok("aarch64") => "win-arm64",
            Ok("x86") => "win-x86",
            other => return Err(format!("unsupported target architecture {other:?}")),
        };
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").map_err(|e| e.to_string())?);
        // OUT_DIR is <profile dir>/build/<package>-<hash>/out; binaries land in <profile dir>.
        let exe_dir = out_dir
            .ancestors()
            .nth(3)
            .ok_or_else(|| format!("unexpected OUT_DIR {}", out_dir.display()))?;
        let dest = exe_dir.join(DLL);
        println!("cargo::rerun-if-changed={}", dest.display());

        let cache = target_dir(exe_dir).join("nuget");
        let nupkg = nuget::WEBVIEW2.fetch(&cache)?;
        nuget::extract(&nupkg, &format!("runtimes/{arch}/native_uap/{DLL}"), &dest)
    }

    /// The profile dir is `<target>/<profile>`, or `<target>/<triple>/<profile>` when cross
    /// building, so the cache is shared by debug and release builds.
    fn target_dir(exe_dir: &Path) -> PathBuf {
        let triple = std::env::var("TARGET").unwrap_or_default();
        let mut dir = exe_dir.parent().unwrap_or(exe_dir);
        if dir.file_name().is_some_and(|name| name == triple.as_str())
            && let Some(parent) = dir.parent()
        {
            dir = parent;
        }
        dir.to_path_buf()
    }
}
