//! A Debian sysroot for the AppImage: the runtime packages and their dependency closure,
//! fetched with `apt-get download` and extracted with `dpkg-deb -x` under
//! `target/package/appimage/sysroot`. apt keeps its package lists in a private directory there,
//! so nothing needs root and the lists are fresh even when the host's are stale. Packages every
//! host provides (glibc, Mesa, X11, ...) are not followed; the AppImage exclude list decides per
//! library what is bundled anyway.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Result;
use crate::ctx::Ctx;

use super::util;

/// Packages whose dependencies are never followed. A trailing `*` matches a prefix.
const HOST_PROVIDED: &[&str] = &[
    "libc6", "libc-bin", "libgcc-s1", "libstdc++6", "gcc-*", "mesa-*", "libgl1", "libglx*", "libegl*",
    "libgles*", "libglvnd*", "libopengl*", "libglapi*", "libgbm*", "libdrm*", "libx11*", "fontconfig*",
    "libfreetype6", "libasound2*", "libpipewire*", "systemd", "systemd-*", "dbus*", "perl*", "python3*",
    "init-system-helpers", "libpam*", "adduser", "passwd", "login*", "base-*", "debconf", "ucf",
    "media-types", "sensible-utils", "util-linux", "mount", "libuuid1", "libcrypt1", "libgpg-error0",
    "libcom-err2", "libgmp10", "libnss*", "netbase", "ca-certificates", "openssl",
    "libcap2-bin", "xdg-user-dirs", "shared-mime-info", "libglib2.0-data", "dconf*", "at-spi2-*",
    "libatspi*", "libatk*", "hunspell-*", "cups-*", "avahi-*", "gtk-update-icon-cache", "libgtk-3-*",
];

/// The sysroot with `packages` and their closure extracted. Only re-extracted when the set of
/// downloaded .deb files changed.
pub fn prepare(ctx: &Ctx, packages: &[&str]) -> Result<PathBuf> {
    let dir = ctx.work.join("appimage");
    let debs = dir.join("debs");
    let sysroot = dir.join("sysroot");
    util::create_dir_all(&debs)?;
    let apt = Apt::new(&dir.join("apt"))?;

    let closure = apt.closure(packages)?;
    println!("sysroot: {} packages", closure.len());
    let wanted = apt.deb_files(&closure)?;
    let missing: Vec<&str> = wanted
        .iter()
        .filter(|(_, file)| !debs.join(file).is_file())
        .map(|(pkg, _)| pkg.as_str())
        .collect();
    if !missing.is_empty() {
        println!("downloading {} packages", missing.len());
        util::run(apt.command("apt-get").arg("download").args(&missing).current_dir(&debs))?;
    }

    let stamp_path = sysroot.join(".debs");
    let stamp = wanted.iter().map(|(_, file)| file.as_str()).collect::<Vec<_>>().join("\n");
    if util::read_to_string(&stamp_path).is_ok_and(|s| s == stamp) {
        return Ok(sysroot);
    }
    util::fresh_dir(&sysroot)?;
    for (_, file) in &wanted {
        util::run(Command::new("dpkg-deb").arg("-x").arg(debs.join(file)).arg(&sysroot))?;
    }
    util::write(&stamp_path, stamp)?;
    Ok(sysroot)
}

/// apt with its lists and cache under a directory of ours, refreshed once per run.
struct Apt {
    dir: PathBuf,
}

impl Apt {
    fn new(dir: &Path) -> Result<Apt> {
        util::create_dir_all(&dir.join("lists/partial"))?;
        util::create_dir_all(&dir.join("cache/archives/partial"))?;
        let apt = Apt { dir: dir.to_path_buf() };
        util::run(apt.command("apt-get").arg("update").stdout(Stdio::null()))?;
        Ok(apt)
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.arg("-o")
            .arg(format!("Dir::State::Lists={}", self.dir.join("lists").display()))
            .arg("-o")
            .arg(format!("Dir::Cache={}", self.dir.join("cache").display()));
        cmd
    }

    /// `packages` and their transitive `Depends`/`Pre-Depends`, first alternative of each `|`
    /// group, stopping at host-provided packages.
    fn closure(&self, packages: &[&str]) -> Result<BTreeSet<String>> {
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<String> = packages.iter().map(|p| (*p).to_owned()).collect();
        while let Some(package) = queue.pop_front() {
            if !seen.insert(package.clone()) {
                continue;
            }
            let listing = util::output(
                self.command("apt-cache")
                    .args(["depends", "--no-recommends", "--no-suggests", "--no-conflicts", "--no-breaks", "--no-replaces", "--no-enhances"])
                    .arg(&package),
            )?;
            // apt-cache prints an alternative group as ` |Depends: a` lines closed by a
            // `  Depends: b` line; only the first alternative is followed.
            let mut in_alternatives = false;
            for line in listing.lines().skip(1) {
                let Some((kind, dep)) = line.trim_start_matches([' ', '|']).split_once(": ") else { continue };
                if !matches!(kind, "Depends" | "PreDepends") {
                    continue;
                }
                let take = !in_alternatives;
                in_alternatives = line.starts_with(" |");
                if !take || dep.starts_with('<') || host_provided(dep) {
                    continue;
                }
                queue.push_back(dep.to_owned());
            }
        }
        Ok(seen)
    }

    /// (package, .deb file name) for every package, from apt's own naming.
    fn deb_files(&self, packages: &BTreeSet<String>) -> Result<Vec<(String, String)>> {
        let listing = util::output(self.command("apt-get").args(["download", "--print-uris"]).args(packages))?;
        let mut files = Vec::new();
        for line in listing.lines() {
            let mut fields = line.split_whitespace();
            let (Some(_url), Some(file)) = (fields.next(), fields.next()) else { continue };
            let package = file.split('_').next().unwrap_or(file).to_owned();
            files.push((package, file.to_owned()));
        }
        if files.len() != packages.len() {
            return Err(format!("apt-get download --print-uris listed {} of {} packages", files.len(), packages.len()));
        }
        Ok(files)
    }
}

fn host_provided(package: &str) -> bool {
    HOST_PROVIDED.iter().any(|pattern| match pattern.strip_suffix('*') {
        Some(prefix) => package.starts_with(prefix),
        None => package == *pattern,
    })
}
