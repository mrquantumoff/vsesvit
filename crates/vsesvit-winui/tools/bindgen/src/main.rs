//! Regenerates `crates/vsesvit-winui/src/bindings.rs`.
//!
//! ```text
//! cargo run --release --target-dir target/bindgen \
//!     --manifest-path crates/vsesvit-winui/tools/bindgen/Cargo.toml
//! ```
//!
//! Downloads the pinned NuGet packages into `<workspace>/target/nuget` (SHA-256 verified),
//! extracts their `.winmd` files and runs windows-bindgen in minimal mode with the filter in
//! `crates/vsesvit-winui/bindings.txt`. Windows.* and Win32 metadata come from windows-bindgen.

#[path = "../../nuget.rs"]
mod nuget;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nuget::{FOUNDATION, INTERACTIVE_EXPERIENCES, Package, WEBVIEW2, WINUI};

/// Package, and the directory inside it whose `.winmd` files are inputs.
const METADATA: [(&Package, &str); 4] = [
    (&WINUI, "metadata/"),
    (&FOUNDATION, "metadata/"),
    (&INTERACTIVE_EXPERIENCES, "metadata/10.0.18362.0/"),
    (&WEBVIEW2, "lib/"),
];

fn main() -> ExitCode {
    match run() {
        Ok(output) => {
            println!("wrote {}", output.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<PathBuf, String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let crate_dir = crate_dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", crate_dir.display()))?;
    let workspace = crate_dir.join("../..");
    let cache = workspace.join("target").join("nuget");
    let winmd_dir = cache.join("winmd");
    if winmd_dir.exists() {
        std::fs::remove_dir_all(&winmd_dir)
            .map_err(|e| format!("clear {}: {e}", winmd_dir.display()))?;
    }

    for (package, dir) in METADATA {
        let nupkg = package.fetch(&cache)?;
        let names = nuget::entries(&nupkg, dir, ".winmd")?;
        let direct: Vec<_> = names
            .iter()
            .filter(|name| !name[dir.len()..].contains('/'))
            .collect();
        if direct.is_empty() {
            return Err(format!("no .winmd files under {dir} in {}", package.id));
        }
        for name in direct {
            let file = &name[dir.len()..];
            nuget::extract(&nupkg, name, &winmd_dir.join(file))?;
        }
    }

    let output = crate_dir.join("src").join("bindings.rs");
    windows_bindgen::builder()
        .input(&winmd_dir)
        .input_default()
        .output(&output)
        .flat()
        .minimal()
        .implements([
            "Microsoft.UI.Xaml.IApplicationOverrides",
            "Microsoft.UI.Xaml.Markup.IXamlMetadataProvider",
        ])
        .compose("Microsoft.UI.Xaml.Application")
        .filter_file(crate_dir.join("bindings.txt"))
        .write();
    Ok(output)
}
