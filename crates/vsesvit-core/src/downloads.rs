//! The downloads list, and the naming and status text both shells share.
//!
//! One row per download this device started. The shell owns the engine download and its
//! live byte counts; core stores the start, each pause or interruption, and the outcome, so the
//! list survives restarts. A download the engine still held when the browser exited reads as
//! failed on the next start ([`Downloads::interrupt_stale`]).
//!
//! A file of a type that can run code ([`is_dangerous`]) is written under its unconfirmed name
//! ([`unconfirmed_path`]) and waits there, [`State::Unconfirmed`], until the user keeps it
//! ([`Downloads::keep`]) or discards it ([`Downloads::discard`]), as Chrome asks.
//!
//! LOCAL: files on this device's disk, so never synced.
//!
//! A download started in a private window has its row in the private session instead
//! ([`crate::private`]): listed with the others until the session ends. Its file stays on disk.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};

use crate::db::bad_column;
use crate::private::Browsing;
use crate::{Error, Profile, Url};

/// Migration v3. [`SCHEMA_STATES`] holds the table's current definition.
pub(crate) const SCHEMA: &str = "
CREATE TABLE downloads (                  -- LOCAL: files on this device's disk
  id          INTEGER PRIMARY KEY,
  url         TEXT NOT NULL,
  path        TEXT NOT NULL,
  started_ms  INTEGER NOT NULL,
  state       TEXT NOT NULL CHECK (state IN ('in_progress','completed','failed','cancelled')),
  received    INTEGER NOT NULL DEFAULT 0,
  total       INTEGER
);
";

/// Migration v10: the state CHECK accepts 'paused', 'interrupted' and 'unconfirmed', and ids are
/// never reused, so a warning left open about a removed entry cannot act on a later one. SQLite
/// cannot alter a CHECK, so the table is rebuilt as `extensions/schema_v4.sql` rebuilds the
/// extension tables.
pub(crate) const SCHEMA_STATES: &str = "
CREATE TABLE downloads_v10 (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  url         TEXT NOT NULL,
  path        TEXT NOT NULL,             -- where the file goes; an unconfirmed one waits beside it
  started_ms  INTEGER NOT NULL,
  state       TEXT NOT NULL CHECK (state IN ('in_progress','paused','interrupted','unconfirmed','completed','failed','cancelled')),
  received    INTEGER NOT NULL DEFAULT 0,
  total       INTEGER
);
INSERT INTO downloads_v10 (id, url, path, started_ms, state, received, total)
  SELECT id, url, path, started_ms, state, received, total FROM downloads;
DROP TABLE downloads;
ALTER TABLE downloads_v10 RENAME TO downloads;
";

/// The row id of a download.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DownloadId(pub i64);

impl DownloadId {
    /// Private rows take negative ids, so they share one id space with the stored rows, whose
    /// SQLite rowids are positive, and a shell keys every download by its id alike.
    fn is_private(self) -> bool {
        self.0 < 0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum State {
    InProgress,
    /// The user paused it; the engine keeps what arrived.
    Paused,
    /// A network or server error stopped it, and the engine can resume it.
    Interrupted,
    /// Every byte arrived, but the file can run code: it stays under its [`unconfirmed_path`]
    /// until the user keeps or discards it.
    Unconfirmed,
    Completed,
    Failed,
    Cancelled,
}

impl State {
    const ALL: [State; 7] = [
        State::InProgress,
        State::Paused,
        State::Interrupted,
        State::Unconfirmed,
        State::Completed,
        State::Failed,
        State::Cancelled,
    ];

    /// Whether the engine still holds the download: it can be cancelled, and it cannot outlive
    /// the process that started it.
    pub fn is_live(self) -> bool {
        matches!(self, State::InProgress | State::Paused | State::Interrupted)
    }

    /// Whether nothing is left to happen to the download, so it can leave the list.
    pub fn is_final(self) -> bool {
        matches!(self, State::Completed | State::Failed | State::Cancelled)
    }

    /// The `state` column's text.
    fn as_str(self) -> &'static str {
        match self {
            State::InProgress => "in_progress",
            State::Paused => "paused",
            State::Interrupted => "interrupted",
            State::Unconfirmed => "unconfirmed",
            State::Completed => "completed",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }

    fn parse(text: &str) -> Option<State> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    pub id: DownloadId,
    /// The source URL as the engine gave it (may be `data:` or `blob:`).
    pub url: String,
    /// The destination file.
    pub path: PathBuf,
    pub started_ms: u64,
    pub state: State,
    /// Bytes written, as of the last [`Downloads::update`]. Live progress lives in the shell.
    pub received: u64,
    pub total: Option<u64>,
}

pub struct Downloads<'p> {
    pub(crate) p: &'p mut Profile,
}

/// The rows of the private session's downloads.
#[derive(Default)]
pub(crate) struct PrivateDownloads {
    rows: Vec<Download>,
    /// The last id handed out, counting down from -1. Kept when the session ends, so a late
    /// [`Downloads::finish`] of an ended session's download never lands on a later one's row.
    last_id: i64,
}

impl PrivateDownloads {
    pub(crate) fn forget(&mut self) {
        self.rows.clear();
    }
}

impl Downloads<'_> {
    /// Records a download that has just been given its destination, in a tab of `browsing`'s
    /// kind. A path that is not valid Unicode is stored lossily.
    pub fn start(&mut self, url: &str, path: &Path, total: Option<u64>, now_ms: u64, browsing: Browsing) -> Result<Download, Error> {
        let state = State::InProgress;
        let id = match browsing {
            Browsing::Normal => {
                self.p.conn.execute(
                    "INSERT INTO downloads (url, path, started_ms, state, total) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![url, path.to_string_lossy(), now_ms as i64, state.as_str(), total.map(|t| t as i64)],
                )?;
                DownloadId(self.p.conn.last_insert_rowid())
            }
            Browsing::Private => {
                self.p.private.downloads.last_id -= 1;
                DownloadId(self.p.private.downloads.last_id)
            }
        };
        let download = Download {
            id,
            url: url.to_owned(),
            path: path.to_path_buf(),
            started_ms: now_ms,
            state,
            received: 0,
            total,
        };
        if browsing == Browsing::Private {
            self.p.private.downloads.rows.push(download.clone());
        }
        Ok(download)
    }

    /// Stores where the download is now: paused, interrupted, running again, waiting for the user
    /// to keep it, or ended. No-op if the download was removed from the list meanwhile.
    pub fn update(&mut self, id: DownloadId, state: State, received: u64, total: Option<u64>) -> Result<(), Error> {
        if id.is_private() {
            if let Some(d) = self.p.private.downloads.rows.iter_mut().find(|d| d.id == id) {
                (d.state, d.received, d.total) = (state, received, total);
            }
            return Ok(());
        }
        self.p.conn.execute(
            "UPDATE downloads SET state = ?2, received = ?3, total = ?4 WHERE id = ?1",
            params![id.0, state.as_str(), received as i64, total.map(|t| t as i64)],
        )?;
        Ok(())
    }

    /// Newest first, the private session's among the stored ones.
    pub fn list(&mut self, limit: usize) -> Result<Vec<Download>, Error> {
        let mut stmt = self.p.conn.prepare_cached(
            "SELECT id, url, path, started_ms, state, received, total FROM downloads
             ORDER BY started_ms DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit.min(i64::MAX as usize) as i64], row_download)?;
        let mut list: Vec<Download> = rows.collect::<Result<_, _>>()?;
        let private = &self.p.private.downloads.rows;
        if !private.is_empty() {
            list.extend(private.iter().cloned());
            // A private id counts down, so its magnitude grows with each download as a rowid does.
            list.sort_by_key(|d| Reverse((d.started_ms, d.id.0.unsigned_abs())));
            list.truncate(limit);
        }
        Ok(list)
    }

    /// Removes the entry from the list. The file stays on disk.
    pub fn remove(&mut self, id: DownloadId) -> Result<(), Error> {
        if id.is_private() {
            self.p.private.downloads.rows.retain(|d| d.id != id);
            return Ok(());
        }
        self.p.conn.execute("DELETE FROM downloads WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// Moves the unconfirmed file of `id` to its destination and lists it as completed. If a
    /// file took that name meanwhile, the kept one is numbered instead; no file is replaced.
    /// Returns where the file now is, or `None` if the entry no longer waits for the user.
    pub fn keep(&mut self, id: DownloadId) -> Result<Option<PathBuf>, Error> {
        let Some(download) = self.unconfirmed(id)? else { return Ok(None) };
        let kept = move_to_free_name(&unconfirmed_path(&download.path), &download.path)?;
        self.p.conn.execute(
            "UPDATE downloads SET state = ?2, path = ?3 WHERE id = ?1",
            params![id.0, State::Completed.as_str(), kept.to_string_lossy()],
        )?;
        Ok(Some(kept))
    }

    /// Deletes the unconfirmed file of `id` and takes it off the list. No-op if the entry no
    /// longer waits for the user.
    pub fn discard(&mut self, id: DownloadId) -> Result<(), Error> {
        let Some(download) = self.unconfirmed(id)? else { return Ok(()) };
        match std::fs::remove_file(unconfirmed_path(&download.path)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        self.remove(id)
    }

    /// The entry `id` while it waits for the user to keep or discard it.
    fn unconfirmed(&mut self, id: DownloadId) -> Result<Option<Download>, Error> {
        let mut stmt = self.p.conn.prepare_cached(
            "SELECT id, url, path, started_ms, state, received, total FROM downloads WHERE id = ?1 AND state = ?2",
        )?;
        Ok(stmt.query_row(params![id.0, State::Unconfirmed.as_str()], row_download).optional()?)
    }

    /// Removes every entry nothing is left to happen to ([`State::is_final`]), the private
    /// session's too. Files stay on disk.
    pub fn clear(&mut self) -> Result<(), Error> {
        self.p.private.downloads.rows.retain(|d| !d.state.is_final());
        self.p.conn.execute(
            "DELETE FROM downloads WHERE state IN (?1, ?2, ?3)",
            [State::Completed, State::Failed, State::Cancelled].map(State::as_str),
        )?;
        Ok(())
    }

    /// Marks every download the engine held ([`State::is_live`]) as failed and returns how many
    /// there were. Shells call it once at startup, before any download can begin: no engine
    /// download outlives the process that started it. An unconfirmed file still waits.
    pub fn interrupt_stale(&mut self) -> Result<usize, Error> {
        Ok(self.p.conn.execute(
            "UPDATE downloads SET state = ?4 WHERE state IN (?1, ?2, ?3)",
            [State::InProgress, State::Paused, State::Interrupted, State::Failed].map(State::as_str),
        )?)
    }
}

fn row_download(row: &rusqlite::Row<'_>) -> Result<Download, rusqlite::Error> {
    let state: String = row.get(4)?;
    let path: String = row.get(2)?;
    let started_ms: i64 = row.get(3)?;
    let received: i64 = row.get(5)?;
    let total: Option<i64> = row.get(6)?;
    Ok(Download {
        id: DownloadId(row.get(0)?),
        url: row.get(1)?,
        path: PathBuf::from(path),
        started_ms: started_ms as u64,
        state: State::parse(&state).ok_or_else(|| bad_column(4, "download state"))?,
        received: received as u64,
        total: total.map(|t| t as u64),
    })
}

/// `dir/name`, or `dir/name (1)`, `dir/name (2)`, ... before the extension, whichever is free:
/// neither it nor its [`unconfirmed_path`] exists. The suggested name comes from the server and
/// is reduced to a plain file name.
pub fn unique_destination(dir: &Path, suggested: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let name = sanitize(suggested);
    numbered(dir, &name)
        .find(|candidate| !exists(candidate) && !exists(&unconfirmed_path(candidate)))
        .expect("an unused name exists")
}

/// `dir/name`, then `dir/name (1)`, `dir/name (2)`, ... before the extension.
fn numbered(dir: &Path, name: &str) -> impl Iterator<Item = PathBuf> {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    std::iter::once(dir.join(name)).chain((1..).map(move |n| dir.join(format!("{stem} ({n}){extension}"))))
}

/// Moves `from` to `to`, or to the first numbered name beside `to` that no file has, and returns
/// where it went. A hard link claims the name, so a file that appears meanwhile is never replaced.
fn move_to_free_name(from: &Path, to: &Path) -> std::io::Result<PathBuf> {
    let dir = to.parent().unwrap_or(Path::new(""));
    let name = to.file_name().unwrap_or_default().to_string_lossy();
    for candidate in numbered(dir, &name) {
        match std::fs::hard_link(from, &candidate) {
            Ok(()) => {
                std::fs::remove_file(from)?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            // A file system without hard links, such as FAT: a rename, once no file has the name.
            Err(_) if !candidate.exists() => {
                std::fs::rename(from, &candidate)?;
                return Ok(candidate);
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the numbered names never end")
}

/// Where a file that can run code waits until the user keeps it: beside its destination, under
/// a name no program opens (`setup.exe.unconfirmed` for `setup.exe`).
pub fn unconfirmed_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".unconfirmed");
    path.with_file_name(name)
}

/// The systems whose dangerous file types differ.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum System {
    Windows,
    Linux,
}

impl System {
    const CURRENT: System = if cfg!(windows) { System::Windows } else { System::Linux };
}

/// File types that run code when opened, after Chrome's list of dangerous download types:
/// lowercase extensions, MIME types, and the systems they run on.
const DANGEROUS_TYPES: &[(&[&str], &[&str], &[System])] = &[
    (
        &[
            "ade", "adp", "app", "application", "appref-ms", "appx", "appxbundle", "bas", "bat", "chm", "cmd", "com", "cpl",
            "dll", "drv", "exe", "gadget", "hlp", "hta", "inf", "ins", "isp", "js", "jse", "library-ms", "lnk", "mad",
            "maf", "mag", "mam", "maq", "mar", "mas", "mat", "mau", "mav", "maw", "mda", "mdb", "mde", "mdt", "mdw", "mdz",
            "mmc", "msc", "msh", "msh1", "msh1xml", "msh2", "msh2xml", "mshxml", "msi", "msix", "msixbundle", "msp", "mst",
            "ocx", "ops", "pcd", "pif", "plg", "prf", "prg", "ps1", "ps1xml", "ps2", "ps2xml", "psc1", "psc2", "psd1",
            "psm1", "pst", "reg", "scf", "scr", "sct", "search-ms", "settingcontent-ms", "shb", "shs", "sys", "url", "vb",
            "vbe", "vbs", "vsmacros", "vsw", "website", "ws", "wsc", "wsf", "wsh", "xbap", "xnk",
        ],
        &[
            "application/hta",
            "application/vnd.microsoft.portable-executable",
            "application/x-bat",
            "application/x-dosexec",
            "application/x-ms-application",
            "application/x-ms-installer",
            "application/x-ms-shortcut",
            "application/x-msdos-program",
            "application/x-msdownload",
            "application/x-msi",
        ],
        &[System::Windows],
    ),
    (
        &["appimage", "bash", "csh", "deb", "desktop", "flatpak", "flatpakref", "ksh", "rpm", "run", "sh", "snap", "tcsh", "zsh"],
        &[
            "application/vnd.appimage",
            "application/vnd.debian.binary-package",
            "application/x-appimage",
            "application/x-debian-package",
            "application/x-desktop",
            "application/x-elf",
            "application/x-executable",
            "application/x-redhat-package-manager",
            "application/x-rpm",
            "application/x-sh",
            "application/x-shellscript",
        ],
        &[System::Linux],
    ),
    (
        &["jar", "jnlp"],
        &["application/java-archive", "application/x-java-archive", "application/x-java-jnlp-file"],
        &[System::Windows, System::Linux],
    ),
];

/// Whether a download named `name` and served as `mime` is of a type that runs code when opened
/// on this system, so it waits for the user to keep it.
pub fn is_dangerous(name: &str, mime: Option<&str>) -> bool {
    dangerous_on(System::CURRENT, name, mime)
}

fn dangerous_on(system: System, name: &str, mime: Option<&str>) -> bool {
    // Windows drops trailing dots and spaces when it makes a file: "setup.exe. " is setup.exe.
    let name = name.trim_end_matches(['.', ' ']).to_lowercase();
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    let mime = mime.map(|m| m.split(';').next().unwrap_or_default().trim().to_ascii_lowercase());
    DANGEROUS_TYPES.iter().filter(|(_, _, systems)| systems.contains(&system)).any(|(extensions, mimes, _)| {
        extension.is_some_and(|e| extensions.contains(&e)) || mime.as_deref().is_some_and(|m| mimes.contains(&m))
    })
}

/// The Mark of the Web for a file downloaded from `url`, as Chrome writes it to the file's
/// `Zone.Identifier` stream on Windows: the Internet zone, and the source without credentials
/// when it is a web address. SmartScreen and Office read it when the file is opened.
pub fn zone_identifier(url: &str) -> String {
    let source = Url::parse(url).ok().filter(|u| matches!(u.scheme(), "http" | "https")).map(|mut u| {
        let _ = u.set_username("");
        let _ = u.set_password(None);
        u.to_string()
    });
    format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={}\r\n", source.as_deref().unwrap_or("about:internet"))
}

/// A server-suggested name reduced to a plain file name: no directories, no control
/// characters, no leading dots. Empty results become "download".
pub fn sanitize(suggested: &str) -> String {
    let base = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim().trim_start_matches('.');
    if cleaned.is_empty() { "download".to_owned() } else { cleaned.to_owned() }
}

/// "512 B", "1.2 KB", "34 MB", "1.1 GB": decimal units, one decimal below 10.
pub fn describe_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let bytes = bytes as u128;
    let mut unit = 1u128;
    for (i, name) in UNITS.iter().enumerate() {
        unit *= 1000;
        let tenths = (bytes * 10 + unit / 2) / unit;
        if tenths < 100 {
            return format!("{}.{} {name}", tenths / 10, tenths % 10);
        }
        let whole = (bytes + unit / 2) / unit;
        if whole < 1000 || i == UNITS.len() - 1 {
            return format!("{whole} {name}");
        }
    }
    unreachable!("the last unit always returns")
}

/// The line under a download's file name, identical in both shells. `live` is the shell's
/// in-memory `(received, total)` for a download in progress; `exists` is whether the file
/// is still on disk.
pub fn status_line(d: &Download, live: Option<(u64, Option<u64>)>, exists: bool) -> String {
    let counts = || match live.unwrap_or((d.received, d.total)) {
        (received, Some(total)) => format!("{} of {}", describe_size(received), describe_size(total)),
        (received, None) => describe_size(received),
    };
    match d.state {
        State::InProgress => counts(),
        State::Paused => format!("Paused · {}", counts()),
        State::Interrupted => format!("Interrupted · {}", counts()),
        State::Unconfirmed => "This type of file can harm your device".to_owned(),
        State::Completed if !exists => "Deleted".to_owned(),
        State::Completed => {
            let size = describe_size(d.received);
            match Url::parse(&d.url).ok().as_ref().and_then(Url::host_str) {
                Some(host) => format!("{size} · {host}"),
                None => size,
            }
        }
        State::Failed => "Failed".to_owned(),
        State::Cancelled => "Cancelled".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn download(url: &str, state: State, received: u64, total: Option<u64>) -> Download {
        Download {
            id: DownloadId(1),
            url: url.to_owned(),
            path: PathBuf::from("/dl/file.bin"),
            started_ms: 0,
            state,
            received,
            total,
        }
    }

    #[test]
    fn names_are_reduced_to_a_plain_file_name() {
        assert_eq!(sanitize("report.pdf"), "report.pdf");
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("C:\\x\\evil.exe"), "evil.exe");
        assert_eq!(sanitize(".bashrc"), "bashrc");
        assert_eq!(sanitize("a\nb\u{7}.txt"), "ab.txt");
        assert_eq!(sanitize(""), "download");
        assert_eq!(sanitize(".."), "download");
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let dir = Path::new("/dl");
        let taken = [dir.join("a.tar.gz"), dir.join("a.tar (1).gz"), dir.join("notes")];
        let exists = |p: &Path| taken.iter().any(|t| t == p);
        assert_eq!(unique_destination(dir, "b.txt", exists), dir.join("b.txt"));
        assert_eq!(unique_destination(dir, "a.tar.gz", exists), dir.join("a.tar (2).gz"));
        assert_eq!(unique_destination(dir, "notes", exists), dir.join("notes (1)"));
    }

    #[test]
    fn sizes_use_decimal_units() {
        let cases = [
            (0, "0 B"),
            (512, "512 B"),
            (999, "999 B"),
            (1000, "1.0 KB"),
            (1_234, "1.2 KB"),
            (9_949, "9.9 KB"),
            (9_950, "10 KB"),
            (999_499, "999 KB"),
            (999_500, "1.0 MB"),
            (3_400_000, "3.4 MB"),
            (10_000_000, "10 MB"),
            (1_100_000_000, "1.1 GB"),
            (5_000_000_000_000_000, "5000 TB"),
            (u64::MAX, "18446744 TB"),
        ];
        for (bytes, text) in cases {
            assert_eq!(describe_size(bytes), text, "{bytes} bytes");
        }
    }

    #[test]
    fn status_lines() {
        let url = "https://example.com/files/a.zip";
        let running = download(url, State::InProgress, 0, Some(10_000_000));
        assert_eq!(status_line(&running, Some((3_200_000, Some(10_000_000))), true), "3.2 MB of 10 MB");
        assert_eq!(status_line(&running, Some((3_200_000, None)), true), "3.2 MB", "unknown total");
        assert_eq!(status_line(&running, None, true), "0 B of 10 MB", "no live counts yet");

        let done = download(url, State::Completed, 10_000_000, Some(10_000_000));
        assert_eq!(status_line(&done, None, true), "10 MB · example.com");
        assert_eq!(status_line(&done, None, false), "Deleted");
        let data = download("data:text/plain,hi", State::Completed, 2, None);
        assert_eq!(status_line(&data, None, true), "2 B", "no host for data:");
        let blob = download("blob:https://example.com/5e1f", State::Completed, 2, None);
        assert_eq!(status_line(&blob, None, true), "2 B", "no host for blob:");

        assert_eq!(status_line(&download(url, State::Failed, 5, None), None, true), "Failed");
        assert_eq!(status_line(&download(url, State::Cancelled, 5, None), None, false), "Cancelled");

        let paused = download(url, State::Paused, 1_000, Some(10_000_000));
        assert_eq!(status_line(&paused, Some((3_200_000, Some(10_000_000))), true), "Paused · 3.2 MB of 10 MB");
        assert_eq!(status_line(&paused, None, false), "Paused · 1.0 KB of 10 MB", "the stored counts");
        let interrupted = download(url, State::Interrupted, 0, None);
        assert_eq!(status_line(&interrupted, Some((5, None)), true), "Interrupted · 5 B");
        let unconfirmed = download(url, State::Unconfirmed, 5, Some(5));
        assert_eq!(status_line(&unconfirmed, None, true), "This type of file can harm your device");
    }

    #[test]
    fn state_names_round_trip() {
        for state in State::ALL {
            assert_eq!(State::parse(state.as_str()), Some(state));
        }
        assert_eq!(State::parse("running"), None);
    }

    #[test]
    fn live_and_final_states() {
        let live: Vec<State> = State::ALL.into_iter().filter(|s| s.is_live()).collect();
        let done: Vec<State> = State::ALL.into_iter().filter(|s| s.is_final()).collect();
        assert_eq!(live, [State::InProgress, State::Paused, State::Interrupted]);
        assert_eq!(done, [State::Completed, State::Failed, State::Cancelled]);
        assert!(!State::Unconfirmed.is_live() && !State::Unconfirmed.is_final(), "waits for the user");
    }

    #[test]
    fn an_unconfirmed_file_holds_its_name() {
        let dir = Path::new("/dl");
        assert_eq!(unconfirmed_path(&dir.join("setup.exe")), dir.join("setup.exe.unconfirmed"));
        let taken = [dir.join("setup.exe.unconfirmed")];
        let exists = |p: &Path| taken.iter().any(|t| t == p);
        assert_eq!(unique_destination(dir, "setup.exe", exists), dir.join("setup (1).exe"));
    }

    #[test]
    fn dangerous_types_depend_on_the_system() {
        use System::{Linux, Windows};
        let cases = [
            ("setup.exe", None, Windows, true),
            ("SETUP.EXE", None, Windows, true),
            ("setup.exe. ", None, Windows, true),
            ("installer.msi", None, Windows, true),
            ("run.ps1", None, Windows, true),
            ("setup.exe", None, Linux, false),
            ("install.sh", None, Linux, true),
            ("app.AppImage", None, Linux, true),
            ("pkg.deb", None, Linux, true),
            ("install.sh", None, Windows, false),
            ("tool.jar", None, Windows, true),
            ("tool.jar", None, Linux, true),
            ("report.pdf", None, Windows, false),
            ("archive.zip", None, Linux, false),
            ("exe", None, Windows, false),
            ("download", Some("application/x-msdownload"), Windows, true),
            ("download", Some("Application/X-MSDownload; charset=binary"), Windows, true),
            ("download", Some("application/x-executable"), Linux, true),
            ("download", Some("application/octet-stream"), Windows, false),
            ("notes.txt", Some("text/plain"), Linux, false),
        ];
        for (name, mime, system, dangerous) in cases {
            assert_eq!(dangerous_on(system, name, mime), dangerous, "{name} {mime:?} on {system:?}");
        }
    }

    #[test]
    fn the_mark_of_the_web_names_a_web_source() {
        assert_eq!(
            zone_identifier("https://user:secret@example.com/files/setup.exe?x=1"),
            "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/files/setup.exe?x=1\r\n"
        );
        assert_eq!(zone_identifier("data:application/x-msdownload,MZ"), "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n");
        assert_eq!(zone_identifier("blob:https://example.com/5e1f"), "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n");
    }
}
