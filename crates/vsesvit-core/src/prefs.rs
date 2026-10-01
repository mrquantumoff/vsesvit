//! Typed preferences.
//!
//! One LWW register per key, holding canonical JSON (`None` = reset to default). Keys
//! are declared in code as typed [`Pref<T>`] constants, so callers never handle strings
//! or JSON. Each pref declares whether it syncs. Keys this build does not know (written
//! by a newer build on another device) are stored and re-synced untouched.
//!
//! A stored value that no longer decodes as `T` (a newer build changed its shape) reads
//! as the default. That is boundary validation at read time; the stored value is left
//! alone for the newer build.

use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::crdt::{JsonText, Lattice, Lww, Seq, Stamp};
use crate::db::{opt_json_col, seq_col, stamp_col};
use crate::search::SearchEngineId;
use crate::shortcuts::Overrides;
use crate::sync::{Kind, SyncTable, changed_rows};
use crate::{Error, Profile};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Synced,
    /// Stored with stamps like everything else but never exported (window geometry,
    /// device name, download dir).
    Local,
}

pub struct Pref<T: 'static> {
    pub key: &'static str,
    pub scope: Scope,
    pub default: fn() -> T,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Where the tab list lives. Vertical tabs (a sidebar) are the default.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabsPosition {
    Left,
    Right,
    Top,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Startup {
    RestoreSession,
    Homepage,
    NewTab,
}

/// Which releases an installation updates to (docs/design/packaging.md, "Releasing"), from the
/// steadiest to the newest.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    Beta,
    Weekly,
    Nightly,
}

impl UpdateChannel {
    pub const ALL: [UpdateChannel; 4] = [Self::Stable, Self::Beta, Self::Weekly, Self::Nightly];

    /// The update server's name for it, which fills `{{channel}}` in an update endpoint.
    pub fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Weekly => "weekly",
            Self::Nightly => "nightly",
        }
    }

    /// The channel `version` was released on: the first field of its prerelease, or stable
    /// without one. `None` for a prerelease no channel is named after.
    pub fn of_version(version: &str) -> Option<UpdateChannel> {
        let version = version.split_once('+').map_or(version, |(v, _)| v);
        let Some((_, prerelease)) = version.split_once('-') else {
            return Some(Self::Stable);
        };
        let name = prerelease.split('.').next().unwrap_or_default();
        Self::ALL.into_iter().find(|channel| channel.name() == name)
    }

    /// The channel this build was released on, which an installation follows until the user
    /// picks another. A local build with an unnamed prerelease follows stable.
    pub fn of_build() -> UpdateChannel {
        Self::of_version(env!("CARGO_PKG_VERSION")).unwrap_or(Self::Stable)
    }
}

pub mod keys {
    use super::*;

    pub const HOMEPAGE: Pref<String> = Pref { key: "homepage", scope: Scope::Synced, default: || "about:home".to_owned() };
    /// Exactly one default engine: a single register, not an `is_default` flag on each
    /// engine record (which two devices could set on two different engines).
    pub const DEFAULT_SEARCH_ENGINE: Pref<SearchEngineId> =
        Pref { key: "search.default", scope: Scope::Synced, default: SearchEngineId::builtin_default };
    pub const THEME: Pref<Theme> = Pref { key: "theme", scope: Scope::Synced, default: || Theme::System };
    pub const STARTUP: Pref<Startup> = Pref { key: "startup", scope: Scope::Synced, default: || Startup::RestoreSession };
    pub const TABS_POSITION: Pref<TabsPosition> = Pref { key: "tabs.position", scope: Scope::Synced, default: || TabsPosition::Left };
    pub const SHOW_BOOKMARKS_BAR: Pref<bool> = Pref { key: "bookmarks_bar.visible", scope: Scope::Synced, default: || true };
    /// A narrow address bar, centered in the toolbar, instead of one that fills it.
    pub const COMPACT_ADDRESS_BAR: Pref<bool> = Pref { key: "address_bar.compact", scope: Scope::Synced, default: || true };
    /// Show whole URLs in the address bar; off, it shows [`crate::address::simplified_url`]
    /// until the user clicks into it.
    pub const SHOW_FULL_URLS: Pref<bool> = Pref { key: "address_bar.full_urls", scope: Scope::Synced, default: || false };
    /// A Home button in the toolbar, next to Reload, that opens [`HOMEPAGE`].
    pub const SHOW_HOME_BUTTON: Pref<bool> = Pref { key: "toolbar.home_button", scope: Scope::Synced, default: || false };
    /// Whether the address bar suggests pages from history ([`crate::search::Omnibox::suggest`]).
    pub const SUGGEST_HISTORY: Pref<bool> = Pref { key: "address_bar.suggest.history", scope: Scope::Synced, default: || true };
    /// Whether the address bar suggests bookmarks.
    pub const SUGGEST_BOOKMARKS: Pref<bool> = Pref { key: "address_bar.suggest.bookmarks", scope: Scope::Synced, default: || true };
    /// Refuse windows a page opens without a user gesture.
    pub const BLOCK_POPUPS: Pref<bool> = Pref { key: "content.block_popups", scope: Scope::Synced, default: || true };
    /// Offer to save passwords typed into sign-in forms, where the engine has a password store.
    pub const SAVE_PASSWORDS: Pref<bool> = Pref { key: "autofill.passwords", scope: Scope::Synced, default: || true };
    /// Save and fill form entries such as addresses, where the engine supports it.
    pub const AUTOFILL_FORMS: Pref<bool> = Pref { key: "autofill.forms", scope: Scope::Synced, default: || true };
    pub const SMOOTH_SCROLLING: Pref<bool> = Pref { key: "scrolling.smooth", scope: Scope::Synced, default: || true };
    /// Local: whether the GPU works well is a property of this device.
    pub const HARDWARE_ACCELERATION: Pref<bool> = Pref { key: "system.hardware_acceleration", scope: Scope::Local, default: || true };
    pub const DEVICE_NAME: Pref<String> = Pref { key: "device.name", scope: Scope::Local, default: || String::new() };
    /// Local: whether an installation checks for and downloads updates is a property of that
    /// installation, not of the user's other devices.
    pub const UPDATES_AUTOMATIC: Pref<bool> = Pref { key: "updates.automatic", scope: Scope::Local, default: || true };
    /// Local, like [`UPDATES_AUTOMATIC`]. Unset, an installation follows the channel its build
    /// was released on.
    pub const UPDATES_CHANNEL: Pref<UpdateChannel> =
        Pref { key: "updates.channel", scope: Scope::Local, default: UpdateChannel::of_build };
    /// Local: a folder on this device's disk. `None` = the platform's Downloads folder, which
    /// only the shell knows.
    pub const DOWNLOADS_DIR: Pref<Option<PathBuf>> = Pref { key: "downloads.directory", scope: Scope::Local, default: || None };
    /// Local: the welcome flow ran on this device; another device's run says nothing
    /// about this one. See [`crate::onboarding`].
    pub const ONBOARDING_DONE: Pref<bool> = Pref { key: "onboarding.done", scope: Scope::Local, default: || false };
    /// Whether each download opens a save dialog instead of going straight to [`DOWNLOADS_DIR`].
    pub const DOWNLOADS_ASK: Pref<bool> = Pref { key: "downloads.ask", scope: Scope::Synced, default: || false };
    /// The user's shortcut reassignments, read through [`crate::shortcuts::Keymap`]. One register
    /// for the whole keymap, not one per command: two devices reassigning different commands
    /// could each take the same chord, and merging per command would give that chord to both.
    /// Last writer wins keeps a keymap one device resolved as a whole.
    pub const SHORTCUTS: Pref<Overrides> = Pref { key: "keyboard.shortcuts", scope: Scope::Synced, default: Overrides::new };
    /// Local: the sync server this device signs in to, as a base URL. Every device of an account
    /// names the same one, so there is nothing to sync, and a synced value would move a device
    /// off its server before it could sign in there.
    pub const SYNC_SERVER: Pref<String> = Pref { key: "sync.server", scope: Scope::Local, default: || DEFAULT_SYNC_SERVER.to_owned() };
    /// What this device syncs; every type by default.
    pub const SYNC_TYPES: Pref<Vec<crate::sync::DataType>> =
        Pref { key: super::SYNC_TYPES_KEY, scope: Scope::Local, default: || crate::sync::DataType::ALL.to_vec() };

    /// Every Local pref above, which sync refuses to write (see [`super::remote_may_write`]).
    pub(crate) const LOCAL_KEYS: &[&str] = &[
        local(&HARDWARE_ACCELERATION),
        local(&DEVICE_NAME),
        local(&UPDATES_AUTOMATIC),
        local(&UPDATES_CHANNEL),
        local(&DOWNLOADS_DIR),
        local(&ONBOARDING_DONE),
        local(&SYNC_SERVER),
        local(&SYNC_TYPES),
    ];

    const fn local<T>(pref: &Pref<T>) -> &'static str {
        assert!(matches!(pref.scope, Scope::Local), "LOCAL_KEYS lists only Local prefs");
        pref.key
    }
}

/// Local, as Chrome's "Customize sync" is per device.
pub const SYNC_TYPES_KEY: &str = "sync.types";

pub const DEFAULT_SYNC_SERVER: &str = "https://vsesvit-service.mrquantumoff.dev";

/// Sync record: one per key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefRecord {
    pub key: String,
    #[serde(with = "crate::crdt::json_register")]
    pub value: Lww<Option<JsonText>>,
}

impl Lattice for PrefRecord {
    fn join(&mut self, other: Self) {
        self.value.join(other.value);
    }
}

pub struct Prefs<'p> {
    pub(crate) p: &'p mut Profile,
}

impl Prefs<'_> {
    /// Primary-key lookup. Falls back to `default()` if the key is missing, reset, or undecodable.
    /// A Local pref reads only a value this device wrote, never one sync stored.
    pub fn get<T: DeserializeOwned>(&mut self, pref: &Pref<T>) -> T {
        let sql = match pref.scope {
            Scope::Synced => "SELECT value FROM prefs WHERE key = ?1",
            Scope::Local => "SELECT value FROM prefs WHERE key = ?1 AND synced = 0",
        };
        let stored: Option<Option<String>> = self
            .p
            .conn
            .query_row(sql, [pref.key], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        stored
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| (pref.default)())
    }

    /// No-op (no stamp) if the canonical JSON is unchanged. Setting the default value
    /// explicitly stores it, so the choice syncs.
    pub fn set<T: Serialize>(&mut self, pref: &Pref<T>, value: &T) -> Result<(), Error> {
        let json = serde_json::to_value(value).map_err(|e| Error::Io(std::io::Error::other(e)))?;
        self.write(pref.key, pref.scope, Some(JsonText::from_value(&json)))
    }

    /// Writes `None`: "use the default", which also syncs.
    pub fn reset<T>(&mut self, pref: &Pref<T>) -> Result<(), Error> {
        self.write(pref.key, pref.scope, None)
    }

    fn write(&mut self, key: &str, scope: Scope, value: Option<JsonText>) -> Result<(), Error> {
        self.p.write(|tx| {
            // A Local pref ignores a row sync stored, as `get` does, so writing takes it back.
            let mut rec = load_record(&tx.sql, key, scope == Scope::Local)?
                .unwrap_or(PrefRecord { key: key.to_owned(), value: Lww::new(None, Stamp::ZERO) });
            if rec.value.v == value {
                return Ok(());
            }
            let at = tx.stamp();
            rec.value.set(value, at);
            // A Local row is never uploaded, so it takes no seq and leaves `change_seq` alone.
            let seq = if scope == Scope::Synced { tx.seq() } else { Seq::ZERO };
            store_record(&tx.sql, &rec, seq, Some(scope == Scope::Synced))
        })
    }
}

const COLUMNS: &str = "key, value, value_at, synced, seq";

fn row_record(row: &rusqlite::Row<'_>) -> Result<(Seq, PrefRecord), rusqlite::Error> {
    let key: String = row.get(0)?;
    let value = opt_json_col(row, 1)?;
    let at = stamp_col(row, 2)?;
    Ok((seq_col(row, 4)?, PrefRecord { key, value: Lww::new(value, at) }))
}

fn load_record(conn: &rusqlite::Connection, key: &str, local_only: bool) -> Result<Option<PrefRecord>, Error> {
    let filter = if local_only { " AND synced = 0" } else { "" };
    let rec = conn.query_row(&format!("SELECT {COLUMNS} FROM prefs WHERE key = ?1{filter}"), [key], row_record).optional()?;
    Ok(rec.map(|(_, r)| r))
}

/// `synced: None` keeps an existing row's flag (remote writes never change scope) and
/// defaults a new row to synced.
fn store_record(conn: &rusqlite::Connection, rec: &PrefRecord, seq: Seq, synced: Option<bool>) -> Result<(), Error> {
    let flag = synced.map(i64::from);
    conn.execute(
        &format!(
            "INSERT INTO prefs ({COLUMNS}) VALUES (?1, ?2, ?3, COALESCE(?4, 1), ?5) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, value_at = excluded.value_at, \
             synced = COALESCE(?4, prefs.synced), seq = excluded.seq"
        ),
        params![rec.key, rec.value.v.as_ref().map(JsonText::as_str), rec.value.at.to_vec(), flag, seq.0 as i64],
    )?;
    Ok(())
}

/// Whether a record from sync may write `key`: never a Local pref, whether this build declares
/// it ([`keys::LOCAL_KEYS`]) or this device stored it as local (a shell's, or a newer build's).
pub(crate) fn remote_may_write(conn: &rusqlite::Connection, key: &str) -> Result<bool, Error> {
    if keys::LOCAL_KEYS.contains(&key) {
        return Ok(false);
    }
    let synced: Option<i64> = conn.query_row("SELECT synced FROM prefs WHERE key = ?1", [key], |r| r.get(0)).optional()?;
    Ok(synced != Some(0))
}

pub(crate) struct PrefsTable;

impl SyncTable for PrefsTable {
    const KIND: Kind = Kind::Prefs;
    type Record = PrefRecord;

    fn wire_id(rec: &PrefRecord) -> String {
        rec.key.clone()
    }

    fn max_stamp(rec: &PrefRecord) -> Option<Stamp> {
        Some(rec.value.at)
    }

    fn load(tx: &rusqlite::Transaction<'_>, wire_id: &str) -> Result<Option<PrefRecord>, Error> {
        load_record(tx, wire_id, false)
    }

    fn store(tx: &rusqlite::Transaction<'_>, rec: &PrefRecord, seq: Seq) -> Result<(), Error> {
        store_record(tx, rec, seq, None)
    }

    fn changed_since(conn: &rusqlite::Connection, since: Seq, limit: usize) -> Result<(Vec<(Seq, PrefRecord)>, bool), Error> {
        changed_rows(conn, "prefs", COLUMNS, "synced = 1", since, limit, row_record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_names_its_channel() {
        for (version, channel) in [
            ("0.2.0", Some(UpdateChannel::Stable)),
            ("0.2.0+build.4", Some(UpdateChannel::Stable)),
            ("0.2.0-beta.1", Some(UpdateChannel::Beta)),
            ("0.1.1-weekly.20260928.5", Some(UpdateChannel::Weekly)),
            ("0.1.1-nightly.20260929.8+abc", Some(UpdateChannel::Nightly)),
            ("0.2.0-rc.1", None),
        ] {
            assert_eq!(UpdateChannel::of_version(version), channel, "{version}");
        }
    }

    #[test]
    fn channels_round_trip_as_their_names() {
        for channel in UpdateChannel::ALL {
            assert_eq!(serde_json::to_value(channel).unwrap(), channel.name());
        }
    }

    /// `LOCAL_KEYS` holds only Local prefs (checked as it is built); this checks it holds them all.
    #[test]
    fn local_keys_lists_every_local_pref() {
        let source = include_str!("prefs.rs");
        let keys = &source[source.find("pub mod keys").unwrap()..source.find("const fn local").unwrap()];
        assert_eq!(keys.matches("scope: Scope::Local").count(), keys::LOCAL_KEYS.len());
    }
}
