//! # vsesvit-core
//!
//! The platform-agnostic half of Vsesvit: the profile store, the sync-ready data model,
//! omnibox policy and the extension install pipeline. Both shells (WinUI 3 + WebView2,
//! GTK4 + WebKitGTK) are thin. They own widgets and engine views and call one
//! [`Profile`] from their UI thread.
//!
//! ```ignore
//! let root = Profile::default_root("Default");
//! let mut profile = Profile::open(&root, OpenOptions::default())?;
//! profile.history().record_visit(&url, Transition::Link)?;
//! let starred = profile.bookmarks().is_bookmarked(&url);
//! ```
//!
//! Module map. Every public mutation is one SQLite transaction stamped with one
//! [`crdt::Stamp`] and one [`crdt::Seq`] (see `db::Tx`).
//!
//! | module           | owns                                                                 |
//! |------------------|----------------------------------------------------------------------|
//! | [`address`]      | URLs as the address bar writes them: decoded, simplified              |
//! | [`certificate`]  | X.509 certificates parsed for the connection popup                    |
//! | [`clean_url`]    | links without tracking parameters, for "copy clean link"             |
//! | [`cookies`]      | cookie controls: third-party cookies, site rules, clearing on exit    |
//! | [`crdt`]         | clock, stamps, `Lww<T>`, `Lattice`: the only merge primitives         |
//! | [`bookmarks`]    | records, fractional positions, in-memory tree, `materialize`          |
//! | [`downloads`]    | the downloads list (LOCAL), file naming, status text                  |
//! | [`favicons`]     | icons of bookmarked sites (LOCAL, never synced)                       |
//! | [`history`]      | page records (grow-only visit sets) + deletion directives             |
//! | [`https_only`]   | HTTPS-only: http upgrades, the warning page, site exceptions          |
//! | [`import`]       | bookmarks from other browsers: HTML export, Chromium, Firefox         |
//! | [`new_tab`]      | the new tab page: search box + most visited sites, as HTML            |
//! | [`onboarding`]   | the first-run welcome: when to show it, recommended extensions        |
//! | [`permissions`]  | site permissions: stored choices, one-time grants, the prompt         |
//! | [`zoom`]         | page zoom per site (LOCAL)                                            |
//! | [`session`]      | this device's windows/tabs (restore) = its published "tabs" record    |
//! | [`prefs`]        | typed preferences                                                    |
//! | [`search`]       | search engines, omnibox resolve + suggest                             |
//! | [`shortcuts`]    | commands, default chords, the user's keymap                           |
//! | [`suggest`]      | the default engine's search suggestions: request, fetch, parsing      |
//! | [`tab_place`]    | a tab's place in its window, which other tabs the tab menu closes     |
//! | [`trackers`]     | tracking protection: the bundled tracker list, level, site exceptions |
//! | [`extensions`]   | desired set (synced) vs installed set (local), CRX3/XPI/unpacked      |
//! | [`ext_storage`]  | `chrome.storage.local` / `.sync` backing for the Linux runtime        |
//! | [`sync`]         | what the sync engine (`vsesvit-sync`) calls: `changes_since`, `apply` |
//! | [`vault`]        | the profile's OS-protected key that seals secrets (LOCAL)             |
//! | [`view_source`]  | `view-source:` addresses, and the source page for engines without one |
//! | `db`             | schema, migrations, `Tx` (stamp + seq + clock persistence)            |

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

pub mod address;
pub mod bookmarks;
pub mod certificate;
pub mod clean_url;
pub mod cookies;
pub mod crdt;
mod db;
pub mod downloads;
pub mod export;
pub mod ext_storage;
pub mod extensions;
pub mod favicons;
pub mod history;
pub mod html;
pub mod https_only;
pub mod import;
pub mod new_tab;
pub mod onboarding;
pub mod permissions;
pub mod prefs;
pub mod search;
pub mod session;
pub mod shortcuts;
pub mod spellcheck;
pub mod suggest;
pub mod sync;
pub mod tab_place;
#[cfg(feature = "testkit")]
pub mod testkit;
pub mod trackers;
pub mod vault;
pub mod view_source;
pub mod zoom;

pub use url::Url;

use crdt::{Clock, DeviceId, Hlc, TimeSource};

/// One open browser profile. One per process, owned by the UI thread.
///
/// Every accessor borrows `&mut self`, reads included. That keeps one rule for callers:
/// the shells keep the profile in a `RefCell` (GTK: `Rc<RefCell<Profile>>`; WinUI: a
/// `thread_local!`) and borrow it for one call. Core never calls back into shell code,
/// so a borrow is never re-entered.
///
/// `Profile` is `!Send` on purpose. Work that must leave the UI thread (extension
/// downloads, the sync engine's network I/O) is shaped as a `Send` value that holds
/// no database handle, and its result is committed back here on the UI thread. Nothing
/// is shared between threads, so there are no locks.
pub struct Profile {
    pub(crate) conn: rusqlite::Connection,
    pub(crate) paths: ProfilePaths,
    pub(crate) device: DeviceId,
    pub(crate) clock: Clock,
    pub(crate) next_seq: u64,
    /// All bookmark records plus the materialized tree, loaded at open. Bookmarks are
    /// small (10^3..10^4) and read on every committed navigation (star state) and every
    /// bookmarks-bar paint, so reads never touch SQLite.
    pub(crate) bookmarks: bookmarks::Model,
    pub(crate) chrome_version: String,
    key_store: vault::KeyStore,
    /// Fetched on first use, then kept for the process lifetime.
    vault_key: Option<vault::Key>,
    created: bool,
    /// Exclusive OS lock (`std::fs::File::try_lock`) on `<root>/LOCK`. The OS releases it
    /// when the process dies, so a crash never leaves a stale lock.
    _lock: std::fs::File,
    _not_send: PhantomData<*const ()>,
}

/// Where everything for one profile lives. See DESIGN.md "Profile directory".
#[derive(Clone, Debug)]
pub struct ProfilePaths {
    /// `<data_local_dir>/Vsesvit/profiles/<name>`
    pub root: PathBuf,
    /// `<root>/vsesvit.db` (+ `-wal`, `-shm`)
    pub db: PathBuf,
    /// `<root>/extensions/<id>/<version>_<sha256 prefix, 32 hex>/`: immutable once committed
    pub extensions: PathBuf,
    /// `<root>/staging/<job uuid>/`: install scratch, wiped at open
    pub staging: PathBuf,
    /// `<root>/engine/`: WebView2 user data folder, or WebKit `NetworkSession` data dir
    pub engine_data: PathBuf,
    /// `<cache_dir>/Vsesvit/<name>/`: WebKit cache dir (WebView2 keeps its cache under engine_data)
    pub engine_cache: PathBuf,
}

impl ProfilePaths {
    fn new(root: &Path) -> ProfilePaths {
        let root = root.to_path_buf();
        let in_default_location = root.parent().is_some_and(|p| p == default_profiles_dir());
        let engine_cache = match (project_dirs(), root.file_name()) {
            (Some(dirs), Some(name)) if in_default_location => dirs.cache_dir().join(name),
            _ => root.join("cache"),
        };
        ProfilePaths {
            db: root.join("vsesvit.db"),
            extensions: root.join("extensions"),
            staging: root.join("staging"),
            engine_data: root.join("engine"),
            engine_cache,
            root,
        }
    }
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "Vsesvit")
}

fn default_profiles_dir() -> PathBuf {
    project_dirs().map(|d| d.data_local_dir().join("profiles")).unwrap_or_else(|| PathBuf::from("profiles"))
}

pub struct OpenOptions {
    /// Time source for the hybrid logical clock. Tests use `TimeSource::Manual` to skew devices.
    pub time: TimeSource,
    /// Device id to mint if the profile is being created. `None` = random. Ignored for
    /// existing profiles (the id is in `meta`).
    pub new_device_id: Option<DeviceId>,
    /// Sent to the Chrome Web Store as `prodversion`. The Windows shell passes the WebView2
    /// runtime's Chromium version; Linux keeps the default.
    pub chrome_version: String,
    /// Where the vault key goes if the profile has none yet.
    pub key_store: vault::KeyStore,
}

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions {
            time: TimeSource::System,
            new_device_id: None,
            chrome_version: extensions::DEFAULT_CHROME_VERSION.to_owned(),
            key_store: vault::KeyStore::default(),
        }
    }
}

impl Profile {
    /// `directories::ProjectDirs::from("", "", "Vsesvit").data_local_dir()/profiles/<name>`:
    /// `%LOCALAPPDATA%\Vsesvit\data\profiles\<name>` on Windows,
    /// `~/.local/share/vsesvit/profiles/<name>` on Linux.
    pub fn default_root(name: &str) -> PathBuf {
        default_profiles_dir().join(name)
    }

    /// Open or create a profile.
    ///
    /// 1. create `root`, take the exclusive lock on `root/LOCK` or fail with [`OpenError::Locked`]
    /// 2. open `vsesvit.db`, set pragmas (WAL, synchronous=NORMAL, foreign_keys), migrate
    ///    (`PRAGMA user_version`), read or mint `meta.device_id`, restore the clock
    /// 3. load all bookmark records and materialize the tree
    /// 4. housekeeping, idempotent (`extensions::on_open`): wipe `staging/`, drop
    ///    `extension_installs` rows whose dir is missing, delete `extensions/*/*` dirs no
    ///    row references; drop favicons of sites with no bookmark left
    pub fn open(root: &Path, opts: OpenOptions) -> Result<Profile, OpenError> {
        std::fs::create_dir_all(root)?;
        let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join("LOCK"))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Err(OpenError::Locked),
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        let paths = ProfilePaths::new(root);
        for dir in [&paths.extensions, &paths.staging, &paths.engine_data] {
            std::fs::create_dir_all(dir)?;
        }

        let mut conn = db::open(&paths.db)?;
        db::migrate(&mut conn)?;
        let now_ms = Clock::new(opts.time.clone(), Hlc::ZERO).now_ms();
        let meta = db::load_or_init_meta(&mut conn, opts.new_device_id, now_ms)?;
        let records = bookmarks::load_all(&conn).map_err(OpenError::from_core)?;

        let mut profile = Profile {
            conn,
            paths,
            device: meta.device,
            clock: Clock::new(opts.time, meta.clock_last),
            next_seq: meta.next_seq,
            bookmarks: bookmarks::Model::new(records),
            chrome_version: opts.chrome_version,
            key_store: opts.key_store,
            vault_key: None,
            created: meta.created,
            _lock: lock,
            _not_send: PhantomData,
        };
        extensions::on_open(&mut profile).map_err(OpenError::from_core)?;
        profile.favicons().prune().map_err(OpenError::from_core)?;
        Ok(profile)
    }

    pub fn paths(&self) -> &ProfilePaths {
        &self.paths
    }

    /// True when this [`Profile::open`] created the profile, false on every later open.
    /// A profile made by a build older than this API reads as not new.
    pub fn is_new(&self) -> bool {
        self.created
    }

    pub fn device_id(&self) -> DeviceId {
        self.device
    }

    /// The Chromium version presented to the Chrome Web Store.
    pub fn chrome_version(&self) -> &str {
        &self.chrome_version
    }

    pub fn bookmarks(&mut self) -> bookmarks::Bookmarks<'_> {
        bookmarks::Bookmarks { p: self }
    }

    pub fn downloads(&mut self) -> downloads::Downloads<'_> {
        downloads::Downloads { p: self }
    }

    pub fn favicons(&mut self) -> favicons::Favicons<'_> {
        favicons::Favicons { p: self }
    }

    pub fn history(&mut self) -> history::History<'_> {
        history::History { p: self }
    }

    pub fn session(&mut self) -> session::Session<'_> {
        session::Session { p: self }
    }

    pub fn prefs(&mut self) -> prefs::Prefs<'_> {
        prefs::Prefs { p: self }
    }

    pub fn site_permissions(&mut self) -> permissions::SitePermissions<'_> {
        permissions::SitePermissions { p: self }
    }

    pub fn site_zoom(&mut self) -> zoom::SiteZoom<'_> {
        zoom::SiteZoom { p: self }
    }

    pub fn search_engines(&mut self) -> search::SearchEngines<'_> {
        search::SearchEngines { p: self }
    }

    pub fn omnibox(&mut self) -> search::Omnibox<'_> {
        search::Omnibox { p: self }
    }

    pub fn extensions(&mut self) -> extensions::Extensions<'_> {
        extensions::Extensions { p: self }
    }

    pub fn ext_storage(&mut self) -> ext_storage::ExtStorage<'_> {
        ext_storage::ExtStorage { p: self }
    }

    /// The surface the sync engine (`vsesvit-sync`) uses. Nothing in core calls it.
    pub fn sync(&mut self) -> sync::SyncStore<'_> {
        sync::SyncStore { p: self }
    }

    /// Moves forward whenever a write gives a record something to upload, local edits and
    /// merges alike, and never otherwise. A shell compares it with the value at its last sync
    /// start to sync soon after a change, without being told about every edit.
    pub fn change_seq(&self) -> crdt::Seq {
        crdt::Seq(self.next_seq)
    }

    /// The key that seals this profile's secrets. The first call may wait on an OS keyring
    /// prompt.
    pub(crate) fn vault_key(&mut self) -> Result<vault::Key, Error> {
        if let Some(key) = self.vault_key {
            return Ok(key);
        }
        let key = vault::load_or_create(&self.conn, self.device, self.key_store)?;
        self.vault_key = Some(key);
        Ok(key)
    }

    /// Run `f` in one SQLite transaction. `Tx::stamp()` mints at most one stamp and
    /// `Tx::seq()` at most one sequence number per transaction. Both are persisted to
    /// `meta` in the same transaction, so a crash can never reuse a stamp. A failing `f`
    /// rolls everything back.
    pub(crate) fn write<R>(
        &mut self,
        f: impl FnOnce(&mut db::Tx<'_>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let Profile { conn, clock, next_seq, device, .. } = self;
        let mut tx = db::Tx::begin(conn, clock, *device, next_seq)?;
        let r = f(&mut tx)?;
        tx.commit()?;
        Ok(r)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("the profile is open in another Vsesvit process")]
    Locked,
    #[error("profile schema {found} is newer than this build supports ({supported})")]
    TooNew { found: u32, supported: u32 },
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl OpenError {
    fn from_core(e: Error) -> OpenError {
        match e {
            Error::Db(e) => OpenError::Db(e),
            Error::Io(e) => OpenError::Io(e),
            other => OpenError::Io(std::io::Error::other(other.to_string())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Bookmark(#[from] bookmarks::BookmarkError),
    #[error(transparent)]
    Storage(#[from] ext_storage::StorageError),
    #[error(transparent)]
    Install(#[from] extensions::InstallError),
    #[error(transparent)]
    Vault(#[from] vault::VaultError),
    #[error("no such item")]
    NotFound,
    #[error("{} is asked for every time", .0.label())]
    AlwaysAsks(permissions::Permission),
    #[error(transparent)]
    EngineForm(#[from] search::FormError),
    #[error("the default search engine cannot be removed")]
    RemoveDefaultEngine,
}
