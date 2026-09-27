# vsesvit-core: design package (candidate 2)

Sketch files: `src/lib.rs` (Profile, paths, errors), `src/clock.rs` (Hlc, Lww, DeviceId), `src/pos.rs`, `src/bookmarks.rs`, `src/history.rs`, `src/prefs.rs`, `src/search.rs`, `src/session.rs`, `src/omnibox.rs`, `src/extensions/{mod,source,crx3,xpi,manifest,installer,storage}.rs`, `src/sync.rs`, `src/schema.sql`, `tests/convergence.rs`.

## Problem

Two thin shells (WinUI 3 + WebView2, GTK4 + WebKitGTK) need one platform-agnostic library for everything that is data or policy: bookmarks, history, session, prefs, search engines, extensions and their `chrome.storage`. Every one of those kinds must be stored today in a shape a future sync engine can consume without a migration, and concurrent bookmark-tree edits on several devices must converge to a valid tree. The non-obvious parts are (1) choosing a merge algebra that is convergent under any delivery order and idempotent under redelivery *without* a server or an operation log, (2) doing that for a tree, and (3) making the extension install pipeline crash-safe and re-runnable while WebView2 imposes its own rules on the unpacked directory (each version in its own dir, no top-level `_metadata/`, `key` in the manifest). Constraints honored from the grounding: single process per profile (lock), shells call core on the UI thread, downloads must not block it, data must survive a crash mid-write, and the design stays small: one SQLite file, no plugin system, no sync server.

## Usage (caller's view)

### Quickstart (what the shell author reads)

```rust
use vsesvit_core::{Profile, ProfilePaths};

let paths = ProfilePaths::default_profile()?;           // %LOCALAPPDATA%\Vsesvit\profiles\default  |  ~/.local/share/vsesvit/profiles/default
let profile = Profile::open(&paths)?;                   // takes the LOCK, runs migrations, reconciles extension dirs
// `profile` is Clone + Send + Sync (Arc inside). Hold one per app; every call is synchronous and short.
// The engine's own data (cookies, cache, service workers) lives in `paths.engine_dir()`; core never opens it.

profile.bookmarks()   // tree CRUD, star lookup
profile.history()     // record visits, deletions
profile.omnibox()     // typed text -> Intent; suggestions
profile.prefs()       // typed key/value
profile.search()      // search engines, default engine
profile.session()     // this device's windows/tabs; other devices' tabs (read-only)
profile.extensions()  // installer(), commit(), list(), set_enabled(), uninstall(), load_unpacked()
profile.ext_storage() // chrome.storage.local / .sync backing (Linux runtime)
profile.sync()        // the port a future sync engine drives; unused by shells
```

Threading rule, stated once: everything on `Profile` may be called from the UI thread and returns in microseconds to low milliseconds. The only slow operation is `Installer::run` (network + unzip); `Installer` holds no database handle, is `Send + Sync`, and is meant to be moved to a worker thread. The result (`Staged`) is `Send`; the shell hands it back to `extensions().commit()` on any thread.

### Windows shell (`vsesvit-winui`)

```rust
// app.rs — launch
let paths = ProfilePaths::default_profile()?;
let profile = Profile::open(&paths)?;
let opts = CoreWebView2EnvironmentOptions::new()?; opts.SetAreBrowserExtensionsEnabled(true)?;
let env = CoreWebView2Environment::CreateWithOptions(None, paths.engine_dir().as_os_str(), &opts)?;
// ...after the first CoreWebView2 exists:
for ext in profile.extensions().list()? {
    if ext.enabled { webview_profile.AddBrowserExtensionAsync(ext.dir.as_os_str())?; }   // dir = <profile>/extensions/<id>/<version>
}
if profile.prefs().get(&pref::RESTORE_SESSION)? {
    if let Some(snapshot) = profile.session().restore()? { self.open_windows(snapshot); }
}

// tab.rs — NavigationCompleted / DocumentTitleChanged
fn on_navigation_completed(&self, url: Url) {
    self.profile.history().record_visit(&url, None, Transition::Link).ok();
    let starred = !self.profile.bookmarks().find_by_url(&url).unwrap_or_default().is_empty();
    self.star_button.SetIsChecked(starred);
}
fn on_title_changed(&self, url: &Url, title: &str) { self.profile.history().set_title(url, title).ok(); }

// star button
fn on_star_clicked(&self) -> Result<()> {
    let bm = self.profile.bookmarks();
    match bm.find_by_url(&self.url)?.first() {
        Some(id) => bm.delete(*id)?,                                             // moves to the trash root; sync-visible
        None => { bm.add(NewBookmark::url(BookmarkId::OTHER, &self.title, &self.url), Place::Last)?; }
    }
    Ok(())
}

// bookmarks bar: reorder by drag
fn on_drop(&self, dragged: BookmarkId, onto: BookmarkId) -> Result<()> {
    self.profile.bookmarks().move_to(dragged, BookmarkId::BAR, Place::Before(onto))
}
fn rebuild_bar(&self) -> Result<()> {
    for b in self.profile.bookmarks().children(BookmarkId::BAR)? { self.bar.add_item(b.id, &b.title, b.url.as_ref()); }
    Ok(())
}

// omnibox
fn on_omnibox_submit(&self, text: &str) -> Result<()> {
    let target = match self.profile.omnibox().interpret(text)? { Intent::Navigate(u) => u, Intent::Search { url, .. } => url };
    self.webview.Navigate(&HSTRING::from(target.as_str()))
}
fn on_omnibox_text_changed(&self, text: &str) { self.popup.set(self.profile.omnibox().suggest(text, 8).unwrap_or_default()); }

// extensions dialog: install from pasted URL / id
fn on_install(&self, input: String) {
    let installer = self.profile.extensions().installer();       // Send + Sync, no DB
    let profile = self.profile.clone();
    let dq = DispatcherQueue::GetForCurrentThread()?;
    std::thread::spawn(move || {
        let staged = installer.run(&input, &mut |phase| log::debug!("{phase:?}"));   // resolve -> fetch -> verify -> stage
        dq.TryEnqueue(&DispatcherQueueHandler::new(move || {
            match staged.and_then(|s| profile.extensions().commit(s)) {              // idempotent: same id+version -> same dir, same row
                Ok(installed) => webview_profile.AddBrowserExtensionAsync(installed.dir.as_os_str()),
                Err(e) => show_error(&e),
            }
        }))
    });
}

// window close / tab change (shell debounces ~1s): one row update
fn persist_session(&self) { self.profile.session().save(&self.snapshot()).ok(); }
```

### Linux shell (`vsesvit-gtk`)

```rust
// window.rs
let session = webkit::NetworkSession::new(Some(paths.engine_dir()), Some(paths.engine_cache_dir()));
webview.connect_load_changed(clone!(#[strong] profile, #[weak] star, move |wv, ev| {
    if ev != webkit::LoadEvent::Committed { return; }
    let Ok(url) = Url::parse(&wv.uri().unwrap_or_default()) else { return };
    profile.history().record_visit(&url, wv.title().as_deref(), Transition::Link).ok();
    star.set_active(!profile.bookmarks().find_by_url(&url).unwrap_or_default().is_empty());
}));

// extension runtime bootstrap (per enabled extension)
for ext in profile.extensions().list()?.into_iter().filter(|e| e.enabled) {
    let m = &ext.manifest;                                   // parsed, localized; ext.dir holds the files
    for cs in &m.content_scripts { ucm.add_script(&user_script_for(&ext.dir, cs, &ext.id)); }
    if let Some(dnr) = profile.extensions().dnr_rulesets(&ext.id)? { filters.compile(&ext.id, dnr).await?; }
    runtime.spawn_background(&ext);                          // hidden WebView, m.background
}

// chrome.storage bridge: script message handler "ext" registered in the extension's world
fn on_storage_call(profile: &Profile, ext: &ExtensionId, call: StorageCall) -> Result<serde_json::Value> {
    let st = profile.ext_storage();
    Ok(match call {
        StorageCall::Get { area, keys } => serde_json::Value::Object(st.get(ext, area, keys.as_deref())?),
        StorageCall::Set { area, items } => { let ch = st.set(ext, area, items)?; runtime.emit_on_changed(ext, area, &ch); json!(null) }
        StorageCall::Remove { area, keys } => { let ch = st.remove(ext, area, &keys)?; runtime.emit_on_changed(ext, area, &ch); json!(null) }
        StorageCall::Clear { area } => { let ch = st.clear(ext, area)?; runtime.emit_on_changed(ext, area, &ch); json!(null) }
    })
}

// install from the extensions page
let installer = profile.extensions().installer();
glib::spawn_future_local(clone!(#[strong] profile, async move {
    let staged = gio::spawn_blocking(move || installer.run(&input, &mut |_| {})).await.unwrap();
    match staged.and_then(|s| profile.extensions().commit(s)) {
        Ok(installed) => runtime.load(&installed),
        Err(e) => toast.show(&e.to_string()),
    }
}));
```

## Shape

### Module map (`crates/vsesvit-core`)

| file | owns | depends on |
|---|---|---|
| `lib.rs` | `Profile`, `ProfilePaths`, `Error`, the write-transaction helper `Tx` | rusqlite, directories |
| `clock.rs` | `DeviceId`, `Hlc`, `HlcClock`, `Lww<T>` | – |
| `pos.rs` | `Pos` fractional sibling index | – |
| `bookmarks.rs` | `BookmarkRecord` (synced), `Bookmarks` API, tree repair | clock, pos |
| `history.rs` | `VisitRecord`, `DeletionRecord` (synced), derived `urls`, `History` API | clock |
| `prefs.rs`, `search.rs`, `session.rs` | one synced record + API each | clock |
| `omnibox.rs` | pure `classify`, `Omnibox` API over history+bookmarks+search | – |
| `extensions/` | `ExtensionRecord`, `Extensions` API, `Installer` pipeline, CRX3, XPI, manifest, `ExtStorage` | ureq, zip, rsa, sha2, base64 |
| `sync.rs` | `Kind`, `Record`, `Change`, `Merge`, `SyncPort` | every record type |

Tracing any shell call touches at most: `lib.rs` (transaction) → one kind module → `clock.rs`/`pos.rs`. Per laziness-protocol no traits between kinds and the store; each kind module owns its SQL.

### (a) Data model: one algebra, eight kinds

Every synced kind is a **state-based CRDT record**: a full row whose mutable fields are `Lww<T> { value, at: Hlc }` registers. Merge is field-wise "higher `Hlc` wins", which is a lattice join: commutative, associative, idempotent. That single fact is what makes "apply remote changes idempotently and in any order" true by construction rather than by careful engine code. There is **no operation log**; the row is the truth and the sync unit.

- **Ids.** UUID v7, 16-byte BLOB primary keys, generated on the creating device. v7 for b-tree locality (visits are append-heavy); the embedded timestamp is never read as data. Natural keys where the world already has them: `ExtensionId` (store id string), pref key, `(ext_id, area, key)`, `DeviceId` for sessions. Well-known constant ids for bookmark roots and prepopulated search engines so they need no sync.
- **Clock.** `Hlc = (wall_ms: u48, counter: u16, device: DeviceId)`, 24-byte big-endian BLOB, lexicographic order == causal-ish order. The clock persists `last_wall` in `meta` on every write transaction so a backwards wall clock after restart cannot mint an older stamp. `apply()` folds every incoming `Hlc` into the local clock, so a user edit made *after seeing* remote state always outranks it (per encode-lessons-in-structure: the tie-break policy is inside `Hlc::cmp`, nowhere else).
- **Change tracking.** One profile-wide monotonic `seq` (in `meta`), stamped on a row whenever its state becomes something **no peer has told us**: every local write, and a remote apply whose merge outcome is `Merged` (local had newer fields than the incoming record). `TookIncoming`/`Unchanged` leave `seq` alone (0 for rows first learned from a peer). The engine enumerates `WHERE seq > watermark`. Rows with `seq = 0` are never pushed as "changes", only as part of `snapshot()` for a first upload. This is single-source-of-truth per invariant: the row is the change.
- **Tombstones.** No separate table. Each kind encodes deletion in its own register so delete-vs-edit is decided by the same LWW rule as every other conflict (table below). Purge is an explicit engine-driven call, never automatic while sync is off (a tombstoned bookmark row costs ~150 bytes).

| kind | key | mutable registers | deletion | merge |
|---|---|---|---|---|
| bookmark | uuid | `title`, `url`, `placement{parent,pos}` | `placement.parent == TRASH` | per-field LWW; tree repaired after apply (below) |
| visit | uuid | none (immutable) | history deletion directive | union; `title = a.title.or(b.title)` |
| history deletion | uuid | none | never (grow-only) | union; applied to local visits on arrival |
| session | device id | whole snapshot | `deleted` flag inside the register | whole-record LWW (single writer) |
| extension | ext id | `installed`, `enabled` | `installed = false` | per-field LWW; unpacked extensions never synced |
| ext storage.sync | (ext, key) | `value: Option<Json>` | `value = None` | LWW per key |
| pref | key | `value: Option<Json>` | `None` = back to default | LWW per key; local-scope prefs never synced |
| search engine | uuid | name, keyword, url, suggest, favicon | `deleted` flag inside the register | whole-record LWW (fields edited together in one dialog) |
| passwords, autofill | reserved `Kind` variants | – | – | slot only (see risks) |

Reading list is not a kind: it is the well-known root folder `BookmarkId::READING_LIST`. Per subtract-before-you-add.

**Bookmark tree convergence.** Deletion *is* a move: `delete(id)` writes `placement = (TRASH, pos)`. So "folder deleted on A while B adds a child" and "A deletes, B moves the child out" are both plain LWW on `placement`, no special casing. Merged placements can still form a cycle (A moved X under Y, B moved Y under X) or dangle (parent unknown or not a folder), so the *effective* tree is a pure function of the merged rows, recomputed after every remote apply and cached in the local-only `bookmark_tree` table:

1. Start with every live node attached to `OTHER` (a valid tree).
2. Replay each node's single latest placement in ascending `Hlc` order; skip a move whose target is the node itself, a descendant of it, an unknown id, or a non-folder. (Kleppmann's move-op tree rule applied to state instead of a log; every device sees the same set of placements, so every device computes the same tree.)
3. Hoist: for each direct child `F` of `TRASH` (trash move at `t_F`), any descendant `n` with `placement.at > t_F` is re-attached, with its subtree, under `OTHER`. A bookmark created or moved *after* the folder was deleted survives; children the deleter saw stay deleted. Deterministic, no data loss, no ping-pong because the repair is a view: stored placements are never rewritten.

Local ops never need repair: they are validated against the effective tree (parent is a live folder, not in the node's own subtree) and update `bookmark_tree` incrementally; the property test asserts incremental == full rebuild after every op. Sibling order is `ORDER BY pos, id` where `pos` is a fractional index (`pos.rs`); concurrent inserts at the same gap collide on `pos` and fall back to id order, still convergent.

### (b) On-disk format and profile layout

One SQLite file, `rusqlite` with `bundled`, WAL, `synchronous=NORMAL`, `foreign_keys=ON`, `user_version` for forward-only migrations kept as SQL strings in `schema.sql`. One file because one `seq` space and one transaction boundary are what make "changes since" and "apply batch atomically" trivial, and because the profile lock is then one thing. WAL gives atomic commits across a crash mid-write; NORMAL may lose the last few milliseconds of committed writes on power loss but never leaves the file inconsistent (the same setting Firefox uses for `places.sqlite`).

```
<profile>/
  LOCK                 std::fs::File::try_lock() held for the Profile's lifetime; Error::ProfileInUse otherwise
  vsesvit.db (+ -wal, -shm)
  extensions/<id>/<version>/            unpacked, immutable once present (presence == complete)
  extensions/<id>/.staging-<random>/    transient; removed by reconcile() on open
  engine/, engine-cache/                handed to WebView2 / WebKitGTK; opaque to core
```

Schema (full DDL in `schema.sql`): synced tables `bookmarks`, `visits`, `history_deletions`, `sessions`, `extensions`, `ext_storage`, `prefs`, `search_engines`, each with `seq INTEGER` + index and `*_hlc BLOB` per register; derived local tables `bookmark_tree` (effective parent, trashed flag) and `urls` (visit_count, last_visit, title, frecency, sha256 url_hash for deletion directives); `meta` (device_id, device_name, seq_head, hlc_last_wall, `sync.*` cursors). Everything the shells query has a plain index: `bookmarks(url)`, `bookmark_tree(eff_parent)`, `urls(frecency)`, `visits(url_id, at_ms)`. Explicit typed columns rather than a JSON blob per row so `sqlite3` on the file shows the truth and the clocks.

### (c) Extension install pipeline

Types make the phases explicit; each is a value the next function consumes, so a crash between phases loses nothing that cannot be rerun:

```
&str ──resolve──▶ InstallSource ──fetch──▶ Package ──verify──▶ VerifiedPackage ──stage──▶ Staged ──commit──▶ InstalledExtension
   (pure parse)    (network, worker)      (pure: CRX3 sig / AMO hash)  (unzip to .staging, manifest)   (rename + DB row, any thread)
```

- `InstallSource`: `ChromeWebStore(ChromeExtId)` from a bare `[a-p]{32}` id or any `chromewebstore.google.com/detail/…/<id>` / legacy `chrome.google.com/webstore/detail/…` URL; `Amo(slug|guid)` from `addons.mozilla.org/…/addon/<slug>/`; `RemoteCrx(Url)`, `RemoteXpi(Url)`; `LocalPackage(PathBuf)`; `Unpacked(PathBuf)`.
- `fetch`: CWS via the `update2/crx?response=redirect…` endpoint (204 ⇒ `InstallError::NotAvailable`), AMO via the v5 API (gives `guid`, file url, `sha256:` hash). `ureq` with a size cap; no UI thread.
- `verify`: CRX3 header parsed by a 40-line protobuf reader (fields 2, 3, 10000; only varint + length-delimited), every RSA proof verified over `"CRX3 SignedData\0" ‖ u32le(len) ‖ signed_header_data ‖ zip` with PKCS#1 v1.5 SHA-256, the proof key deriving to `crx_id` is required and must equal the requested id, header bytes containing zip EOCD magics rejected. ECDSA proofs are parsed and ignored (no P-256 crate in the list; open question). XPI: no PKCS#7 verification; trust is TLS to AMO plus the API-reported SHA-256 (`Verification::AmoHash`). Direct `.xpi` URLs get `Verification::TransportOnly`.
- `stage`: unzip into `extensions/<id>/.staging-<rand>/` with path sanitization (zip-slip), drop top-level `_metadata/`, write the developer key into `manifest.json` as `key` when absent, parse+validate the manifest, resolve `__MSG_*__` for display. For `Unpacked`, no copy: `Staged.location = InPlace(dir)`, id from `key` or SHA-256 of the canonical path; flagged `synced = false`.
- `commit` (holds the DB lock): if `<id>/<version>/` already exists, delete the staging dir and reuse it (re-running an install converges); else `rename` staging → final (atomic on both OSes), then upsert the row (`installed=true`, `enabled` untouched if the row existed, else `true`). Crash after rename, before the row: `reconcile()` on next open adopts the newest complete version dir it finds for that id, removes stray `.staging-*` and version dirs the row does not reference, and marks rows whose dir is missing as `Pending` so the shell can re-download. Per make-operations-idempotent: `commit` and `reconcile` are both "converge to the state the row describes".
- `uninstall`: `installed = Lww(false)`, best-effort `remove_dir_all` (Windows may hold files open; `reconcile()` finishes it next start). `set_enabled` is an `Lww<bool>` write; the shells apply it to their runtime.
- Manifest model (`manifest.rs`): MV2/MV3 superset with `action` unified from `action|browser_action|page_action`, `background` as an enum, `content_scripts`, `declarative_net_request.rule_resources`, `web_accessible_resources` (both shapes), `browser_specific_settings.gecko.id`, `key`, `default_locale`; unknown fields kept in `extra`. `Extensions::dnr_rulesets(id)` parses the rule files so the Linux shell only translates to content blockers.

### (d) Public API

The usage section is the API. Sub-API structs (`Bookmarks<'_>`, …) are zero-cost borrows of `Profile`; every write goes through `Profile::write(|tx| …)`, which opens one SQLite transaction, hands out `tx.stamp()` (next `Hlc`) and `tx.seq()` (next `seq`), and persists the clock; validation happens in the API method (URL parse, folder checks, quotas), never in SQL (per boundary-discipline). Index:

| handle | methods |
|---|---|
| `Profile` | `open`, `open_in_memory`, `paths`, `device_id`, and one accessor per handle below |
| `Bookmarks` | `add`, `get`, `children`, `find_by_url`, `rename`, `set_url`, `move_to`, `delete`, `path`, `folders`, `search`, `purge_trash`, `snapshot_tree`, `check_invariants` |
| `History` | `record_visit`, `set_title`, `recent`, `visits_of`, `search`, `delete_url`, `delete_range`, `clear` |
| `Omnibox` | `interpret`, `suggest`, `engine_for`; pure `classify` |
| `Prefs` | `get`, `set`, `reset`, `is_set` over the static `pref::*` registry |
| `SearchEngines` | `list`, `default`, `set_default`, `by_keyword`, `add`, `update`, `remove`, `search_url` |
| `Session` | `save`, `restore`, `clear`, `remote_devices`, `forget_device`, `set_device_name` |
| `Extensions` | `installer`, `commit`, `install_blocking`, `load_unpacked`, `list`, `pending`, `get`, `set_enabled`, `uninstall`, `reconcile`, `dnr_rulesets` |
| `Installer` (worker thread) | `resolve`, `fetch`, `verify`, `stage`, `run`, `stage_unpacked` |
| `ExtStorage` | `get`, `set`, `remove`, `clear`, `bytes_in_use` |
| `SyncPort` | `head`, `changes_since`, `snapshot`, `apply`, `get_cursor`, `set_cursor`, `purge_tombstones`, `now` |

### (e) How a sync engine plugs in

`sync.rs` exists now, is exercised by the property test now, and is the entire surface an engine will use:

```rust
let port = profile.sync();
let since = port.get_cursor("server.pushed_seq")?.map(Seq::from_bytes).unwrap_or(Seq::ZERO);
let out: Vec<Change> = port.changes_since(since, Kind::ALL, 500)?;   // push these; each carries kind, key, seq, full Record (serde)
port.set_cursor("server.pushed_seq", &out.last().map(|c| c.seq).unwrap_or(since).to_bytes())?;
let report: ApplyReport = port.apply(&incoming)?;                    // any order, any duplication, one transaction; bumps seq only for Merged rows
port.purge_tombstones(Kind::Bookmarks, older_than)?;                 // engine decides when every device has seen the trash
```

`Record` is a closed enum of the eight record structs, each `Serialize + Deserialize + Merge`, so the engine can ship them as JSON/CBOR without knowing the schema, and a smart server can run the same `merge`. Cursors live in core's `meta` table so an apply and its cursor advance commit together. Adding sync later adds code and a `sync.*` namespace in `meta`; it changes no row.

### (f) The convergence property

`tests/convergence.rs`: proptest builds 2–4 in-memory profiles with distinct `DeviceId`s, generates an interleaved script of local bookmark ops (add url/folder, rename, move across folders, reorder, delete node, delete folder, restore from trash) resolved against each device's *current visible tree*, and pairwise partial syncs where the delivered batch is shuffled, truncated, and partly duplicated. After a final all-to-all exchange until no device has changes for any other, it asserts: every device's `Bookmarks::snapshot_tree()` is byte-identical; `check_invariants()` holds on each (every live node reaches exactly one root, no cycles, `pos` unique per sibling set, `bookmark_tree` == fresh rebuild); re-applying any device's full `snapshot(Kind::Bookmarks)` to any other yields an `ApplyReport` with zero `took_incoming` and zero `merged`; and after settling no device has anything left to send. Two pinned scenarios fix the deletion policy: a bookmark added under a folder *after* another device deleted it ends up live under `OTHER` everywhere; one added *before* the deletion (and seen by the deleter) stays trashed everywhere. Two smaller properties: the `Merge` impls for every `Record` variant satisfy commutativity, associativity and idempotence on random pairs/triples; and `incremental bookmark_tree == rebuild()` after every local op.

## Synthesis decision

*Filled in by arena.*

## Tradeoffs accepted

- We accept state-based records (full row per change, no op log) in exchange for merge that is a lattice join: idempotent and order-independent for free, and a server that can be a dumb key/value store. Intermediate states are lost; nothing here needs them.
- We accept a 24-byte `Hlc` per register (3 per bookmark) in exchange for a tie-break that never depends on anything outside the value and a schema readable with `sqlite3`.
- We accept "delete = move to TRASH" and a view-level repair in exchange for one conflict rule for the whole tree; the cost is a `bookmark_tree` cache and a full rebuild (O(n·depth), ~ms for 100k nodes) after each remote apply.
- We accept that a hoisted or cycle-broken node lands in `OTHER` rather than "near where it was" in exchange for a repair that is a pure function of state and never writes new clocks.
- We accept per-visit history sync with hashed deletion directives (Chrome's shape) in exchange for "clear history" being one small row; directive rows are retained forever.
- We accept whole-record LWW for search engines and sessions (one writer or one dialog) instead of per-field, per laziness-protocol.
- We accept omnibox suggestions via SQL `LIKE` over `urls` ordered by frecency in exchange for zero extra index machinery; the upgrade path (in-memory URL index or FTS5) is a private change inside `omnibox.rs`.
- We accept no signature verification for XPI (TLS + AMO hash) rather than hand-rolling PKCS#7/X.509 with no crate for it.
- We accept a single SQLite file for all kinds including session snapshots and `storage.local` writes; SQLite in WAL handles thousands of small commits per second, far beyond a browser's write rate.

## Alternatives considered

- **Operation log + per-device vector clocks (op-based CRDT).** Better history of intent, but requires reliable causal delivery or an ever-growing log, and "enumerate local changes" becomes log slicing with compaction. The grounding asks for any-order idempotent batches with no server semantics; state-based wins.
- **Chrome-style server-mediated conflict resolution (client keeps base version, server picks).** Needs a server to exist to define the semantics; we must converge without one.
- **Separate `deleted` flag + tombstone table for bookmarks.** Makes delete-vs-move a second conflict rule and folder deletion N tombstones; the trash-root move is one rule and one row.
- **Integer sibling positions with reindexing.** Every reorder rewrites siblings and produces spurious conflicts; fractional `pos` makes a reorder a one-row change.
- **Several SQLite files (places / extensions / session / storage).** Firefox's shape; costs cross-file atomicity, several locks and several seq spaces for no gain at this scale.
- **A generic `Store<K, V: Merge>` trait with one table per implementor.** Speculative abstraction; each kind's SQL is ~10 statements and its queries are specific (tree, frecency, prefix). Kinds share `Hlc`/`Lww`/`seq` and the `Record` enum, nothing more.

## Open questions and risks

- Should CRX3 ECDSA proofs be verified (add `p256`), and should store installs require the Web Store publisher key hash, or is developer-key verification plus id match enough for v1?
- Is "hoist to `OTHER`" the wanted policy for a bookmark added under a folder another device deleted, or should the folder be revived (Firefox)? The algorithm supports either; the choice is one branch in `repair()`.
- Per-visit history sync can be large (hundreds of thousands of rows). Should the engine cap history at N days by policy, or should history simply not be synced in the first engine? Nothing in core changes either way.
- Does `rusqlite` `bundled` 0.40 enable `SQLITE_ENABLE_FTS5`/`JSON1` on both targets? Only matters if the omnibox upgrade path is taken.
- `synchronous=NORMAL` vs `FULL`: is losing the last ~ms of session state on power loss acceptable? (App crashes lose nothing either way.)
- Passwords/autofill: the slot is a `Kind` variant and a rule (secret material only in the OS store; the synced record carries a keyring reference locally and an engine-encrypted blob remotely). Should the row shape be fixed now, or left until the keyring integration exists?

## Next implementation step

Implement `clock.rs` (`Hlc`, `HlcClock`, `Lww`) and `bookmarks.rs` `merge` + `repair()` with the in-memory `Profile` and the convergence property test, before any shell code.
