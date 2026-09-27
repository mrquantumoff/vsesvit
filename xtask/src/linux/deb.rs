//! `Vsesvit_<v>_amd64.deb` with `dpkg-deb`.

use std::path::Path;
use std::process::Command;

use crate::Result;
use crate::ctx::{BINARY, Ctx, HOMEPAGE, MAINTAINER, SUMMARY};

use super::deps;
use super::stage::Stage;
use super::util;

pub fn build(ctx: &Ctx, stage: &Stage, artifact: &Path) -> Result {
    let installed_kib = util::tree_size(&stage.root)?.div_ceil(1024);
    let control = format!(
        "Package: {BINARY}\n\
         Version: {version}\n\
         Architecture: amd64\n\
         Maintainer: {MAINTAINER}\n\
         Installed-Size: {installed_kib}\n\
         Depends: {depends}\n\
         Recommends: {recommends}\n\
         Section: web\n\
         Priority: optional\n\
         Homepage: {HOMEPAGE}\n\
         Description: {SUMMARY}\n \
         Vsesvit is a web browser built on WebKitGTK. It installs extensions from the\n \
         Chrome Web Store and keeps bookmarks, history, open tabs and settings in a\n \
         sync-ready profile.\n",
        version = ctx.version,
        depends = deps::required(|d| d.deb).join(", "),
        recommends = deps::optional(|d| d.deb).join(", "),
    );
    util::write(&stage.root.join("DEBIAN/control"), control)?;
    util::run(Command::new("dpkg-deb").args(["--root-owner-group", "--build"]).arg(&stage.root).arg(artifact))
}
