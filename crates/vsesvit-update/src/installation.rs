//! How this copy of Vsesvit was installed, read from the `package-format` marker each package
//! puts next to the real executable.

use std::path::{Path, PathBuf};

const MARKER: &str = "package-format";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installation {
    /// No marker, or one this build does not know: a `cargo run`. Never updates itself.
    Unpackaged,
    Nsis,
    Deb,
    Rpm,
    Pacman,
    /// `image` is `$APPIMAGE`, the file the new image replaces.
    AppImage { image: PathBuf },
    /// Flatpak updates it.
    Flatpak,
}

/// The kind of file an update artifact is. Each self-updating installation takes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Nsis,
    Deb,
    Rpm,
    Pacman,
    AppImage,
}

impl Installation {
    /// Reads the marker next to the canonicalized `current_exe()` (on Linux `bin/vsesvit` is a
    /// symlink to `lib/vsesvit/vsesvit`, where the marker is) and `$APPIMAGE`.
    pub fn detect() -> Installation {
        let Some(exe_dir) = std::env::current_exe()
            .and_then(|exe| exe.canonicalize())
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        else {
            return Installation::Unpackaged;
        };
        let marker = std::fs::read_to_string(exe_dir.join(MARKER)).ok();
        Installation::from_parts(marker.as_deref(), std::env::var_os("APPIMAGE").map(PathBuf::from))
    }

    pub fn from_parts(marker: Option<&str>, appimage_env: Option<PathBuf>) -> Installation {
        match marker.map(str::trim) {
            Some("nsis") => Installation::Nsis,
            Some("deb") => Installation::Deb,
            Some("rpm") => Installation::Rpm,
            Some("pacman") => Installation::Pacman,
            Some("appimage") => match appimage_env.filter(|image| !image.as_os_str().is_empty()) {
                Some(image) => Installation::AppImage { image },
                None => Installation::Unpackaged,
            },
            Some("flatpak") => Installation::Flatpak,
            Some(unknown) => {
                log::warn!("unknown {MARKER} marker {unknown:?}; treating this build as unpackaged");
                Installation::Unpackaged
            }
            None => Installation::Unpackaged,
        }
    }

    /// The marker string, which is also the protocol's `{{bundle_type}}` and the last part of a
    /// static-format platform key. `None` for an unpackaged build.
    pub fn variant(&self) -> Option<&'static str> {
        match self {
            Installation::Unpackaged => None,
            Installation::Flatpak => Some("flatpak"),
            other => other.format().map(Format::variant),
        }
    }

    pub fn self_updates(&self) -> bool {
        self.format().is_some()
    }

    /// The artifact this installation installs, `None` when it does not update itself.
    pub fn format(&self) -> Option<Format> {
        match self {
            Installation::Unpackaged | Installation::Flatpak => None,
            Installation::Nsis => Some(Format::Nsis),
            Installation::Deb => Some(Format::Deb),
            Installation::Rpm => Some(Format::Rpm),
            Installation::Pacman => Some(Format::Pacman),
            Installation::AppImage { .. } => Some(Format::AppImage),
        }
    }
}

impl Format {
    pub(crate) const ALL: [Format; 5] = [Format::Nsis, Format::Deb, Format::Rpm, Format::Pacman, Format::AppImage];

    pub fn variant(self) -> &'static str {
        match self {
            Format::Nsis => "nsis",
            Format::Deb => "deb",
            Format::Rpm => "rpm",
            Format::Pacman => "pacman",
            Format::AppImage => "appimage",
        }
    }

    /// The downloaded file's name ends with this, which is what the installing tool expects.
    pub(crate) fn file_suffix(self) -> &'static str {
        match self {
            Format::Nsis => "-setup.exe",
            Format::Deb => ".deb",
            Format::Rpm => ".rpm",
            Format::Pacman => ".pkg.tar.zst",
            Format::AppImage => ".AppImage",
        }
    }

    /// Whether `bytes` start the way a file of this format starts.
    pub fn matches_magic(self, bytes: &[u8]) -> bool {
        match self {
            Format::Nsis => bytes.starts_with(b"MZ"),
            Format::Deb => bytes.starts_with(b"!<arch>\ndebian-binary"),
            Format::Rpm => bytes.starts_with(&[0xED, 0xAB, 0xEE, 0xDB]),
            Format::Pacman => bytes.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]),
            Format::AppImage => bytes.starts_with(b"\x7FELF") && bytes.get(8..11) == Some(b"AI\x02".as_slice()),
        }
    }
}
