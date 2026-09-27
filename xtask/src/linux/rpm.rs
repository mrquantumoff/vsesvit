//! `Vsesvit-<v>-1.x86_64.rpm` with `rpmbuild -bb`. The spec packages the staged tree as is:
//! nothing is compiled or stripped inside rpmbuild.

use std::path::Path;
use std::process::Command;

use crate::Result;
use crate::ctx::{APP_ID, BINARY, Ctx, Format, HOMEPAGE, LICENSE, SUMMARY};

use super::deps;
use super::stage::Stage;
use super::util;

pub fn build(ctx: &Ctx, stage: &Stage, artifact: &Path) -> Result {
    let version = Format::Rpm.package_version(&ctx.version);
    let topdir = ctx.work.join("rpm/topdir");
    util::fresh_dir(&topdir)?;
    for sub in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
        util::create_dir_all(&topdir.join(sub))?;
    }
    let spec = format!(
        "%global debug_package %{{nil}}\n\
         %global _build_id_links none\n\
         %global __os_install_post %{{nil}}\n\
         \n\
         Name: {BINARY}\n\
         Version: {version}\n\
         Release: 1\n\
         Summary: {SUMMARY}\n\
         License: {LICENSE}\n\
         URL: {HOMEPAGE}\n\
         AutoReqProv: no\n\
         {requires}\n\
         {recommends}\n\
         \n\
         %description\n\
         Vsesvit is a web browser built on WebKitGTK. It installs extensions from the\n\
         Chrome Web Store and keeps bookmarks, history, open tabs and settings in a\n\
         sync-ready profile.\n\
         \n\
         %install\n\
         cp -a '{root}/.' %{{buildroot}}/\n\
         \n\
         %files\n\
         /usr/bin/{BINARY}\n\
         /usr/lib/{BINARY}/\n\
         /usr/share/applications/{APP_ID}.desktop\n\
         /usr/share/metainfo/{APP_ID}.metainfo.xml\n\
         /usr/share/icons/hicolor/*/apps/{APP_ID}.*\n",
        requires = deps::required(|d| d.rpm).iter().map(|d| format!("Requires: {d}")).collect::<Vec<_>>().join("\n"),
        recommends = deps::optional(|d| d.rpm).iter().map(|d| format!("Recommends: {d}")).collect::<Vec<_>>().join("\n"),
        root = super::display(&stage.root),
    );
    let spec_path = topdir.join("SPECS").join(format!("{BINARY}.spec"));
    util::write(&spec_path, spec)?;
    // rpm 6 writes v6 packages by default, which rpm 4 hosts cannot read.
    util::run(
        Command::new("rpmbuild")
            .args(["-bb", "--target", "x86_64", "--define", "_rpmformat 4"])
            .arg("--define")
            .arg(format!("_topdir {}", super::display(&topdir)))
            .arg("--define")
            .arg(format!("_dbpath {}", super::display(&topdir.join("rpmdb"))))
            .arg(&spec_path),
    )?;
    let built = topdir.join("RPMS/x86_64").join(format!("{BINARY}-{version}-1.x86_64.rpm"));
    util::copy(&built, artifact)
}
