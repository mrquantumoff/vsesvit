use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::Result;

pub const APP_ID: &str = "dev.mrquantumoff.vsesvit";
pub const PRODUCT: &str = "Vsesvit";
pub const BINARY: &str = "vsesvit";
pub const SUMMARY: &str = "A web browser with Chrome Web Store extensions";
pub const HOMEPAGE: &str = "https://github.com/mrquantumoff/vsesvit";
pub const MAINTAINER: &str = "Demir Yerli <demiryerli@gmail.com>";
pub const LICENSE: &str = "LicenseRef-Proprietary";

/// A package format. Its variant string is Tauri's `{{bundle_type}}` and the suffix of its
/// `latest.json` platform key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Nsis,
    Deb,
    Rpm,
    Pacman,
    AppImage,
    Flatpak,
}

impl Format {
    pub const ALL: [Format; 6] = [Format::Nsis, Format::Deb, Format::Rpm, Format::Pacman, Format::AppImage, Format::Flatpak];

    pub fn variant(self) -> &'static str {
        match self {
            Format::Nsis => "nsis",
            Format::Deb => "deb",
            Format::Rpm => "rpm",
            Format::Pacman => "pacman",
            Format::AppImage => "appimage",
            Format::Flatpak => "flatpak",
        }
    }

    /// File name in `target/dist/`, following the Tauri bundler's names.
    pub fn artifact_name(self, version: &str) -> String {
        match self {
            Format::Nsis => format!("{PRODUCT}_{version}_x64-setup.exe"),
            Format::Deb => format!("{PRODUCT}_{version}_amd64.deb"),
            Format::Rpm => format!("{PRODUCT}-{version}-1.x86_64.rpm"),
            Format::Pacman => format!("{BINARY}-{version}-1-x86_64.pkg.tar.zst"),
            Format::AppImage => format!("{PRODUCT}_{version}_amd64.AppImage"),
            Format::Flatpak => format!("{PRODUCT}_{version}_x86_64.flatpak"),
        }
    }

    /// `latest.json` platform keys this format is published under. Flatpak updates through
    /// Flatpak, so it has none.
    pub fn manifest_keys(self) -> &'static [&'static str] {
        match self {
            Format::Nsis => &["windows-x86_64-nsis", "windows-x86_64"],
            Format::Deb => &["linux-x86_64-deb"],
            Format::Rpm => &["linux-x86_64-rpm"],
            Format::Pacman => &["linux-x86_64-pacman"],
            Format::AppImage => &["linux-x86_64-appimage", "linux-x86_64"],
            Format::Flatpak => &[],
        }
    }
}

impl FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self> {
        Format::ALL
            .into_iter()
            .find(|f| f.variant() == s)
            .ok_or_else(|| format!("unknown format {s:?}"))
    }
}

pub struct Ctx {
    /// Workspace version, e.g. `0.1.0`.
    pub version: String,
    /// Repository root.
    pub root: PathBuf,
    /// The cargo target directory.
    pub target: PathBuf,
    /// `target/dist`: finished artifacts, their `.sig` files and `latest.json`.
    pub dist: PathBuf,
    /// `target/package`: staging trees and downloaded tools, safe to delete.
    pub work: PathBuf,
}

impl Ctx {
    pub fn new() -> Result<Ctx> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("xtask is not inside the workspace")?
            .to_path_buf();
        let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
        let ctx = Ctx {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            dist: target.join("dist"),
            work: target.join("package"),
            target,
            root,
        };
        std::fs::create_dir_all(&ctx.dist).map_err(|e| format!("{}: {e}", ctx.dist.display()))?;
        std::fs::create_dir_all(&ctx.work).map_err(|e| format!("{}: {e}", ctx.work.display()))?;
        Ok(ctx)
    }

    pub fn artifact(&self, format: Format) -> PathBuf {
        self.dist.join(format.artifact_name(&self.version))
    }

    pub fn packaging(&self) -> PathBuf {
        self.root.join("packaging")
    }
}
