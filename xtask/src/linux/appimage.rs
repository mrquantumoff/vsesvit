//! `Vsesvit_<v>_amd64.AppImage`: the staged tree plus GTK, libadwaita, WebKitGTK with its helper
//! processes, GStreamer with its plugins, the TLS GIO module, the SVG pixbuf loader, GLib schemas
//! and the Adwaita icons, copied from a Debian sysroot by walking `DT_NEEDED`, then packed onto
//! the pinned type-2 runtime with `mksquashfs`.
//!
//! WebKitGTK finds `WebKitWebProcess` and friends through a directory compiled into
//! `libwebkitgtk-6.0.so` (`PKGLIBEXECDIR` in `ProcessExecutablePathGLib.cpp`; the
//! `WEBKIT_EXEC_PATH` override exists only in developer-mode builds). Like Tauri's bundler, we
//! rewrite that directory in the library to a relative one and `AppRun` runs the browser from
//! `$APPDIR/usr`, so the helpers resolve inside the image. The relative directory starts with
//! `./wk`, a link to `usr` itself, rather than `././`, because the web process sandbox binds it
//! at the same path under its own root: `/wk/...` can be made there, `/lib/...` cannot. Inside
//! the sandbox bubblewrap keeps the working directory, which the browser adds to it with
//! `$APPDIR`. Every other path stays, among them `/usr/bin/bwrap` and the host directories the
//! sandbox is built from.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::Result;
use crate::ctx::{APP_ID, BINARY, Ctx};
use crate::icons;

use super::stage::{Stage, desktop_file};
use super::util::{self, Download};
use super::{LIBDIR, deps, elf, sysroot};

const RUNTIME: Download = Download {
    url: "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64",
    sha256: "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
    file_name: "appimage-runtime-x86_64-20251108",
};

/// Debian packages bundled on top of the runtime dependency table.
const EXTRA_PACKAGES: &[&str] = &[
    "gstreamer1.0-plugins-base",
    "glib-networking",
    "librsvg2-common",
    "gsettings-desktop-schemas",
    "adwaita-icon-theme",
    "hicolor-icon-theme",
];

const WEBKIT_HELPERS: &[&str] =
    &["WebKitWebProcess", "WebKitNetworkProcess", "WebKitGPUProcess", "injected-bundle/libwebkitgtkinjectedbundle.so"];

/// GStreamer plugins the browser cannot do without. Any other plugin whose libraries the sysroot
/// cannot satisfy (the GTK 3 sink, LV2 hosts, ML inference, ...) is left out with a note.
const REQUIRED_GST_PLUGINS: &[&str] = &[
    "libgstapp.so",
    "libgstaudioconvert.so",
    "libgstaudioparsers.so",
    "libgstaudioresample.so",
    "libgstautodetect.so",
    "libgstcoreelements.so",
    "libgstdav1d.so",
    "libgstisomp4.so",
    "libgstlibav.so",
    "libgstmatroska.so",
    "libgstogg.so",
    "libgstopus.so",
    "libgstplayback.so",
    "libgstpulseaudio.so",
    "libgsttypefindfunctions.so",
    "libgstvideoconvertscale.so",
    "libgstvideoparsersbad.so",
    "libgstvorbis.so",
    "libgstvpx.so",
];

/// Libraries every AppImage leaves to the host (pkg2appimage's list).
const EXCLUDE_LIST: &str = include_str!("../../../packaging/linux/appimage/excludelist");

pub fn build(ctx: &Ctx, stage: &Stage, artifact: &Path) -> Result {
    let packages: Vec<&str> = deps::required(|d| d.deb).into_iter().chain(EXTRA_PACKAGES.iter().copied()).collect();
    let sysroot = sysroot::prepare(ctx, &packages)?;
    let appdir = ctx.work.join("appimage/AppDir");
    util::fresh_dir(&appdir)?;
    util::copy_tree(&stage.root, &appdir)?;

    let mut bundle = Bundle::new(&sysroot, &appdir)?;
    bundle.add_dependencies(&appdir.join("usr/lib").join(BINARY).join(BINARY))?;
    for helper in WEBKIT_HELPERS {
        bundle.add(&format!("{LIBDIR}/webkitgtk-6.0/{helper}"))?;
    }
    bundle.add(&format!("{LIBDIR}/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner"))?;
    bundle.add(&format!("{LIBDIR}/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders"))?;
    for dir in [format!("{LIBDIR}/gio/modules"), format!("{LIBDIR}/gdk-pixbuf-2.0/2.10.0/loaders")] {
        for plugin in bundle.plugins_in(&dir)? {
            bundle.add(&plugin)?;
        }
    }
    let gst_dir = format!("{LIBDIR}/gstreamer-1.0");
    for plugin in bundle.plugins_in(&gst_dir)? {
        let name = plugin.rsplit('/').next().unwrap_or(&plugin);
        if REQUIRED_GST_PLUGINS.contains(&name) {
            bundle.add(&plugin)?;
        } else {
            bundle.add_if_satisfiable(&plugin)?;
        }
    }
    bundle.finish()?;

    let schemas = appdir.join("usr/share/glib-2.0/schemas");
    util::copy_tree(&sysroot.join("usr/share/glib-2.0/schemas"), &schemas)?;
    util::run(Command::new("glib-compile-schemas").arg(&schemas))?;
    util::copy_tree(&sysroot.join("usr/share/icons/Adwaita"), &appdir.join("usr/share/icons/Adwaita"))?;
    util::copy(&sysroot.join("usr/share/icons/hicolor/index.theme"), &appdir.join("usr/share/icons/hicolor/index.theme"))?;
    util::run(Command::new("gio-querymodules").arg(appdir.join(LIBDIR).join("gio/modules")))?;
    patch_webkit_paths(&appdir.join(LIBDIR).join("libwebkitgtk-6.0.so.4"))?;
    util::symlink(Path::new("."), &appdir.join("usr").join(HELPERS_LINK))?;

    let apprun = appdir.join("AppRun");
    util::copy(&super::packaging_linux(ctx).join("appimage/AppRun"), &apprun)?;
    util::set_mode(&apprun, 0o755)?;
    util::copy(&appdir.join("usr/share/applications").join(desktop_file()), &appdir.join(desktop_file()))?;
    util::set_mode(&appdir.join(desktop_file()), 0o644)?;
    util::write(&appdir.join(format!("{APP_ID}.png")), icons::png(256)?)?;
    util::symlink(Path::new(&format!("{APP_ID}.png")), &appdir.join(".DirIcon"))?;

    let squashfs = ctx.work.join("appimage/AppDir.squashfs");
    let _ = std::fs::remove_file(&squashfs);
    util::run(
        Command::new("mksquashfs")
            .arg(&appdir)
            .arg(&squashfs)
            .args(["-root-owned", "-noappend", "-no-xattrs", "-comp", "zstd", "-Xcompression-level", "19", "-quiet"]),
    )?;
    let runtime = RUNTIME.fetch(&ctx.work.join("tools"))?;
    let mut out = std::fs::File::create(artifact).map_err(|e| format!("{}: {e}", artifact.display()))?;
    out.write_all(&util::read(&runtime)?).map_err(|e| e.to_string())?;
    let mut image = std::fs::File::open(&squashfs).map_err(|e| format!("{}: {e}", squashfs.display()))?;
    std::io::copy(&mut image, &mut out).map_err(|e| e.to_string())?;
    util::set_mode(artifact, 0o755)
}

/// Copies files from the sysroot into the AppDir, following their `DT_NEEDED` closure. Seeds
/// (executables, plugins) keep their path; libraries land flat in the multiarch directory under
/// their soname, so one `LD_LIBRARY_PATH` entry finds them wherever Debian had them.
struct Bundle<'a> {
    sysroot: &'a Path,
    appdir: &'a Path,
    excluded: HashSet<&'static str>,
    /// soname -> sysroot-relative path; the first library directory wins.
    index: HashMap<String, String>,
    copied: HashSet<String>,
    /// unresolved soname -> files that need it.
    missing: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> Bundle<'a> {
    fn new(sysroot: &'a Path, appdir: &'a Path) -> Result<Self> {
        let excluded = EXCLUDE_LIST
            .lines()
            .map(|line| line.split('#').next().unwrap_or("").trim())
            .filter(|line| !line.is_empty())
            .collect();
        let mut index = HashMap::new();
        for dir in [LIBDIR, "usr/lib"] {
            if sysroot.join(dir).is_dir() {
                for rel in util::walk(&sysroot.join(dir))? {
                    let rel = format!("{dir}/{}", rel.to_string_lossy());
                    let name = rel.rsplit('/').next().unwrap_or(&rel).to_owned();
                    if name.contains(".so") {
                        index.entry(name).or_insert(rel);
                    }
                }
            }
        }
        Ok(Bundle { sysroot, appdir, excluded, index, copied: HashSet::new(), missing: BTreeMap::new() })
    }

    /// Copies `rel` to the same path and what it links against.
    fn add(&mut self, rel: &str) -> Result {
        self.place(rel, rel)
    }

    /// Adds a plugin only when the sysroot has every library it needs; a plugin left out is
    /// reported but does not fail the build.
    fn add_if_satisfiable(&mut self, rel: &str) -> Result {
        let unresolved = self.unresolved(rel)?;
        if unresolved.is_empty() {
            self.add(rel)
        } else {
            println!("leaving out {rel}: needs {}", unresolved.into_iter().collect::<Vec<_>>().join(", "));
            Ok(())
        }
    }

    /// Every `*.so` in the sysroot directory `rel`, sorted.
    fn plugins_in(&self, rel: &str) -> Result<Vec<String>> {
        let dir = self.sysroot.join(rel);
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".so"))
            .map(|name| format!("{rel}/{name}"))
            .collect();
        names.sort();
        Ok(names)
    }

    /// Copies what an already placed file links against.
    fn add_dependencies(&mut self, file: &Path) -> Result {
        let needed_by = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for soname in elf::needed(file)?.unwrap_or_default() {
            if self.excluded.contains(soname.as_str()) {
                continue;
            }
            match self.index.get(&soname).cloned() {
                Some(rel) => self.place(&rel, &format!("{LIBDIR}/{soname}"))?,
                None => {
                    self.missing.entry(soname).or_default().insert(needed_by.clone());
                }
            }
        }
        Ok(())
    }

    fn place(&mut self, from_rel: &str, to_rel: &str) -> Result {
        if !self.copied.insert(to_rel.to_owned()) {
            return Ok(());
        }
        let to = self.appdir.join(to_rel);
        util::copy(&self.sysroot.join(from_rel), &to)?;
        self.add_dependencies(&to)
    }

    /// Sonames in the closure of `rel` that neither the sysroot nor the exclude list covers.
    fn unresolved(&self, rel: &str) -> Result<BTreeSet<String>> {
        let mut unresolved = BTreeSet::new();
        let mut seen = HashSet::new();
        let mut queue = vec![rel.to_owned()];
        while let Some(rel) = queue.pop() {
            if !seen.insert(rel.clone()) {
                continue;
            }
            for soname in elf::needed(&self.sysroot.join(&rel))?.unwrap_or_default() {
                if self.excluded.contains(soname.as_str()) {
                    continue;
                }
                match self.index.get(&soname) {
                    Some(dep) => queue.push(dep.clone()),
                    None => {
                        unresolved.insert(soname);
                    }
                }
            }
        }
        Ok(unresolved)
    }

    fn finish(self) -> Result {
        println!("bundled {} files from the sysroot", self.copied.len());
        if self.missing.is_empty() {
            return Ok(());
        }
        let lines: Vec<String> = self
            .missing
            .into_iter()
            .map(|(soname, users)| format!("{soname} (needed by {})", users.into_iter().collect::<Vec<_>>().join(", ")))
            .collect();
        Err(format!(
            "libraries neither in the sysroot nor on the exclude list; add their package to the sysroot or the soname to \
             packaging/linux/appimage/excludelist:\n  {}",
            lines.join("\n  ")
        ))
    }
}

fn patch_webkit_paths(library: &Path) -> Result {
    let mut data = util::read(library)?;
    let patched = relocate(&mut data);
    if patched.is_empty() {
        return Err(format!("{}: no helper directory to relocate; WebKitGTK's layout changed", library.display()));
    }
    println!("relocated in {}: {}", library.display(), patched.join(" "));
    util::write(library, data)
}

/// `usr/wk` in the image links to `usr`, so `./wk/lib/...` from `usr` is `usr/lib/...`.
const HELPERS_LINK: &str = "wk";

/// Rewrites the C strings naming WebKit's helper directory (`PKGLIBEXECDIR`) or a path in it
/// to start with `./wk` instead of `/usr`, keeping the length, so they resolve inside the
/// image. Every other path stays: the sandbox needs the host's `/usr/bin/bwrap` and
/// `/usr/bin/xdg-dbus-proxy` and binds the host's `/usr/lib`, `/usr/share`, ... into itself, and
/// the data directories name files the image does not ship. Returns the strings it rewrote, as
/// they were.
fn relocate(data: &mut [u8]) -> Vec<String> {
    let helpers = format!("/{LIBDIR}/webkitgtk-6.0");
    let mut patched = Vec::new();
    let mut at = 0;
    while let Some(found) = find(&data[at..], helpers.as_bytes()) {
        let start = at + found;
        let end = start + data[start..].iter().position(|b| *b == 0).unwrap_or(data.len() - start);
        at = start + 1;
        let path = &data[start..end];
        if (start > 0 && data[start - 1] != 0) || !matches!(path.get(helpers.len()), None | Some(b'/')) {
            continue;
        }
        patched.push(String::from_utf8_lossy(path).into_owned());
        data[start..start + 4].copy_from_slice(format!("./{HELPERS_LINK}").as_bytes());
    }
    patched
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_helper_directory_is_relocated_and_the_sandbox_paths_stay() {
        let kept = [
            "/usr/bin/bwrap",
            "/usr/bin/xdg-dbus-proxy",
            "/usr/bin",
            "/usr/lib",
            "/usr/lib64",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/local/lib",
            "/usr/local/lib64",
            "/usr/local/share",
            "/usr/share",
            "/usr/share/locale",
            "/usr/libexec/gstreamer-1.0/gst-plugin-scanner",
            "/usr/lib/x86_64-linux-gnu/webkitgtk-6.0x",
            "not/usr/lib/x86_64-linux-gnu/webkitgtk-6.0",
        ];
        let moved = ["/usr/lib/x86_64-linux-gnu/webkitgtk-6.0", "/usr/lib/x86_64-linux-gnu/webkitgtk-6.0/injected-bundle/"];
        let strings: Vec<&str> = kept.iter().chain(&moved).copied().collect();
        let mut data = format!("\0{}\0", strings.join("\0")).into_bytes();
        assert_eq!(relocate(&mut data), moved);
        let after = String::from_utf8(data).unwrap();
        let after: Vec<&str> = after.trim_matches('\0').split('\0').collect();
        assert_eq!(after[..kept.len()], kept);
        assert_eq!(after[kept.len()..], ["./wk/lib/x86_64-linux-gnu/webkitgtk-6.0", "./wk/lib/x86_64-linux-gnu/webkitgtk-6.0/injected-bundle/"]);
    }

    /// AppRun's shebang and desktop-file-validate both fail on CRLF.
    #[test]
    fn the_linux_packaging_files_have_unix_line_endings() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/linux");
        for file in util::walk(&dir).unwrap() {
            let path = dir.join(file);
            if path.is_file() {
                assert!(!util::read(&path).unwrap().contains(&b'\r'), "{} has CRLF line endings", path.display());
            }
        }
    }
}
