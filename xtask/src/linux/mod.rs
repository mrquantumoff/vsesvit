//! deb, rpm, pacman, AppImage and Flatpak. Each format has its own file. `stage` builds the
//! `usr/` tree that deb, rpm, pacman and the AppImage share, and `deps` is the one runtime
//! dependency table.

mod appimage;
mod deb;
mod deps;
mod elf;
mod flatpak;
mod pacman;
mod rpm;
mod stage;
mod sysroot;
mod util;

use std::path::PathBuf;
use std::process::Command;

use crate::Result;
use crate::ctx::{BINARY, Ctx, Format};

/// Builds the release binary and the package for `format` into `target/dist/`.
pub fn package(format: Format, ctx: &Ctx) -> Result<PathBuf> {
    if !cfg!(target_os = "linux") {
        return Err(format!(
            "{} packages are built on Linux; from Windows run: bash scripts/wsl.sh xtask package {}",
            format.variant(),
            format.variant()
        ));
    }
    let artifact = ctx.artifact(format);
    match format {
        Format::Nsis => return Err("nsis is not a Linux format".to_owned()),
        Format::Flatpak => flatpak::build(ctx, &artifact)?,
        Format::Deb | Format::Rpm | Format::Pacman | Format::AppImage => {
            let binary = build_release(ctx)?;
            let stage = stage::stage(ctx, format, &binary)?;
            match format {
                Format::Deb => deb::build(ctx, &stage, &artifact)?,
                Format::Rpm => rpm::build(ctx, &stage, &artifact)?,
                Format::Pacman => pacman::build(ctx, &stage, &artifact)?,
                _ => appimage::build(ctx, &stage, &artifact)?,
            }
        }
    }
    Ok(artifact)
}

/// `cargo build --release --locked -p vsesvit`, returning the executable cargo reports, so the
/// binary is found whatever `CARGO_TARGET_DIR` is.
fn build_release(ctx: &Ctx) -> Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["build", "--release", "--locked", "-p", BINARY, "--message-format=json-render-diagnostics"])
        .current_dir(&ctx.root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo build --release --locked -p {BINARY} failed ({})", output.status));
    }
    let executable = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|msg| msg["reason"] == "compiler-artifact")
        .find_map(|msg| msg["executable"].as_str().map(PathBuf::from))
        .ok_or("cargo build reported no executable")?;
    Ok(executable)
}

/// The multiarch library directory of the build host, as Debian lays it out.
const LIBDIR: &str = "usr/lib/x86_64-linux-gnu";

fn packaging_linux(ctx: &Ctx) -> PathBuf {
    ctx.packaging().join("linux")
}
