//! The `usr/` tree every Linux package installs (docs/design/packaging.md, "The installation
//! marker"): the stripped binary and its marker under `usr/lib/vsesvit`, the `usr/bin` symlink,
//! the desktop entry, the AppStream metainfo and the hicolor icons.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;
use crate::ctx::{APP_ID, BINARY, Ctx, Format};
use crate::icons;

use super::util;

pub struct Stage {
    /// The tree root; `usr/` is directly inside.
    pub root: PathBuf,
}

pub const MARKER: &str = "package-format";

pub fn desktop_file() -> String {
    format!("{APP_ID}.desktop")
}

pub fn metainfo_file() -> String {
    format!("{APP_ID}.metainfo.xml")
}

/// Renders the metainfo template with the release version and today's date.
pub fn metainfo(ctx: &Ctx) -> Result<String> {
    let template = util::read_to_string(&super::packaging_linux(ctx).join(metainfo_file()))?;
    let date = time::OffsetDateTime::now_utc()
        .date()
        .format(time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| e.to_string())?;
    Ok(template.replace("@VERSION@", &ctx.version).replace("@DATE@", &date))
}

/// Builds the tree under `target/package/<format>/root`, fresh on every call.
pub fn stage(ctx: &Ctx, format: Format, binary: &Path) -> Result<Stage> {
    let root = ctx.work.join(format.variant()).join("root");
    util::fresh_dir(&root)?;
    let usr = root.join("usr");

    let exe = usr.join("lib").join(BINARY).join(BINARY);
    util::copy(binary, &exe)?;
    util::run(Command::new("strip").arg("--strip-unneeded").arg(&exe))?;
    util::set_mode(&exe, 0o755)?;
    util::write(&exe.with_file_name(MARKER), format!("{}\n", format.variant()))?;
    util::symlink(Path::new("../lib").join(BINARY).join(BINARY).as_path(), &usr.join("bin").join(BINARY))?;

    let desktop = usr.join("share/applications").join(desktop_file());
    util::copy(&super::packaging_linux(ctx).join(desktop_file()), &desktop)?;
    util::set_mode(&desktop, 0o644)?;
    let metainfo_path = usr.join("share/metainfo").join(metainfo_file());
    util::write(&metainfo_path, metainfo(ctx)?)?;

    icons::write_hicolor(&usr.join("share/icons/hicolor"))?;

    validate(&desktop, &metainfo_path)?;
    Ok(Stage { root })
}

/// Runs the freedesktop validators when the host has them; a missing validator is reported, not
/// fatal, so a minimal build host still packages.
fn validate(desktop: &Path, metainfo: &Path) -> Result {
    if util::exists_on_path("desktop-file-validate") {
        util::run(Command::new("desktop-file-validate").arg(desktop))?;
    } else {
        println!("desktop-file-validate not found, desktop entry not validated");
    }
    if util::exists_on_path("appstreamcli") {
        util::run(Command::new("appstreamcli").args(["validate", "--no-net"]).arg(metainfo))?;
    } else {
        println!("appstreamcli not found, metainfo not validated");
    }
    Ok(())
}
