//! Small OS helpers the shells share: where a download came from, the Downloads folder, opening
//! a file or showing it in the file manager, and whether Vsesvit is the default browser.
//!
//! Each does the same job on Windows and Linux with that system's means. Nothing here knows a
//! toolkit or an engine, and nothing blocks the caller for long.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

/// Records where a downloaded file came from, as the system's own programs read it.
///
/// - **Windows.** The Mark of the Web, an alternate data stream `Zone.Identifier` that
///   SmartScreen and Office check when the file is opened ([`crate::downloads::zone_identifier`]).
///   A file that already has one, such as one the engine marked, keeps it. `referrer` is not
///   written.
/// - **Linux.** The extended attributes `user.xdg.origin.url` and `user.xdg.referrer.url`, as
///   Chrome writes them. A source that is not a web address leaves its attribute unwritten.
///
/// The URLs lose their credentials. The shell calls this for downloads of normal browsing only,
/// never private ones, since the mark tells where the user was.
pub fn mark_origin(file: &Path, url: &str, referrer: Option<&str>) -> io::Result<()> {
    sys::mark_origin(file, url, referrer)
}

/// The user's Downloads folder, wherever they moved it: the Windows known folder, or the
/// `xdg-user-dirs` entry on Linux, else `~/Downloads`.
pub fn downloads_dir() -> io::Result<PathBuf> {
    let dirs = directories::UserDirs::new().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
    match dirs.download_dir() {
        Some(dir) => Ok(dir.to_owned()),
        None if cfg!(windows) => Err(io::Error::new(io::ErrorKind::NotFound, "no Downloads folder")),
        None => Ok(dirs.home_dir().join("Downloads")),
    }
}

/// Opens a URL, file or folder with the program the system picks for it.
pub fn open(target: &OsStr) -> io::Result<()> {
    sys::open(target)
}

/// Shows the folder holding `file` in the file manager, with `file` selected where the file
/// manager can. On Linux it runs in the background, and a failure is logged.
pub fn show_in_folder(file: &Path) -> io::Result<()> {
    sys::show_in_folder(file)
}

/// Where Vsesvit stands as the browser the system opens links with.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DefaultBrowser {
    Vsesvit,
    Other,
    /// The system does not know Vsesvit as a browser: this copy was not installed by an installer
    /// that registers it.
    Unregistered,
}

impl DefaultBrowser {
    fn of(ours: bool, registered: bool) -> Self {
        match (registered, ours) {
            (false, _) => DefaultBrowser::Unregistered,
            (true, true) => DefaultBrowser::Vsesvit,
            (true, false) => DefaultBrowser::Other,
        }
    }
}

/// Whether Vsesvit is the default browser. Reads the system's settings on every call.
pub fn default_browser() -> DefaultBrowser {
    sys::default_browser()
}

#[cfg(windows)]
pub use sys::REGISTERED_NAME;

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::io::{self, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::DefaultBrowser;

    /// The names `packaging/windows/installer.nsi` registers Vsesvit under.
    pub const REGISTERED_NAME: &str = "Vsesvit";
    const PROG_ID: &str = "VsesvitHTML";

    pub fn mark_origin(file: &Path, url: &str, _referrer: Option<&str>) -> io::Result<()> {
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        match std::fs::File::create_new(&stream) {
            Ok(mut f) => f.write_all(crate::downloads::zone_identifier(url).as_bytes()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(Some(0)).collect()
    }

    pub fn open(target: &OsStr) -> io::Result<()> {
        let (verb, target) = (wide(OsStr::new("open")), wide(target));
        let result = unsafe {
            ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
        };
        // Up to 32 is an error code.
        match result as usize {
            0..=32 => Err(io::Error::from_raw_os_error(result as i32)),
            _ => Ok(()),
        }
    }

    pub fn show_in_folder(file: &Path) -> io::Result<()> {
        // Explorer reads its own command line: the path is quoted inside the one argument.
        let mut select = std::ffi::OsString::from("/select,\"");
        select.push(file);
        select.push("\"");
        std::process::Command::new("explorer.exe").raw_arg(select).spawn().map(drop)
    }

    pub fn default_browser() -> DefaultBrowser {
        let registered = user_registry_string(r"Software\RegisteredApplications", REGISTERED_NAME).is_some();
        let choice =
            user_registry_string(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice", "ProgId");
        DefaultBrowser::of(choice.as_deref() == Some(PROG_ID), registered)
    }

    /// A string value under `HKEY_CURRENT_USER`.
    fn user_registry_string(key: &str, value: &str) -> Option<String> {
        let (key, value) = (wide(OsStr::new(key)), wide(OsStr::new(value)));
        let read = |data: *mut u16, bytes: &mut u32| unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), data.cast(), bytes)
        };
        let mut bytes = 0;
        if read(std::ptr::null_mut(), &mut bytes) != 0 {
            return None;
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
        if read(buffer.as_mut_ptr(), &mut bytes) != 0 {
            return None;
        }
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]))
    }
}

#[cfg(not(windows))]
mod sys {
    use std::ffi::OsStr;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::DefaultBrowser;

    /// The desktop entry `packaging/linux` installs, which names Vsesvit to the system.
    const DESKTOP_ID: &str = "dev.mrquantumoff.vsesvit.desktop";

    #[cfg(target_os = "linux")]
    pub fn mark_origin(file: &Path, url: &str, referrer: Option<&str>) -> io::Result<()> {
        if let Some(origin) = crate::downloads::web_source(url) {
            xattr::set(file, "user.xdg.origin.url", origin.as_bytes())?;
        }
        if let Some(referrer) = referrer.and_then(crate::downloads::web_source) {
            xattr::set(file, "user.xdg.referrer.url", referrer.as_bytes())?;
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn mark_origin(_: &Path, _: &str, _: Option<&str>) -> io::Result<()> {
        Ok(())
    }

    /// Starts `program` and reaps it in the background, since `xdg-open` may outlive the caller's
    /// interest and a child never waited for stays a zombie.
    fn launch(program: &str, args: &[&OsStr]) -> io::Result<()> {
        let mut child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    pub fn open(target: &OsStr) -> io::Result<()> {
        launch("xdg-open", &[target])
    }

    pub fn show_in_folder(file: &Path) -> io::Result<()> {
        let file = std::path::absolute(file)?;
        let uri = url::Url::from_file_path(&file).map_err(|()| io::Error::new(io::ErrorKind::InvalidInput, "not an absolute path"))?;
        std::thread::Builder::new().name("show-in-folder".into()).spawn(move || {
            if show_items(uri.as_str()) {
                return;
            }
            // No file manager answers on the bus: open the folder without selecting.
            let folder = file.parent().unwrap_or(&file);
            if let Err(e) = Command::new("xdg-open").arg(folder).stdin(Stdio::null()).status() {
                log::warn!("show {} in its folder: {e}", file.display());
            }
        })?;
        Ok(())
    }

    /// Asks the file manager over D-Bus (`org.freedesktop.FileManager1`) to select `uri`.
    fn show_items(uri: &str) -> bool {
        Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=3000",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{uri}"))
            .arg("string:")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    pub fn default_browser() -> DefaultBrowser {
        let current = Command::new("xdg-mime")
            .args(["query", "default", "x-scheme-handler/https"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
        let ours = current.as_deref() == Some(DESKTOP_ID);
        DefaultBrowser::of(ours, ours || desktop_entry_installed())
    }

    /// Whether the system has Vsesvit's desktop entry in one of its application folders.
    fn desktop_entry_installed() -> bool {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share")));
        let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        let data_dirs = if data_dirs.is_empty() { "/usr/local/share:/usr/share".to_owned() } else { data_dirs };
        data_home
            .into_iter()
            .chain(data_dirs.split(':').map(PathBuf::from))
            .any(|dir| dir.join("applications").join(DESKTOP_ID).exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_browser_is_ours_only_when_registered() {
        assert_eq!(DefaultBrowser::of(true, true), DefaultBrowser::Vsesvit);
        assert_eq!(DefaultBrowser::of(false, true), DefaultBrowser::Other);
        assert_eq!(DefaultBrowser::of(false, false), DefaultBrowser::Unregistered);
        assert_eq!(DefaultBrowser::of(true, false), DefaultBrowser::Unregistered);
    }

    #[test]
    fn the_downloads_folder_is_an_absolute_path() {
        assert!(downloads_dir().unwrap().is_absolute());
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("vsesvit-os-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_finished_file_is_marked_once() {
        let dir = Scratch::new();
        let file = dir.0.join("setup.exe");
        std::fs::write(&file, b"MZ").unwrap();
        mark_origin(&file, "https://example.com/setup.exe", None).unwrap();
        mark_origin(&file, "https://other.example/", None).unwrap();
        let mark = std::fs::read_to_string(format!("{}:Zone.Identifier", file.display()));
        assert_eq!(mark.unwrap(), "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/setup.exe\r\n");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_download_carries_its_origin_and_referrer_as_extended_attributes() {
        let dir = Scratch::new();
        let file = dir.0.join("report.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        let marked = mark_origin(&file, "https://user:secret@example.com/files/report.pdf", Some("https://example.com/files/"));
        if let Err(e) = &marked
            && (e.kind() == io::ErrorKind::Unsupported || e.raw_os_error() == Some(95))
        {
            eprintln!("skipped: {} has no user extended attributes", dir.0.display());
            return;
        }
        marked.unwrap();
        let read = |name| String::from_utf8(xattr::get(&file, name).unwrap().unwrap()).unwrap();
        assert_eq!(read("user.xdg.origin.url"), "https://example.com/files/report.pdf");
        assert_eq!(read("user.xdg.referrer.url"), "https://example.com/files/");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_source_that_is_not_a_web_address_leaves_no_attribute() {
        let dir = Scratch::new();
        let file = dir.0.join("data.txt");
        std::fs::write(&file, b"x").unwrap();
        match mark_origin(&file, "data:text/plain,x", None) {
            Ok(()) => assert_eq!(xattr::get(&file, "user.xdg.origin.url").ok().flatten(), None),
            Err(e) => panic!("nothing to write, yet {e}"),
        }
    }
}
