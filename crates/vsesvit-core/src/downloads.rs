//! The downloads list, and the naming and status text both shells share.
//!
//! One row per download this device started. The shell owns the engine download and its
//! live byte counts; core stores the start and the outcome, so the list survives restarts.
//! A download still in progress when the browser exits reads as failed on the next start
//! ([`Downloads::interrupt_stale`]).
//!
//! LOCAL: files on this device's disk, so never synced.

use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::db::bad_column;
use crate::{Error, Profile, Url};

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

/// The row id of a download.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DownloadId(pub i64);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum State {
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl State {
    const ALL: [State; 4] = [State::InProgress, State::Completed, State::Failed, State::Cancelled];

    /// The `state` column's text.
    fn as_str(self) -> &'static str {
        match self {
            State::InProgress => "in_progress",
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
    /// Bytes written, as of the last [`Downloads::finish`]. Live progress lives in the shell.
    pub received: u64,
    pub total: Option<u64>,
}

pub struct Downloads<'p> {
    pub(crate) p: &'p mut Profile,
}

impl Downloads<'_> {
    /// Records a download that has just been given its destination. A path that is not
    /// valid Unicode is stored lossily.
    pub fn start(&mut self, url: &str, path: &Path, total: Option<u64>, now_ms: u64) -> Result<Download, Error> {
        let state = State::InProgress;
        self.p.conn.execute(
            "INSERT INTO downloads (url, path, started_ms, state, total) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![url, path.to_string_lossy(), now_ms as i64, state.as_str(), total.map(|t| t as i64)],
        )?;
        Ok(Download {
            id: DownloadId(self.p.conn.last_insert_rowid()),
            url: url.to_owned(),
            path: path.to_path_buf(),
            started_ms: now_ms,
            state,
            received: 0,
            total,
        })
    }

    /// Stores the outcome. `state` is a final state, never [`State::InProgress`]. No-op if
    /// the download was removed from the list meanwhile.
    pub fn finish(&mut self, id: DownloadId, state: State, received: u64, total: Option<u64>) -> Result<(), Error> {
        debug_assert_ne!(state, State::InProgress, "finish takes a final state");
        self.p.conn.execute(
            "UPDATE downloads SET state = ?2, received = ?3, total = ?4 WHERE id = ?1",
            params![id.0, state.as_str(), received as i64, total.map(|t| t as i64)],
        )?;
        Ok(())
    }

    /// Newest first.
    pub fn list(&mut self, limit: usize) -> Result<Vec<Download>, Error> {
        let mut stmt = self.p.conn.prepare_cached(
            "SELECT id, url, path, started_ms, state, received, total FROM downloads
             ORDER BY started_ms DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit.min(i64::MAX as usize) as i64], row_download)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Removes the entry from the list. The file stays on disk.
    pub fn remove(&mut self, id: DownloadId) -> Result<(), Error> {
        self.p.conn.execute("DELETE FROM downloads WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// Removes every entry that is not in progress. Files stay on disk.
    pub fn clear(&mut self) -> Result<(), Error> {
        self.p.conn.execute("DELETE FROM downloads WHERE state <> ?1", [State::InProgress.as_str()])?;
        Ok(())
    }

    /// Marks every download still in progress as failed and returns how many there were.
    /// Shells call it once at startup, before any download can begin: no engine download
    /// outlives the process that started it.
    pub fn interrupt_stale(&mut self) -> Result<usize, Error> {
        Ok(self.p.conn.execute(
            "UPDATE downloads SET state = ?2 WHERE state = ?1",
            [State::InProgress.as_str(), State::Failed.as_str()],
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

/// `dir/name`, or `dir/name (1)`, `dir/name (2)`, ... before the extension, whichever does not
/// exist yet. The suggested name comes from the server and is reduced to a plain file name.
pub fn unique_destination(dir: &Path, suggested: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let name = sanitize(suggested);
    let candidate = dir.join(&name);
    if !exists(&candidate) {
        return candidate;
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name.as_str(), ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){extension}")))
        .find(|candidate| !exists(candidate))
        .expect("an unused name exists")
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
    match d.state {
        State::InProgress => match live.unwrap_or((d.received, d.total)) {
            (received, Some(total)) => format!("{} of {}", describe_size(received), describe_size(total)),
            (received, None) => describe_size(received),
        },
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
    }

    #[test]
    fn state_names_round_trip() {
        for state in State::ALL {
            assert_eq!(State::parse(state.as_str()), Some(state));
        }
        assert_eq!(State::parse("paused"), None);
    }
}
