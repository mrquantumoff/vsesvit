# vsesvit-core: design rationale (candidate 3)

## Problem

`vsesvit-core` is the platform-agnostic library both shells (WinUI/WebView2 on
Windows, GTK4+WebKitGTK on Linux) call for everything that isn't rendering a page
or drawing a widget: bookmarks, history, sessions, extensions, preferences, search
engines, and the extension install pipeline. The shape is non-obvious for one
reason above the others: every data kind must be **sync-ready today** even though
sync itself doesn't exist yet — there is no server, no protocol, no second device
to test against, and no license to redesign the storage format when sync finally
lands. That constraint, stated directly in the grounding, rules out the natural
shortcut of "store whatever's convenient locally, retrofit sync fields later." It
also means the *hard* part of this design — how a bookmark tree edited
concurrently on two devices converges to a valid tree with no cycles and no
orphans — has to be solved and property-tested now, with no real sync engine to
exercise it, which is why a big share of this document is about a merge algorithm
for a plugin that doesn't exist yet.

Two more constraints from the grounding shape everything below: single process per
profile (enforced with a lock, not a convention), and data must survive a crash
mid-write — which pushes toward a storage engine with real transactions rather than
files-plus-hope, and toward an install pipeline where "crashed halfway through"
and "not started" are indistinguishable from the outside.

## Usage (caller's view)

Both shells hold one `vsesvit_core::Profile`, opened once at startup, and call
into it directly from their UI thread. Every read here is a local SQLite lookup,
fast enough for the UI thread by construction (see [Shape](#shape)); the one
operation that touches the network — installing an extension — returns a plain
`Future` that the *shell* schedules on its own event loop. Core never assumes
WinUI's dispatcher queue or GTK's main context.

Windows shell (WinUI 3), star button + omnibox commit + extension install:

```rust
// crates/vsesvit-winui/src/browser_page.rs
use vsesvit_core::{ExtensionSource, Transition};

impl BrowserPage {
    fn on_navigation_committed(&self, url: url::Url) {
        // Local SQLite reads/writes; safe to call straight from the UI thread.
        self.profile.history().record_visit(&url, Transition::Typed).ok();
        let starred = self.profile.bookmarks().is_bookmarked(&url).unwrap_or(false);
        self.star_button.set_checked(starred);
    }

    fn on_star_clicked(&self) {
        let url = self.current_url();
        let bookmarks = self.profile.bookmarks();
        if bookmarks.is_bookmarked(&url).unwrap_or(false) {
            bookmarks.remove_by_url(&url).ok();
        } else {
            bookmarks.add_link(self.bookmarks_bar_id, &url, &self.current_title()).ok();
        }
    }

    fn on_install_extension_clicked(&self, store_url: &str) {
        let source = ExtensionSource::parse(store_url).expect("bad store url");
        let profile = self.profile.clone();
        let dispatcher = self.dispatcher_queue.clone();
        // core hands back a Future; THIS shell drives it on ITS dispatcher queue.
        let weak = self.weak_self();
        dispatcher.spawn(async move {
            let result = profile.extensions().install(source, |p| {
                // marshal progress back to the UI thread; core doesn't know how.
            }).await;
            if let Some(this) = weak.upgrade() {
                this.on_install_finished(result);
            }
        });
    }
}
```

Linux shell (GTK4), the same three actions:

```rust
// crates/vsesvit-gtk/src/window.rs
use glib::clone;
use vsesvit_core::{ExtensionSource, Transition};

impl imp::BrowserWindow {
    fn connect_signals(&self, profile: vsesvit_core::Profile) {
        self.web_view.connect_load_changed(clone!(@strong profile, @weak self as win => move |view, event| {
            if event == webkit6::LoadEvent::Committed {
                let url: url::Url = view.uri().unwrap().parse().unwrap();
                profile.history().record_visit(&url, Transition::Link).ok();
                win.star_button.set_active(profile.bookmarks().is_bookmarked(&url).unwrap_or(false));
            }
        }));

        self.star_button.connect_clicked(clone!(@strong profile, @weak self as win => move |_| {
            let url: url::Url = win.web_view.uri().unwrap().parse().unwrap();
            let bookmarks = profile.bookmarks();
            if bookmarks.is_bookmarked(&url).unwrap_or(false) {
                bookmarks.remove_by_url(&url).ok();
            } else {
                bookmarks.add_link(win.bookmarks_bar_id, &url, &win.web_view.title().unwrap_or_default()).ok();
            }
        }));

        self.install_button.connect_clicked(clone!(@strong profile, @weak self as win => move |_| {
            let source = ExtensionSource::parse(&win.url_entry.text()).unwrap();
            // core's Future driven on GTK's OWN main context; core never links glib.
            glib::spawn_future_local(clone!(@strong profile, @weak win => async move {
                match profile.extensions().install(source, |_p| {}).await {
                    Ok(ext) => win.refresh_extensions_list(),
                    Err(e) => win.show_toast(&e.to_string()),
                }
            }));
        }));
    }
}
```

The full public surface these calls hit is in [`lib.rs`](lib.rs). The types it
returns and takes are in [`model.rs`](model.rs); how they're merged, in
[`merge.rs`](merge.rs); the on-disk format, in [`store.rs`](store.rs); the install
pipeline, in [`extension.rs`](extension.rs); the sync plug-in point, in
[`sync.rs`](sync.rs); the convergence property test, in
[`tests/convergence.rs`](tests/convergence.rs).

## Shape

**Identity, clock, and tombstones (`model.rs`).** Every syncable row carries a
`SyncId` (uuid v4, minted once, never reused) and is wrapped in
`Record<T> = Live { hlc, payload } | Tombstone { hlc }` — deletion never removes a
row, it replaces the payload with a marker stamped at the same logical clock as
everything else, so "deleted after what you have" is distinguishable from "never
existed" (needed for deletion propagation, called out explicitly in the
grounding). The clock is a hybrid logical clock, `Hlc { millis, counter, device }`
— total order even when two machines' wall clocks disagree or tie, and it is the
**only** ordering used to resolve every conflict in the system (single source of
truth per invariant): one `Hlc` type, one comparison, reused by every merge
function below instead of each kind inventing its own tie-break.

**Change tracking (`model.rs` + `store.rs`).** A single `change_log` table
(kind, id, hlc) is appended to in the *same transaction* as every mutation, local
or remote. This is what a future sync engine's "enumerate changes since cursor"
reads (grounding requirement 7), and it's why there's no separate dirty-bit or
per-table sync flag to keep in sync with itself.

**Per-kind merge semantics (`merge.rs`).** Every data kind in the grounding maps
onto one of four shapes, each with exactly one merge implementation reused across
every kind of that shape:

1. **Tree** (bookmarks only) — a node's `Placement { parent, order }` and its
   `payload` (title/url) are stamped with *independent* HLCs, because they change
   from genuinely different UI actions (a move vs. a rename) that can race each
   other; folding them into one clock would let an unrelated rename silently undo
   a concurrent move. Sibling order is a fractional `OrderKey` (string that always
   admits a value between any two others), so inserting or reordering one sibling
   never rewrites any other sibling's row — no renumbering, no cross-row
   contention. See "bookmark-tree convergence strategy" below.
2. **Append-only facts + suppression tombstone** (history) — visits are immutable;
   applying a remote batch is `INSERT OR IGNORE` keyed by the visit's own id, a set
   union. The only thing that competes is a "forget this URL" tombstone, which is
   keyed on the URL, not the visit, so it suppresses every visit to that URL
   including ones synced in later from a device that hasn't heard about the
   deletion yet.
3. **Per-key LWW KV** (preferences, `chrome.storage.sync`) — one table keyed by
   `(namespace, owner, key)` where `namespace` distinguishes "the browser's own
   prefs" from "extension X's synced storage." One merge function serves both,
   because both are, semantically, the same eight lines of last-write-wins code;
   giving them separate tables would just be that code copy-pasted twice.
   Search-engine defaults live here too (`Preferences["default_search_engine"]`),
   which is why "two devices set different default search engines" needs no
   special-case merge: it's an ordinary LWW key.
4. **Simple LWW row** (installed extensions, search engine definitions) — a whole
   small record clocked as one or more independently-timestamped fields.
   Installed extensions specifically split `version` and `enabled` into separate
   HLCs, for the same reason bookmarks split placement from payload: toggling a
   disable switch and running an update are different actions that can race.
   Critically, **the synced payload for an extension is metadata only** — id,
   source, version, enabled. The unpacked files on disk are never synced; a
   remote "extension X version Y is installed" change is applied by *re-running
   the local install pipeline* for X@Y, not by shipping bytes.

Sessions/open tabs are deliberately **not** one of the four merge shapes: only the
owning device ever writes its own `SessionSnapshot` row (keyed by `device_id`), so
there is no concurrent-writer case, and folding it into a generic merged
"document" kind would just be pretending a problem exists that doesn't
(per separate-before-serializing-shared-state). Reading "tabs from other devices"
is a read-only scan of other devices' rows.

Passwords/autofill are a named slot, not a placeholder: `EntityKind::Credential`
and `CredentialBlob { id, ciphertext, wrapped_by, hlc }` exist in `model.rs` today,
unused. Core stores and moves opaque bytes and an `Hlc`; it never becomes a
keychain, because wrapping/unwrapping is inherently platform-specific (DPAPI vs.
libsecret) and belongs on the shell side of the boundary, not in core.

**Bookmark-tree convergence strategy.** `merge.rs::apply_bookmark_change` accepts
one incoming placement/payload change and is safe to call in any order and more
than once:

- *Field LWW*: a placement only overwrites the stored one if
  `incoming.placement_hlc > stored.placement_hlc` (device id breaks ties);
  payload independently.
- *Cycle guard*: before committing a placement whose parent differs from the
  stored one, walk the new parent's ancestor chain. If it reaches the node being
  moved, the move is redirected to `RECOVERY_ROOT` instead — deterministically, so
  every device applying the same change_log entry makes the same call, not a
  per-device coin flip.
- *Orphan guard*: if the target parent is a tombstone, the node goes to
  `RECOVERY_ROOT` instead of under a dead folder. This is what makes "folder
  deleted on device A, child added under it on device B" converge: whichever HLC
  is later wins, and if the delete wins, the child surfaces at the top level
  instead of disappearing.

`RECOVERY_ROOT` is a well-known, never-tombstoned bookmark id (a sibling of
"Bookmarks bar" / "Other bookmarks") so a user can always find what the merge
algorithm rescued instead of it vanishing silently.

**On-disk format (`store.rs`).** SQLite via `rusqlite` 0.40 (`bundled`), WAL mode,
`synchronous = FULL`. Reasons, weighed against the alternatives:

- The dominant access patterns are relational (children of a folder in order,
  visits joined to URLs, "does this URL have a bookmark") — an index-backed SQL
  engine answers them directly; a flat log or a plain KV store (`sled`/`redb`)
  would require hand-rolling exactly the indices SQLite already has.
  `synchronous = FULL` over the perf-friendlier `NORMAL` is a deliberate choice:
  the grounding's "must survive a crash mid-write" is a hard constraint, not a
  nice-to-have, and FULL is what guarantees a committed transaction is actually on
  disk before the write call returns, at the cost of a bit of write latency —
  install and bookmark writes are not hot-loop operations.
- Transactions give atomic multi-row writes for free (an `OrderKey` rebalance
  touching several siblings, or "insert the extension row and the change_log
  entry together" — never observed half-done).
- One file, identical format on both target OSes, which matters when the same
  crate ships to both shells from one codebase.
- Two mature browsers (Chromium, Firefox) already use exactly this engine for
  exactly this data, which is decent evidence it fits the shape of the problem
  rather than fighting it.

Full DDL is in `store.rs`. Profile directory layout:

```
<profile_root>/
  profile.lock            # PID + start-time; single-process-per-profile enforcement
  vsesvit.db              # bookmarks, history, kv, search engines, extension registry, change_log, session snapshots
  vsesvit.db-wal/-shm     # SQLite WAL sidecars
  extensions/
    <extension_id>/
      <version>/           # unpacked files; re-install of the same version is a fast no-op (hash check)
        manifest.json
      CURRENT               # plain text file naming the active version dir (no symlink privilege needed on Windows)
      .tmp-<uuid>/          # install scratch; swept on next Profile::open if abandoned by a crash
  cache/
    crx-downloads/          # transient .crx/.xpi bytes, freely cleared
```

`profile.lock` (not SQLite's own file lock) is the single-process enforcement
mechanism: created with `create_new` at open, holding the PID and process start
time; a stale lock from a crash is detected by checking whether that PID is still
alive *and* still the same process (start time matches), so re-opening after a
crash needs no manual cleanup, per make-operations-idempotent.

**Extension install pipeline (`extension.rs`).** `ExtensionSource::parse` accepts
a Chrome Web Store URL, a bare 32-character a–p store id, an AMO URL, a local
`.xpi` path, or an unpacked directory, and picks the right code path. CRX3 is
parsed by hand (magic `Cr24`, version, header length, then the two
length-delimited protobuf fields we actually need —public key and signature—
read directly, since no protobuf crate is in the approved dependency list and the
format is simple enough not to need one) and **verified**: signature over the zip
via `rsa` + `sha2`, and the id re-derived from the public key and checked against
the id the store URL asked for. A CRX that fails verification is never unpacked —
this is real security enforcement, not a formality, per boundary-discipline. XPI
(AMO fallback) and unpacked dev extensions are trusted at the manifest level only;
see "Tradeoffs accepted." The manifest model keeps a raw `serde_json::Map`
alongside typed fields we interpret today, so an unrecognized manifest key never
breaks parsing (forward compatible, per encode-lessons-in-structure applied to
"don't let an external format's evolution break us").

Idempotency and crash-safety come from *where* durable state changes, not from a
persisted state machine: download/verify/unpack all happen in a `.tmp-<uuid>/`
scratch directory, and the **only** durable step is one `fs::rename` into
`extensions/<id>/<version>/` plus one SQLite transaction that upserts the
`installed_extension` row and appends the change_log entry. If the process dies
before that rename+transaction, nothing durable moved — a retry starts clean, and
an abandoned `.tmp-*` directory is swept the next time `Profile::open` runs.
Re-running an install already present at that exact version short-circuits after
the verify step (hash match on the existing `manifest.json`) into a no-op that
still reconciles the DB row, in case disk and DB ever drifted apart independently.

**Sync plug-in point (`sync.rs`).** A `ChangeSource` trait —
`changes_since(cursor) -> (ChangeBatch, Cursor)` and
`apply_remote(batch) -> ApplyReport` — is the entire surface a future
`vsesvit-sync` crate needs. `apply_remote` is implemented by calling the exact
same `merge.rs` functions a local write goes through, so "two local writes race"
and "a remote batch lands mid-session" are one code path, not two that can drift
out of sync with each other. `Profile::change_source()` is the one method that
exists today; nothing else in core needs to change when sync is built.

## Synthesis decision

*Filled in by arena after comparing candidates.*

## Tradeoffs accepted

- We accept hand-rolling the two CRX3 protobuf fields we need instead of pulling
  in a protobuf crate, in exchange for staying inside the approved dependency
  list and not carrying a general protobuf runtime for two fixed-shape fields.
- We accept **not** verifying Mozilla's XPI signature (the META-INF layer) for
  the AMO fallback path, in exchange for not building a second, less-used
  verification path for a source the grounding itself calls a fallback; manifest
  parsing still applies, and this is a strictly worse-trust path than the
  primary CRX3 flow, which is exactly what "fallback" should mean.
- We accept `synchronous = FULL`'s extra write latency in exchange for the
  crash-survival guarantee the grounding requires as a hard constraint, rather
  than the faster `NORMAL` most read-heavy apps default to.
- We accept that a concurrent move that would form a cycle is *silently*
  redirected to `RECOVERY_ROOT` rather than surfaced as a conflict the user
  resolves by hand, in exchange for a merge that always terminates
  deterministically without a UI for conflict resolution that doesn't exist yet.
- We accept extra table width (three columns per HLC field, sometimes two HLCs
  per row) in the SQLite schema, in exchange for never inventing a second
  encoding for "when did this change" — the same three-column shape everywhere
  means one set of read/write helpers, not N.

## Alternatives considered

- **Per-kind bespoke sync tables with custom conflict logic** (e.g. what a
  "just add a `synced_at` timestamp per existing table" approach devolves into) —
  rejected because it produces N slightly-different merge implementations that
  drift apart under maintenance, instead of four shapes with one implementation
  each; it also tends to smuggle wall-clock timestamps in as the ordering
  mechanism, which breaks the moment two devices' clocks disagree.
- **Integer position for bookmark siblings, renumbered on every insert** —
  rejected: renumbering touches every sibling row on every insert, which turns
  "add one bookmark" into a write that contends with every concurrent edit to
  that folder, and produces spurious merge conflicts for siblings nobody
  touched. Fractional `OrderKey` makes an insert a single-row write.
- **A KV engine (`sled`/`redb`) instead of SQLite** — the only real contender.
  Loses on this workload specifically because "children of a folder in order" and
  "visits joined to URL, filtered by time" are exactly the queries a relational
  index answers for free; a KV engine would need us to hand-build and
  hand-maintain those indices ourselves, which is the SQL engine's job.

## Open questions and risks

- Is `RECOVERY_ROOT`'s silent-redirect behavior the right UX, or should a
  rescued node also get a synced "flag for review" marker so the user's *other*
  device shows a badge, not just the device where the conflict was resolved?
- The grounding says WebView2's extension runtime is opaque (it owns background,
  content scripts, `chrome.*` once installed) — does `Extensions::storage_get/set`
  need a WebView2-side counterpart at all, or does WebView2 keep its own
  `chrome.storage.sync` state that core would need to read out of it rather than
  own directly? This design assumes core owns the KV rows on both platforms and
  WebKitGTK's hand-built runtime reads/writes through core; if WebView2 doesn't
  expose a hook to intercept `chrome.storage.sync` calls, that assumption needs
  revisiting before Windows extension storage sync is real.
- CRX3's `CrxFileHeader` can carry more than one `sha256_with_rsa` proof (Chrome
  accepts multiple signers during key rotation); the sketch verifies the first
  one found. Worth deciding whether store-installed extensions ever need
  multi-signer verification before this ships.

## Next implementation step

Write `store.rs`'s schema as a real migration-0 and get `Profile::create` /
`Profile::open` round-tripping on an in-memory connection under `cargo test`,
so every other module has a real `Connection` to write against instead of a
sketch.
