//! Owned by W0.4.
//! The shell's icons (UA section 3.4): Lucide 1.43.0 SVGs from `gpui-kit-assets` 0.7.3, ISC licence
//! (`assets/icons/LICENSE-LUCIDE`). Only the icons the shell uses are copied, unmodified, into
//! `assets/icons/`. `star-fill` is gpui-kit-assets' filled copy of Lucide's star (Apache-2.0).
//! Use one as `Icon::new(VsIcon::Back)`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use gpui_kit_assets::IconNamed;

macro_rules! vs_icons {
    ($($(#[$doc:meta])* $variant:ident => $file:literal,)*) => {
        /// An icon of the shell, named for what it means; one Lucide file each.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        pub enum VsIcon {
            $($(#[$doc])* $variant,)*
            /// A download's or a file's type.
            File(FileType),
        }

        impl VsIcon {
            /// Every icon, the file types included.
            pub const ALL: [VsIcon; [$(stringify!($variant)),*].len() + FileType::ALL.len()] = {
                let plain = [$(VsIcon::$variant),*];
                let mut all = [VsIcon::Back; [$(stringify!($variant)),*].len() + FileType::ALL.len()];
                let mut i = 0;
                while i < plain.len() {
                    all[i] = plain[i];
                    i += 1;
                }
                while i < all.len() {
                    all[i] = VsIcon::File(FileType::ALL[i - plain.len()]);
                    i += 1;
                }
                all
            };

            /// The asset path, `icons/<lucide name>.svg`.
            pub fn file(self) -> &'static str {
                match self {
                    $(VsIcon::$variant => concat!("icons/", $file, ".svg"),)*
                    VsIcon::File(kind) => kind.file(),
                }
            }
        }
    };
}

vs_icons! {
    Back => "arrow-left",
    Forward => "arrow-right",
    Reload => "rotate-cw",
    Stop => "x",
    Home => "house",
    SidebarLeft => "panel-left",
    SidebarRight => "panel-right",
    /// The private window's mark.
    Private => "hat-glasses",
    /// New tab, and every "add".
    NewTab => "plus",
    Download => "download",
    Menu => "menu",
    /// Extensions.
    Puzzle => "puzzle",
    Search => "search",
    Secure => "lock",
    Insecure => "lock-open",
    Info => "info",
    Star => "star",
    StarFilled => "star-fill",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
    /// Tab close, and every dismiss.
    Close => "x",
    Pin => "pin",
    Volume => "volume-2",
    Muted => "volume-x",
    Camera => "video",
    Microphone => "mic",
    Screen => "screen-share",
    /// The favicon of a page that has none.
    Globe => "globe",
    Folder => "folder",
    FolderOpen => "folder-open",
    FolderNew => "folder-plus",
    /// Also a submenu's arrow.
    ChevronRight => "chevron-right",
    /// Also find's "next".
    ChevronDown => "chevron-down",
    /// Find's "previous".
    ChevronUp => "chevron-up",
    /// A history row.
    History => "clock",
    Bookmark => "bookmark",
    SwitchTab => "app-window",
    /// A chosen swatch.
    Check => "check",
    Edit => "pencil",
    Trash => "trash",
    Reset => "undo-2",
    /// Open a profile in its own window.
    OpenWindow => "square-arrow-out-up-right",
    Shield => "shield",
    Settings => "settings",
    Palette => "palette",
    Keyboard => "keyboard",
    Warning => "triangle-alert",
    Error => "circle-x",
    Password => "key-round",
    Package => "package",
    More => "ellipsis-vertical",
    MoveTo => "folder-input",
    CancelDownload => "circle-stop",
    Print => "printer",
    /// View source.
    Code => "code",
    Copy => "copy",
    Link => "link",
}

/// What a file holds, for its icon.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FileType {
    Generic,
    Archive,
    Image,
    Video,
    Audio,
    Text,
    Code,
}

impl FileType {
    pub const ALL: [FileType; 7] =
        [Self::Generic, Self::Archive, Self::Image, Self::Video, Self::Audio, Self::Text, Self::Code];

    fn file(self) -> &'static str {
        match self {
            Self::Generic => "icons/file.svg",
            Self::Archive => "icons/file-archive.svg",
            Self::Image => "icons/file-image.svg",
            Self::Video => "icons/file-play.svg",
            Self::Audio => "icons/file-music.svg",
            Self::Text => "icons/file-text.svg",
            Self::Code => "icons/file-code.svg",
        }
    }
}

impl IconNamed for VsIcon {
    fn path(self) -> SharedString {
        self.file().into()
    }
}

macro_rules! embed {
    ($($name:literal),* $(,)?) => {
        &[$((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../../assets/icons/", $name, ".svg"))),)*]
    };
}

/// Each file in `assets/icons/`, by asset path.
static FILES: &[(&str, &[u8])] = embed![
    "app-window", "arrow-left", "arrow-right", "bookmark", "check", "chevron-down", "chevron-right",
    "chevron-up", "circle-stop", "circle-x", "clock", "code", "copy", "download", "ellipsis-vertical",
    "file", "file-archive", "file-code", "file-image", "file-music", "file-play", "file-text", "folder",
    "folder-input", "folder-open", "folder-plus", "globe", "hat-glasses", "house", "info", "key-round",
    "keyboard", "link", "lock", "lock-open", "menu", "mic", "package", "palette", "panel-left",
    "panel-right", "pencil", "pin", "plus", "printer", "puzzle", "rotate-cw", "screen-share", "search",
    "settings", "shield", "square-arrow-out-up-right", "star", "star-fill", "trash", "triangle-alert",
    "undo-2", "video", "volume-2", "volume-x", "x", "zoom-in", "zoom-out",
];

/// The application's asset source: the shell's icons, then gpui-kit-assets' component icons
/// (without embedding the whole Lucide set, about 1 MiB).
#[derive(Copy, Clone, Debug, Default)]
pub struct VsesvitAssets;

impl AssetSource for VsesvitAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match FILES.iter().find(|(file, _)| *file == path) {
            Some((_, bytes)) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_kit_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all: Vec<SharedString> = gpui_kit_assets::Assets.list(path)?;
        all.extend(FILES.iter().filter(|(file, _)| file.starts_with(path)).map(|(file, _)| SharedString::from(*file)));
        all.sort();
        all.dedup();
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_icon_has_its_file_and_every_file_an_icon() {
        let used: BTreeSet<&str> = VsIcon::ALL.iter().map(|icon| icon.file()).collect();
        let shipped: BTreeSet<&str> = FILES.iter().map(|(file, _)| *file).collect();
        assert_eq!(used, shipped);
        assert_eq!(shipped.len(), FILES.len(), "a file is embedded twice");
        let on_disk: BTreeSet<String> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".svg"))
            .map(|name| format!("icons/{name}"))
            .collect();
        assert_eq!(on_disk, shipped.iter().map(|s| s.to_string()).collect());
    }

    #[test]
    fn the_files_are_unmodified_copies_of_gpui_kit_assets() {
        for (file, bytes) in FILES {
            let upstream = gpui_kit_assets::AllAssets.load(file).unwrap().unwrap();
            assert_eq!(*bytes, &*upstream, "{file}");
        }
    }

    #[test]
    fn the_source_falls_back_to_the_component_icons() {
        assert!(VsesvitAssets.load("icons/arrow-left.svg").unwrap().is_some());
        assert!(VsesvitAssets.load("icons/window-close.svg").unwrap().is_some(), "a component icon");
        assert!(VsesvitAssets.load("icons/no-such-icon.svg").is_err());
        let listed = VsesvitAssets.list("icons/").unwrap();
        assert!(listed.iter().any(|p| p == "icons/hat-glasses.svg"));
        assert!(listed.iter().any(|p| p == "icons/window-close.svg"));
    }
}
