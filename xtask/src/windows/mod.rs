//! The NSIS installer: `packaging/windows/installer.nsi` over a staged tree.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;
use crate::ctx::{BINARY, Ctx, Format, HOMEPAGE, MAINTAINER, PRODUCT, SUMMARY};
use crate::icons;

/// Files that sit next to `vsesvit.exe` in the release build and ship with it.
/// The DLL is copied there by crates/vsesvit-winui/build.rs.
const PAYLOAD: [&str; 2] = ["vsesvit.exe", "Microsoft.Web.WebView2.Core.dll"];

/// Builds the release exe and `target/dist/Vsesvit_<v>_x64-setup.exe`.
pub fn package(ctx: &Ctx) -> Result<PathBuf> {
    if !cfg!(windows) {
        return Err("the nsis package is built on Windows".to_owned());
    }
    let makensis = find_makensis()?;
    run(Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "--release", "--locked", "-p", BINARY])
        .current_dir(&ctx.root))?;

    let release = ctx.target.join("release");
    let stage = ctx.work.join("nsis");
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    }
    std::fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    for name in PAYLOAD {
        let from = release.join(name);
        std::fs::copy(&from, stage.join(name)).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    write(&stage.join(format!("{BINARY}.ico")), &icons::ico()?)?;
    write(&stage.join("package-format"), Format::Nsis.variant().as_bytes())?;

    let artifact = ctx.artifact(Format::Nsis);
    let script = ctx.packaging().join("windows").join("installer.nsi");
    let publisher = MAINTAINER.split(" <").next().unwrap_or(MAINTAINER);
    let defines = [
        ("PRODUCTNAME", PRODUCT),
        ("BINARY", BINARY),
        ("PUBLISHER", publisher),
        ("HOMEPAGE", HOMEPAGE),
        ("DESCRIPTION", SUMMARY),
        ("VERSION", &ctx.version),
        ("VIVERSION", &file_version(&ctx.version)),
        ("STAGE", &stage.display().to_string()),
        ("OUTFILE", &artifact.display().to_string()),
    ];
    let mut makensis = Command::new(makensis);
    makensis.args(["-INPUTCHARSET", "UTF8", "-V3"]);
    for (name, value) in defines {
        makensis.arg(format!("-D{name}={value}"));
    }
    run(makensis.arg(&script))?;
    Ok(artifact)
}

/// `VIProductVersion` wants four numbers: `1.2.3-beta.1` becomes `1.2.3.0`.
fn file_version(version: &str) -> String {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts: Vec<&str> = core.split('.').collect();
    parts.resize(4, "0");
    parts.join(".")
}

fn find_makensis() -> Result<PathBuf> {
    let exe = if cfg!(windows) { "makensis.exe" } else { "makensis" };
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).map(|dir| dir.join(exe)).collect::<Vec<_>>())
        .unwrap_or_default();
    let scoop = std::env::var_os("SCOOP")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| Path::new(&home).join("scoop")));
    let installed = [
        scoop.map(|s| s.join(r"apps\nsis\current").join(exe)),
        std::env::var_os("ProgramFiles(x86)").map(|p| Path::new(&p).join(r"NSIS").join(exe)),
        std::env::var_os("ProgramFiles").map(|p| Path::new(&p).join(r"NSIS").join(exe)),
    ];
    on_path
        .into_iter()
        .chain(installed.into_iter().flatten())
        .find(|p| p.is_file())
        .ok_or_else(|| "makensis not found on PATH, in scoop or in Program Files; install NSIS 3 (`scoop install extras/nsis`)".to_owned())
}

fn write(path: &Path, bytes: &[u8]) -> Result {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn run(command: &mut Command) -> Result {
    let status = command.status().map_err(|e| format!("{}: {e}", command.get_program().display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed: {status}", command.get_program().display()))
    }
}

#[cfg(test)]
mod tests {
    use super::file_version;

    #[test]
    fn file_versions_have_four_numbers() {
        assert_eq!(file_version("0.1.0"), "0.1.0.0");
        assert_eq!(file_version("1.2.3-beta.1+build"), "1.2.3.0");
    }
}
