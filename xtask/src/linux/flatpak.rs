//! `Vsesvit_<v>_x86_64.flatpak` from `packaging/flatpak/<APP_ID>.yml` with `flatpak-builder
//! --user`, then `flatpak build-bundle`. The manifest builds offline from
//! `cargo-sources.json`, regenerated here from `Cargo.lock` in the format of
//! flatpak-builder-tools' `flatpak-cargo-generator`, so the same manifest is Flathub-ready.

use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::Result;
use crate::ctx::{APP_ID, Ctx};

use super::util;

const FLATHUB: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

pub fn build(ctx: &Ctx, artifact: &Path) -> Result {
    let dir = ctx.packaging().join("flatpak");
    write_cargo_sources(&ctx.root.join("Cargo.lock"), &dir.join("cargo-sources.json"))?;
    let work = ctx.work.join("flatpak");
    util::create_dir_all(&work)?;
    util::run(Command::new("flatpak").args(["remote-add", "--user", "--if-not-exists", "flathub", FLATHUB]))?;
    util::run(
        Command::new("flatpak-builder")
            .args(["--user", "--force-clean", "--disable-rofiles-fuse", "--install-deps-from=flathub"])
            .arg(format!("--state-dir={}", super::display(&work.join("state"))))
            .arg(format!("--repo={}", super::display(&work.join("repo"))))
            .arg(work.join("build"))
            .arg(dir.join(format!("{APP_ID}.yml"))),
    )?;
    util::run(Command::new("flatpak").arg("build-bundle").arg(work.join("repo")).arg(artifact).arg(APP_ID))
}

/// One `archive` source per crates.io package in the lock file plus its `.cargo-checksum.json`,
/// and the `cargo/config.toml` that points cargo at the vendored copies.
fn write_cargo_sources(lock: &Path, out: &Path) -> Result {
    let mut sources = Vec::new();
    for package in packages(&util::read_to_string(lock)?)? {
        let (Some(name), Some(version)) = (package.get("name"), package.get("version")) else {
            return Err(format!("{}: package without name or version", lock.display()));
        };
        match package.get("source").map(String::as_str) {
            None => continue,
            Some(CRATES_IO) => {}
            Some(other) => return Err(format!("{name} {version} comes from {other}; only crates.io sources are vendored")),
        }
        let checksum = package.get("checksum").ok_or_else(|| format!("{name} {version} has no checksum"))?;
        let dest = format!("cargo/vendor/{name}-{version}");
        sources.push(json!({
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
            "sha256": checksum,
            "dest": dest,
        }));
        sources.push(json!({
            "type": "inline",
            "contents": json!({ "package": checksum, "files": {} }).to_string(),
            "dest": dest,
            "dest-filename": ".cargo-checksum.json",
        }));
    }
    sources.push(json!({
        "type": "inline",
        "contents": "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"cargo/vendor\"\n",
        "dest": "cargo",
        "dest-filename": "config.toml",
    }));
    let text = serde_json::to_string_pretty(&sources).map_err(|e| e.to_string())? + "\n";
    if util::read_to_string(out).is_ok_and(|current| current == text) {
        return Ok(());
    }
    util::write(out, text)
}

/// The `[[package]]` tables of a Cargo.lock as key -> unquoted value maps.
fn packages(lock: &str) -> Result<Vec<std::collections::HashMap<String, String>>> {
    let mut packages = Vec::new();
    for line in lock.lines() {
        if line == "[[package]]" {
            packages.push(std::collections::HashMap::new());
        } else if let (Some(package), Some((key, value))) = (packages.last_mut(), line.split_once(" = "))
            && let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"'))
        {
            package.insert(key.to_owned(), value.to_owned());
        }
    }
    Ok(packages)
}
