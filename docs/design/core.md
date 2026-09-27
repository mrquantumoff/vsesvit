# vsesvit-core design

Chosen by an architect arena of three independent designs (`arena/candidate-{1,2,3}.md`) and a cross-judge (`arena/judge.md`).
This document is candidate 1 with the synthesis below applied. Where the body and the synthesis disagree, the synthesis wins.

## Problem

Both shells are thin and call one library, from their UI thread, for all data and policy. That library has to meet
three requirements that pull against each other. It must answer per-navigation and per-keystroke queries without
visible latency. It must install extensions from two stores plus unpacked dirs, with network I/O off the UI thread and
idempotent re-runs. Most importantly, every synced kind (bookmarks, history, tabs, extensions, `storage.sync`, prefs,
search engines, and later passwords and autofill) must be stored so that a sync engine added later can enumerate local
changes, apply remote batches idempotently in any order, propagate deletions, and converge bookmark trees under
concurrent move/reorder/delete. That last part must work without a migration.

Constraints that shaped the design:

- One process per profile, so a lock.
- Crash safety.
- No sync server or plugin system now.
- WebView2 owns the extension runtime on Windows, including `chrome.storage`. It loads extensions *in place* from a
  folder, persists them in its own profile, and drops an extension whose files change.
- Chromium refuses to load unpacked dirs that contain `_`-prefixed entries such as `_metadata/`.
- WebKitGTK has no extension runtime, so the Linux shell builds one on top of core.

The design has one idea and applies it everywhere. **Each record is a join-semilattice, and any global invariant is a
pure function of the merged records.** Merging is then order-free and idempotent by construction. The bookmark tree is
valid on every device because every device computes it with the same function from the same records. Sync is never
asked to preserve tree validity.

## Usage (caller's view)

### Linux shell (GTK4 + libadwaita + WebKitGTK 6)

```rust
use vsesvit_core::{Profile, OpenOptions, OpenError, Url};
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::history::Transition;
use vsesvit_core::extensions::{InstallJob, InstallSource};
use vsesvit_core::ext_storage::Area;
use vsesvit_core::prefs::keys;

type Core = Rc<RefCell<Profile>>;

fn start(app: &adw::Application) -> anyhow::Result<Core> {
    let profile = match Profile::open(&Profile::default_root("Default"), OpenOptions::default()) {
        Err(OpenError::Locked) => return forward_argv_to_running_instance(),   // single-instance handoff
        other => other?,
    };
    let core: Core = Rc::new(RefCell::new(profile));
    let paths = core.borrow().paths().clone();
    let net = webkit6::NetworkSession::new(paths.engine_data.to_str(), paths.engine_cache.to_str());

    let mut p = core.borrow_mut();
    if let Some(snapshot) = p.session().restore()? { windows::restore(app, &net, &snapshot); }
    p.history().expire()?;
    for ext in p.extensions().list()? { if ext.enabled { runtime::load(&ext); } }   // dir + parsed Manifest
    let work = p.extensions().reconcile()?;             // installed on another device, missing here
    drop(p);
    for job in work.install { spawn_install(&core, job); }
    Ok(core)
}

// 1. every committed navigation: record a visit, update the star
web_view.connect_load_changed(clone!(#[strong] core, #[weak] star, move |wv, ev| {
    if ev != webkit6::LoadEvent::Committed { return; }
    let Some(url) = wv.uri().and_then(|u| Url::parse(&u).ok()) else { return };
    let mut p = core.borrow_mut();
    if let Err(e) = p.history().record_visit(&url, Transition::Link) { log::warn!("history: {e}"); }
    star.set_active(p.bookmarks().is_bookmarked(&url));                 // in-memory hash lookup
}));
web_view.connect_title_notify(clone!(#[strong] core, move |wv| {
    if let (Some(u), Some(t)) = (wv.uri(), wv.title()) {
        if let Ok(url) = Url::parse(&u) { let _ = core.borrow_mut().history().set_title(&url, &t); }
    }
}));

// 2. omnibox
entry.connect_changed(clone!(#[strong] core, move |e| {
    let items = core.borrow_mut().omnibox().suggest(&e.text(), 8).unwrap_or_default();
    popover::show(items);                                               // Vec<Suggestion { source, title, target }>
}));
entry.connect_activate(clone!(#[strong] core, #[weak] web_view, move |e| {
    if let Ok(Some(target)) = core.borrow_mut().omnibox().resolve(&e.text()) {
        web_view.load_uri(target.url().as_str());
    }
}));

// 3. star button and bookmarks bar
star.connect_clicked(clone!(#[strong] core, #[weak] web_view, move |_| {
    let Some(url) = web_view.uri().and_then(|u| Url::parse(&u).ok()) else { return };
    let title = web_view.title().unwrap_or_default();
    let mut p = core.borrow_mut();
    let mut bm = p.bookmarks();
    let r = match bm.find_by_url(&url).first() {
        Some(existing) => bm.remove(existing.id),
        None => bm.add_url(BookmarkId::OTHER, InsertAt::End, &title, &url).map(drop),
    };
    if let Err(e) = r { toast(&e) }
    bar.set_items(bm.children(BookmarkId::TOOLBAR));                     // Vec<BookmarkNode>, display order
}));
// drag-and-drop in the bar or the manager:
core.borrow_mut().bookmarks().move_to(dragged, target_folder, InsertAt::Index(drop_index))?;   // Err(WouldCycle) -> refuse drop

// 4. extensions: a store URL pasted into the omnibox or chosen in the manager
let job = core.borrow_mut().extensions().prepare_install(InstallSource::parse(&text)?)?;
spawn_install(&core, job);

fn spawn_install(core: &Core, job: InstallJob) {
    let core = core.clone();
    glib::spawn_future_local(async move {
        let staged = gio::spawn_blocking(move || job.run()).await.expect("install thread panicked");
        match staged.map_err(Into::into).and_then(|s| core.borrow_mut().extensions().commit(s)) {
            Ok(Some(ext)) => runtime::load(&ext),        // content scripts, background view, action popup
            Ok(None) => {}                               // uninstalled on another device while downloading
            Err(e) => toast(&e),
        }
    });
}

// The Linux WebExtensions runtime's chrome.storage bridge
fn storage_set(core: &Core, ext: &ExtensionId, area: Area, items: BTreeMap<String, Value>) -> Result<(), Error> {
    let changes = core.borrow_mut().ext_storage().set(ext, area, items)?;   // quota-checked for Area::Sync
    runtime::fire_storage_on_changed(ext, area, &changes);
    Ok(())
}

// 5. prefs,  6. session (debounced ~2 s after any tab change, and on shutdown)
let theme = core.borrow_mut().prefs().get(&keys::THEME);
core.borrow_mut().session().save(&windows::snapshot())?;   // TabSnapshot.restore_state = webkit session-state bytes
```

### Windows shell (WinUI 3 + WebView2)

```rust
thread_local! { static CORE: RefCell<Option<Profile>> = const { RefCell::new(None) }; }
fn core<R>(f: impl FnOnce(&mut Profile) -> R) -> R { CORE.with_borrow_mut(|p| f(p.as_mut().expect("profile open"))) }

async fn start() -> anyhow::Result<()> {
    let chromium = CoreWebView2Environment::GetAvailableBrowserVersionString(&HSTRING::new())?;
    let profile = Profile::open(&Profile::default_root("Default"),
        OpenOptions { chrome_version: chromium_part(&chromium), ..OpenOptions::default() })?;
    let udf = HSTRING::from(profile.paths().engine_data.as_os_str());
    CORE.set(Some(profile));

    let opts = CoreWebView2EnvironmentOptions::new()?;
    opts.SetAreBrowserExtensionsEnabled(true)?;
    let env = CoreWebView2Environment::CreateWithOptionsAsync(&HSTRING::new(), &udf, &opts)?.await?;
    webview.EnsureCoreWebView2WithEnvironmentAsync(&env)?.await?;
    let wv = webview.CoreWebView2()?;

    // 1. committed navigation
    wv.ContentLoading(&TypedEventHandler::new(clone!(star => move |s: Ref<CoreWebView2>, _| {
        let Ok(url) = Url::parse(&s.ok()?.Source()?.to_string()) else { return Ok(()) };
        let starred = core(|p| {
            if let Err(e) = p.history().record_visit(&url, Transition::Link) { log::warn!("{e}") }
            p.bookmarks().is_bookmarked(&url)
        });
        star.SetIsChecked(&IReference::from(starred))
    })))?;

    // 4. extensions: bring WebView2's persisted set in line with core's
    let work = core(|p| p.extensions().reconcile())?;
    for job in work.install { spawn_install(job); }
    sync_webview2_extensions(&wv.Profile()?).await?;
    Ok(())
}

fn spawn_install(job: InstallJob) {
    let dq = DispatcherQueue::GetForCurrentThread().unwrap();
    std::thread::spawn(move || {
        let staged = Cell::new(Some(job.run()));                        // Result<StagedInstall, InstallError>: Send
        let _ = dq.TryEnqueue(&DispatcherQueueHandler::new(move || {
            let Some(staged) = staged.take() else { return Ok(()) };
            match staged.map_err(Into::into).and_then(|s| core(|p| p.extensions().commit(s))) {
                Ok(Some(_)) => ui::spawn_local(async { sync_webview2_extensions(&webview2_profile()).await }),
                Ok(None) => {}
                Err(e) => ui::show_error(&e),
            }
            Ok(())
        }));
    });
}

/// Idempotent. `engine_id == None` means "WebView2 has not loaded this dir yet" (new install or update).
/// Add before remove: adding a dir with an existing id replaces that extension in place.
async fn sync_webview2_extensions(wvp: &CoreWebView2Profile) -> windows::core::Result<()> {
    let wanted = core(|p| p.extensions().list()).unwrap_or_default();
    for ext in wanted.iter().filter(|e| e.engine_id.is_none()) {
        let added = wvp.AddBrowserExtensionAsync(&HSTRING::from(ext.dir.as_os_str()))?.await?;
        let wv_id = added.Id()?.to_string();
        if let Err(e) = core(|p| p.extensions().set_engine_id(&ext.id, &wv_id)) { log::warn!("{e}") }
    }
    let wanted = core(|p| p.extensions().list()).unwrap_or_default();
    for w in wvp.GetBrowserExtensionsAsync()?.await? {
        let id = w.Id()?.to_string();
        match wanted.iter().find(|e| e.engine_id.as_deref() == Some(id.as_str())) {
            None => w.RemoveAsync()?.await?,                                // uninstalled (here or elsewhere)
            Some(e) if e.enabled != w.IsEnabled()? => w.EnableAsync(e.enabled)?.await?,
            Some(_) => {}
        }
    }
    Ok(())
}
```

### A future sync engine (not built now)

```rust
// crates/vsesvit-sync. Network on a worker thread; each call below runs on the UI thread.
for &kind in Kind::ALL {
    let cursor = up_cursor(p, kind)?;                                   // p.sync().engine_state("up/<code>")
    let batch = p.sync().changes_since(kind, cursor, 500)?;            // ChangeBatch { records, upto, more }
    worker.upload(kind, batch.records);                                // encrypt + PUT; then, on the UI thread:
    p.sync().set_engine_state(&format!("up/{}", kind.code()), &batch.upto.0.to_be_bytes())?;
}
let report = p.sync().apply(downloaded_records)?;                      // any order, dupes fine, one tx per batch
if report.changed.bookmarks  { bar.refresh() }
if report.changed.extensions { let w = p.extensions().reconcile()?; /* run w.install, unload w.removed */ }
for (ext, changes) in report.changed.ext_storage { runtime::fire_storage_on_changed(&ext, Area::Sync, &changes) }
```

## Shape

### Crate and module map

Keep the proposed workspace. `vsesvit-core` has no `cfg(target_os)` anywhere and no system dependencies (SQLite is
bundled), so `cargo test` runs the same on both hosts. Modules: `crdt`, `bookmarks`, `history`, `session`, `prefs`,
`search` (engines and omnibox), `reading_list`, `extensions/{mod, install, crx, manifest}`, `ext_storage`, `sync`, and a
private `db`. Every trace (for example `history::record_visit`, then `Profile::write`, then `db::Tx`) spans at most
three files.

### Merge primitives (`crdt.rs`)

- **`Stamp { hlc, device }`** is a hybrid logical clock value, `(ms << 16) | counter`, plus a random nonzero 64-bit
  `DeviceId`. It is totally ordered and stored as a 16-byte big-endian BLOB, so byte order equals `Ord`. The clock is
  persisted in `meta` inside every write transaction, so a crash can never reuse a stamp. `observe()` runs on every
  incoming stamp, so an edit made after seeing X always beats X, even with skewed clocks.
- **`Lww<T>`**: join is the max by `(stamp, value)`. The value tiebreak keeps the order total even if a copied profile
  produces duplicate stamps. `set()` of an unchanged value mints no stamp, so idempotent UI actions cause no sync
  traffic.
- **Grow-only sets** (history visits, history deletion directives): join is union.
- **Terminal tombstones** (bookmarks, search engines): deleted absorbs alive.
- **`Extra`**: every mutable synced field has the wire shape `{"v": .., "at": ".."}`. Unknown fields from a newer build
  therefore merge as `Lww<JsonText>` and are uploaded again intact. Without that, the first old device to re-upload a
  record would erase a new field everywhere. A future field with a different merge rule needs a new `Kind`.

Each record type implements `Lattice`. Merge is commutative, associative and idempotent, per record, and that alone
satisfies "apply in any order, idempotently".

### Per-kind model

| Kind (code) | Key | Fields and merge | Deletion |
|---|---|---|---|
| Bookmarks (1) | random UUID; 4 fixed root UUIDs, never stored | `placement: Lww<{parent,pos}>` (one register, so a move is atomic), title/url LWW, `added_ms` min | terminal tombstone that **keeps placement** |
| HistoryPages (2) | URL (natural) | title LWW, `visits` grow-only set of `{at_ms, device, transition}` | derived: a page with no surviving visits disappears |
| HistoryDeletions (3) | random UUID | immutable `{url?, from_ms, to_ms}`, grow-only set | expire with retention |
| Sessions (4) | DeviceId | whole `SessionSnapshot` in one `Lww<Option<..>>`, **one writer per record** | `None` = forgotten device |
| Extensions (5) | extension id | desired state only: `store`, `installed`, `enabled` LWW | `installed = false` (reinstall allowed) |
| ExtStorageSync (6) | (ext, key) | `Lww<Option<JsonText>>` per key, which is Chrome's own semantics | `None` |
| Prefs (7) | key | `Lww<Option<JsonText>>`; `Scope::Local` rows never exported | `None` = default |
| SearchEngines (8) | `builtin:*` or UUID | name/keyword/urls LWW; built-ins are code at `Stamp::ZERO`, never seeded | terminal tombstone |
| ReadingList (9) | URL | title/present/read LWW, `added_ms` min | `present = false` (re-add allowed) |
| Passwords (10), Autofill (11) | reserved | same table conventions; secret columns hold ciphertext sealed by a DPAPI/libsecret key. The lattice works on ciphertext unchanged | terminal tombstone |

Rule of thumb: random-id kinds use terminal tombstones, because a re-add is a new thing. Natural-key kinds use LWW
presence, because the key can come back. "Exactly one of" constraints are single registers (default engine is a pref;
the active tab is an index), never a flag on many records.

### Bookmark tree convergence (`bookmarks.rs`)

Merge never produces a tree. `materialize(&records) -> Tree` does, and it is pure, deterministic and O(n log n):

1. **Unknown or non-folder parent** (a child arrives before its parent): place under `OTHER` until the parent arrives.
2. **Cycles** from concurrent moves: the raw parent graph is functional, so each cycle is simple. The member with the
   greatest placement stamp (the move that closed the cycle) loses its edge and goes to `OTHER`.
3. **Orphans** (a child added to a folder deleted elsewhere): tombstones keep their placement, so walk up to the nearest
   live ancestor. New data is never dropped.
4. **Order**: `(position, id)`. Positions are base-62 fractional indices, and equal positions from concurrent inserts
   tie-break on id.

Repairs are **never written back**. If each device stamped its own repair, the devices would fight forever. Local
deletes tombstone the node and its whole *effective* subtree with one stamp. That gives the exact, testable spec:
**a node is dead iff some device deleted it or a subtree containing it that the device could see.** Local moves are
refused (`WouldCycle`) if the node is an ancestor of the target in the effective tree *or* along the raw placement
chain. The second check means a local move can never lose its own cycle tie-break. Bookmarks live in memory
(`Model { records, tree }`). Every write plans against memory, commits one transaction, then swaps memory and
re-materializes. That costs about 1 ms for 10^4 nodes; `import()` batches.

### Change tracking and the one sync rule (`sync.rs`)

Every synced row has `seq`, a profile-wide counter drawn once per transaction. `changes_since(kind, seq, limit)` is an
index range scan. `apply()` does the following for each record:

```
merged = local ⊔ incoming
if merged != local    -> store it                      (our state changed)
if merged != incoming -> mark dirty with a new seq     (the sender holds less than we do: re-upload)
```

The second line lets the design work with the dumbest server: one that stores the last upload per `(kind, id)` and
never merges. A stale or concurrent upload that overwrote the server copy is repaired by the next device that notices.
When merged == incoming nothing is marked, so records do not echo. The property test uses exactly this server and
bounds settling at 4 rounds. The engine surface is concrete methods, not a trait (one implementation, one consumer):
`changes_since`, `apply` returning `ApplyReport { merged, unchanged, rejected, changed }`, and `engine_state` /
`set_engine_state` in an opaque `sync_state` table. Cursors do not need to be atomic with `apply` because re-applying
is harmless. Incoming JSON is validated at the boundary (`BookmarkWire -> BookmarkRecord`, `ExtensionId`, `RelPath`,
URL parsing). Bad records are reported and skipped, never partly applied. Quotas are never checked on apply, since
doing so would make the outcome depend on local state.

### On-disk format and profile directory

One SQLite file, `rusqlite` with the `bundled` feature, WAL, `synchronous=NORMAL`, every public mutation one
transaction. That gives atomic multi-table writes (an extension commit touches desired and actual; an apply spans
kinds), crash atomicity, indexes for history, and no fsync on the per-navigation path. The trade: power loss can drop
the last few ms of commits; an app crash loses nothing. Typed tables per kind, not one JSON document table:
`<field>_at` stamp columns next to each LWW field, `seq`, and `extra` (full DDL in `src/schema.sql`, with CHECKs for
kind/field agreement, tombstone shape, and who owns `enabled`). Local-only tables (`tab_restore_state`,
`extension_installs`, `ext_storage_local`) have no stamps, so sync cannot see them.

```
%LOCALAPPDATA%\Vsesvit\data\profiles\<name>\   |  ~/.local/share/vsesvit/profiles/<name>/
  LOCK                        std File::try_lock, held for the process lifetime; the OS frees it on crash
  vsesvit.db (-wal, -shm)
  extensions/<id>/<version>_<hash8>/   unpacked, key-injected, IMMUTABLE (WebView2 drops changed extensions)
  staging/<uuid>/             in-flight installs; wiped at open
  engine/                     WebView2 user data folder | WebKit NetworkSession data dir
~/.cache/vsesvit/<name>/      WebKit cache (Linux)
```

At open (idempotent housekeeping): wipe `staging/`, drop install rows whose dir vanished, and delete version dirs that
no row references.

### Extension install pipeline (`extensions/`)

Desired state (synced `extensions` rows, store installs only) is kept apart from actual state (local
`extension_installs`: version, dir, source, parsed `Manifest`, `engine_id`). Every in-between state is an in-memory
value, which gives the state machine *and* idempotency. `InstallSource::parse` handles CWS new and legacy URLs, bare
ids, AMO URLs, `.crx`/`.xpi` paths, and unpacked dirs. `prepare_install` creates an `InstallJob`, which is `Send`, owns
a `StagingDir` guard and has no DB handle. `job.run()` runs off the UI thread and does
fetch → verify → unpack → inject `key` → `Manifest::load`, producing a `StagedInstall` that only `run` can construct.
`commit` runs on the UI thread: an atomic rename into the content-addressed dir (if the dir exists, it is already
complete), then one transaction. The content-addressed dir makes the same bytes idempotent, and different bytes with
the same version never mutate an existing dir.

- CRX3: a hand-rolled protobuf reader. RSA PKCS#1 v1.5/SHA-256 over
  `"CRX3 SignedData\0" ‖ len ‖ signed_header_data ‖ zip`. A developer proof whose key hash equals `crx_id` is required.
  Store downloads also need id == expected and a valid proof by the pinned Web Store publisher key.
- AMO: the API JSON gives the file URL plus a sha256, which is checked. There is no Mozilla signature verification.
- Unpack rejects zip-slip, symlinks, reserved names and archive bombs, and strips `_metadata/`.
- `Intent::User` commits write the synced desired state (`Lww::set`, so a re-run mints nothing).
  `Intent::Reconcile` commits never do, and are discarded if the extension was uninstalled elsewhere meanwhile.
- `reconcile()` = desired ∖ actual → jobs; actual-from-store ∖ desired → local removal.

### Threading

`Profile` is `!Send` and lives on the UI thread. Everything slow is a `Send` value with no DB handle (an `InstallJob`,
or a future engine's `Vec<WireRecord>`), and its result is committed back on the UI thread. Nothing is shared, so there
are no locks, per separate-before-serializing-shared-state.

### Access patterns traced

| # | Call | Cost |
|---|---|---|
| 1 | `record_visit` + `is_bookmarked` | 1 WAL transaction (upsert page, insert visit, refresh stats) + 1 in-memory hash lookup |
| 2 | `omnibox().resolve` / `suggest` | pure `classify` over engines / in-memory bookmark scan + 1 query on `url_key` / `frecency` indexes with LIMIT |
| 3 | bookmark CRUD, `children` | validate in memory, 1 transaction, re-materialize in memory / clone of one `Vec` |
| 4 | install / `list` / enable | worker job + 1 rename + 1 transaction / 1 query, no disk scan |
| 5 | `prefs().get/set` | primary-key lookup / 1 transaction |
| 6 | `session().save/restore` | 1 transaction (no-op if unchanged) / 1 read |
| 7 | `changes_since` / `apply` | `seq` index range / 1 transaction per batch |

### The property test (`tests/convergence.rs`)

`devices_converge` runs three real `Profile`s on real SQLite files. Their clocks start up to an hour apart and advance
independently. Each case runs 1–120 random steps: bookmark add/rename/move/reorder/remove-folder, visits, URL
forgets, prefs, `storage.sync` set/remove, and reading list edits. The devices sync through the dumb server with
truncated, shuffled, duplicated and batch-split downloads, and then settle. The test asserts:

1. **Quiescence**: settling finishes within 4 rounds, so nothing echoes.
2. **Convergence**: byte-identical exports for every kind.
3. **Same tree**: every device shows an identical materialized tree.
4. **Valid tree**: no cycles; every live record is reachable exactly once; parents are folders; indexes are dense.
5. **Deletion spec, exactly**: `live == created − deleted_as_seen_by_the_deleting_device`.
6. **Idempotence**: re-applying a shuffled, duplicated full export merges nothing and marks nothing dirty.

`lattice_laws` checks idempotence, commutativity and associativity per record type, using tiny stamp ranges so ties
are common. `materialize_always_valid` feeds arbitrary record sets (cycles, dangling parents, separators as parents)
and requires a valid tree covering exactly the live records.

### Deliberately not done

- No async runtime.
- No observer or event bus: mutations return their changes, and `ApplyReport.changed` covers sync.
- No sync-engine trait.
- No operation log.
- No at-rest encryption yet.
- No protobuf crate.
- No XPI signature verification.
- No automatic extension updates. The pipeline already supports them: an update is a newer-version install.
- No DNR-to-WebKit translation. That is the Linux runtime's job, and it reads `Manifest.dnr_rulesets`.

## Synthesis decision

**Base: candidate 1 (Opus).** The cross-judge (Fable) scored the candidates 28, 24 and 13 and recommended candidate 1. My own read agrees. It is the only design where every global invariant is a pure function of lattice state. It states the deletion rule causally and tests it exactly. Its "merged != incoming, so re-upload" rule works against a server that never merges. It also covers the WebView2 specifics: `engine_id`, add-before-remove, and immutable content-addressed version dirs.

**Grafted.**

- From candidate 2: each install records its provenance as a `Verification` enum (`ChromeWebStore { publisher_verified }`, `AmoHash`, `LocalCrx`, `LocalXpi`, `Unpacked`). The CRX3 parser rejects zip end-of-central-directory magic inside the header and fails if any proof is invalid, as Chromium does. `InstallJob::run` takes `progress: &mut dyn FnMut(InstallPhase)` for the download UI.
- From candidate 2: the reading list is dropped as a kind. Code 9 is retired. If it comes back, it is a new table and kind and touches no existing row.
- From candidate 3: search-engine records use `Record<T> = Live(T) | Tombstone(Stamp)` instead of live fields next to `deleted: Option<Stamp>`.
- From candidate 3: the reserved credential wire shape is named now: `CredentialBlob { ciphertext, wrapped_by }` for kinds 10 and 11.

**Fixed (defects the judge found in candidate 1).**

- History retention is no longer part of `normalize` or the join. A per-device cutoff made devices with different cutoffs echo forever. Version 1 keeps history until the user clears it, and clearing writes synced deletion directives. `History::expire` is removed.
- A history page record keeps only its newest 64 visits. Union followed by "keep the 64 largest `(at_ms, device)`" is still a semilattice join, because the top 64 of a union depends only on the global top 64. This bounds the record size.
- `move_to`'s raw-chain cycle check carries a visited set, because the raw placement graph can contain cycles that do not involve the node being moved.
- Session record equality ignores local restore state. Restore blobs live only in the local `tab_restore_state` table.
- CRX3 verification also checks ECDSA P-256 proofs (the `p256` crate). It requires a developer proof whose key derives to `crx_id`. Downloads from the Chrome Web Store also need a proof by the Web Store publisher key, whose SPKI SHA-256 is `61f7f2a6bfcf74cd0bc1fe2497cc9b04254c658f79f2145392867ea8366367cf`.
- `BookmarkId::is_root` must not treat the nil UUID as a root.
- The wire rule is tested with serde. Every synced field is `{"v": .., "at": ".."}`, and a field that needs a different merge rule needs a new `Kind`.

**Rejected.**

- Candidate 2's "delete is a move to TRASH, hoist by stamp". The hoist compares HLCs, which is not causality, so a child added concurrently with an earlier HLC gets trashed. Candidate 1's rule (a node is dead iff some device deleted it, or a subtree containing it, while it could see the node) is exact and already keeps concurrent additions. A "recently deleted" view can be local.
- Candidate 2's incremental `bookmark_tree` cache. It goes wrong once a skipped placement becomes valid again. Candidate 1 re-materializes after every write.
- `sha256(url)` in deletion directives. A future engine encrypts records end to end, and hashing a known URL adds no privacy.
- Candidate 3's apply-time cycle and orphan guards. They depend on arrival order and diverge (see the judge's worked example).

## Tradeoffs accepted

- We accept that concurrent edits to the *same* field lose one side (LWW) in exchange for field-level merge everywhere
  else and a one-line merge rule.
- We accept that a cycle loser lands in "Other bookmarks" instead of its previous folder. That position is not
  recoverable from state alone. In exchange we get a state-based design with no operation log.
- We accept terminal bookmark tombstones. A child that device A deleted as part of a folder stays deleted even if
  device B moved it out concurrently. In exchange the deletion rule is exact and testable, and concurrently *added*
  children are never lost.
- We accept tombstones kept forever (tiny; GC needs engine knowledge of all devices) in exchange for not building
  causal-stability tracking now.
- We accept an O(n) re-materialization on every bookmark write (about 1 ms at 10^4 nodes) in exchange for one
  derivation path, with no incremental tree maintenance to get wrong.
- We accept re-uploads of merged records (extra traffic) in exchange for correctness against a server that never merges.
- We accept `synchronous=NORMAL` (power loss can drop the last commits) in exchange for no fsync on each navigation.
- We accept stored derived columns in two places (history frecency and stats, the Manifest snapshot in
  `extension_installs`) in exchange for indexable ranking and disk-free `list()`. Each has exactly one writer.
- We accept that `storage.sync` does not sync on Windows (WebView2 owns `chrome.storage`) in exchange for letting
  WebView2 own the full extension runtime.

## Alternatives considered

- **Operation log plus a replicated-tree move CRDT (Kleppmann et al.)** gives better move semantics (the loser keeps its
  prior parent). It needs a durable log, undo/redo on out-of-order arrival, and log truncation by causal stability,
  which in turn needs to know every device, meaning engine knowledge inside core now. It lost on size and coupling.
- **Chrome's shape** (a JSON `Bookmarks` file and `Preferences`, with sync metadata added later) fails "no migration
  later". Without per-field stamps, offline edits need a server-mirror three-way merge (Firefox's Dogear), which is a
  redesign.
- **One generic document table** `records(kind, id, json, seq)` gives uniform sync plumbing, but every hot query (star,
  children, omnibox prefix, frecency) needs JSON-extracted indexes. Typed tables plus an internal `SyncTable` trait keep
  that uniformity only where it pays: `apply_one` and `changes_since` are each written once.
- **Automerge or Yrs documents** are not in the available crate set, are opaque blobs, and have no cycle-safe tree
  move. We would still need our own materializer.

## Open questions and risks

- Is "Other bookmarks" the right landing place for cycle losers and parentless arrivals, or should they go to the
  root of the loser's former top-level folder?
- Is "delete wins for everything the deleter could see" the product rule we want, or should a concurrent move revive
  a node (LWW liveness instead of terminal tombstones)?
- `storage.sync` on Windows: accept that it syncs only between Linux devices, or build a shim that routes
  WebView2 extensions' `chrome.storage.sync` through core?
- When WebView2 re-adds a new dir with an existing id, does the extension keep its `chrome.storage` data? This needs a
  spike before updates ship, because the reconcile code assumes add-replaces without data loss.
- Should a Linux device auto-install synced Chrome extensions that the WebKit runtime supports only partly? This may
  need a compatibility gate in `reconcile()`.
- Pinning the Web Store publisher key means installs fail closed if Google ever serves ECDSA-only publisher proofs
  (`rsa` is the only verifier available). Is that acceptable?
- Should `Clock::observe` clamp remote stamps more than a day ahead? A peer with a broken clock otherwise pushes our
  HLC forward permanently. That is harmless for ordering but ugly.
- A copied profile directory duplicates the device id. Merge stays convergent thanks to the value tiebreak, but the
  two devices share one Sessions record. Should `meta` hold a machine fingerprint and re-mint on mismatch?

## Next implementation step

Implement `crdt.rs`, `db::Tx`, and bookmarks (record join, `Position`, `materialize`, `Model`, the `bookmarks` table)
plus `SyncTable` for `Kind::Bookmarks` only. Get `materialize_always_valid`, `lattice_laws` and `devices_converge`
green with bookmark ops only. Every other kind then follows the same template.
