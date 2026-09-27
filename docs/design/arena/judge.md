# Arena cross-judge: vsesvit-core candidates

## Scores (1-5)

| Criterion | C1 | C2 | C3 |
|---|---|---|---|
| 1 Sync readiness | 4 | 4 | 2 |
| 2 Type discipline | 5 | 4 | 2 |
| 3 Shell ergonomics | 5 | 4 | 2 |
| 4 Provability | 5 | 4 | 2 |
| 5 Simplicity | 4 | 4 | 3 |
| 6 Robustness | 5 | 4 | 2 |
| **Total** | **28** | **24** | **13** |

### Evidence

**Candidate 1** (`candidate-1/vsesvit-core/`)
1. Sync 4: every kind has id + `seq` + per-field `*_at` stamps + tombstone (`schema.sql` header conventions; DESIGN "Per-kind model"); the tree is a pure `materialize` over a lattice (`bookmarks.rs` rules 1-4) and `apply_one`'s "merged != incoming -> dirty" repairs a non-merging server (`sync.rs` module doc). Loses a point because `history::normalize` applies per-device retention inside the join, which makes HistoryPages non-convergent and echo between devices with different cutoffs (defects below).
2. Types 5: `NodeState` makes kind/field agreement unrepresentable, `BookmarkWire -> BookmarkRecord` `TryFrom` is the single validation boundary, `StagedInstall` has private fields only `InstallJob::run` can fill, `RelPath`/`MatchPattern`/`ExtensionId`/`Position`/`Stamp` all parse once (`bookmarks.rs`, `extensions/install.rs`, `extensions/manifest.rs`, `crdt.rs`).
3. Shell 5: `Profile` is `!Send` on the UI thread, slow work is a `Send` `InstallJob` with no DB handle committed back on the UI thread, both shells shown end to end including WebView2 `engine_id` reconciliation and add-before-remove (DESIGN "Usage"; `lib.rs` Profile doc).
4. Provability 5: `materialize`, `Lattice::join`, `crx::parse/verify`, `classify`, `normalize` are pure; `tests/convergence.rs` runs three real profiles with skewed clocks through a last-upload-wins server with shuffle/dup/split and asserts quiescence, byte-equal exports, identical valid trees, an exact causal deletion spec, and no-op re-apply; plus `lattice_laws` and `materialize_always_valid` on arbitrary record sets.
5. Simplicity 4: no async runtime, observer, op log, or engine trait (DESIGN "Deliberately not done"), but nine kinds including a speculative ReadingList, `Extra` unknown-field pass-through, and an internal `SyncTable` trait.
6. Robustness 5: `File::try_lock`, WAL with stamp/seq persisted in the same tx, `user_version` migrations + `TooNew`, staging wiped at open, content-addressed immutable `<version>_<hash8>` dirs, idempotent `commit`, GC of unreferenced dirs (`lib.rs` open steps; `extensions/mod.rs` commit; `db.rs`).

**Candidate 2** (`candidate-2/src/`)
1. Sync 4: per-field `Lww` rows with `seq` and `Hlc`; pure `repair()` (Kleppmann rule over final states + hoist) yields a convergent tree (`bookmarks.rs` repair doc). Loses a point because `MergeOutcome::from_registers` maps all-`KeptLocal` to `Unchanged` (no seq bump), so a stale overwrite on the "dumb key/value store" the design promises is never repaired, and `Lww::merge` treats equal stamps as the same write with no value tiebreak (`clock.rs`, `sync.rs`).
2. Types 4: `ChromeExtId([u8;16])`, `ExtensionId`, `EngineId`, closed `Record` enum, `Verification`, static `Pref<T>` registry, typed `AmoAddon` at the boundary (`source.rs`, `prefs.rs`, `sync.rs`); but `BookmarkRecord.url: Lww<Option<Url>>` admits folder-with-URL ("tolerated on merge"), `Staged`/`VerifiedPackage`/`Package` are all-`pub` so `commit` cannot tell verified content apart, and `Manifest` paths/patterns are `String` with an untagged `Background` (`bookmarks.rs`, `installer.rs`, `manifest.rs`).
3. Shell 4: `Profile: Clone+Send+Sync`, `Installer` is `Send+Sync` with no DB, both shells shown (DESIGN "Usage"); but the shell owns the pending-install loop (`Extensions::pending`) and the Windows sample re-adds every extension to WebView2 on each launch with no engine-id bookkeeping.
4. Provability 4: `repair` is pure (but `pub(crate)`), `Merge` per record, `crx3::verify` pure over bytes; convergence test has 2-4 profiles, mangled pairwise deliveries, settle, snapshot equality, invariants, idempotent re-apply and pinned deletion scenarios (`tests/convergence.rs`), but no injectable clock (skew untested), only bookmarks in the multi-device property, and `apply_local`/`mangle`/`any_record` are `todo!()`.
5. Simplicity 4: reading list folded into a root folder, whole-record LWW for sessions/engines, no store trait (DESIGN d, "Alternatives"); but two derived caches (`bookmark_tree`, `urls`), `RemoteCrx`/`RemoteXpi` beyond the grounding, and a wider port (`head/snapshot/purge_tombstones/now`).
6. Robustness 4: `File::try_lock`, WAL, `user_version` + `SchemaTooNew`, staging + rename, idempotent `commit`, `reconcile()` adopts/cleans dirs (`lib.rs` open; `extensions/mod.rs`); but version dirs are not content-addressed, `commit` step 3 deletes old version dirs WebView2 may still hold open, and store installs skip the publisher proof.

**Candidate 3** (`candidate-3/`)
1. Sync 2: ids, HLC, `Record<T>` tombstones and `change_log` exist (`model.rs`), but `apply_bookmark_change` evaluates the cycle and orphan guards against the local tree at arrival time and writes the redirected parent back (`merge.rs` steps 3-4; DESIGN "Bookmark-tree convergence strategy"), which is order-dependent and non-convergent (worked example below); no per-row dirty tracking, so no re-upload path exists.
2. Types 2: `Preferences::get(&str) -> Option<Value>` is stringly; `Background` allows service_worker+page+scripts at once; two different `ExtensionSource` enums (`model.rs` vs `extension.rs`); `ExtensionId::generate_for_unpacked` violates the type's documented invariant; synced `HistoryVisit` carries a local rowid `UrlId(i64)`; `kv_entry` rows can hold both a live and a tombstone HLC (`store.rs` SCHEMA).
3. Shell 2: the API reads small, but `Extensions::install` returns a `Future` the shell polls on its main loop while the pipeline uses blocking `ureq` with no executor or thread in core (`lib.rs`; DESIGN "Usage"): network on the UI thread unless something unstated spawns a thread. `storage.local`, get-all/remove/clear/onChanged, and pending-install reconcile are missing.
4. Provability 2: every merge fn takes `&rusqlite::Transaction` (`merge.rs`), so nothing is pure; the test is two devices, one full exchange each, no partial syncs, skew, third device or quiescence check (`tests/convergence.rs`); `Crx3::parse` is pure but wrong (below).
5. Simplicity 3: five files and one KV table for prefs + `storage.sync` are real reductions, but the PID+start-time lock, `CURRENT` file, `cache/crx-downloads` and duplicated types add back, and much of the smallness is missing function.
6. Robustness 2: WAL+FULL (an fsync per navigation on the UI thread), no `user_version`/migration mechanism, `CURRENT` is a third durable step outside the rename+tx, TOCTOU-prone PID lock, extension fns take only `&ProfileDir` (implying a second connection and no shared clock), CRX verification does not match the CRX3 signed message (`store.rs`, `extension.rs`).

## Recommendation: base on Candidate 1

It is the only package where every global invariant is a pure function of a provable lattice (`materialize`), the deletion rule is stated causally and tested exactly, the sync rule repairs a non-merging server, and the boundary types (`BookmarkWire`, `StagedInstall`, `RelPath`) make the security- and sync-relevant illegal states unconstructible. Its threading needs no locks, it handles WebView2 specifics (`engine_id`, add-before-remove, immutable dirs) the others miss, and it is the only sketch that claims to type-check with its property test. Its one real convergence bug (retention inside `normalize`) is a one-line removal.

## Grafts

From Candidate 2:
1. Delete-as-move-to-TRASH with a well-known TRASH root plus hoist-by-stamp (`bookmarks.rs` repair steps 3-4): gives a recoverable "recently deleted" list and lets a concurrent move-out survive a folder delete, which C1's terminal tombstone loses. Keep C1's pure `materialize`; add TRASH as a root and hoist live descendants whose placement stamp exceeds the trash stamp. Document that HLC-after is not causally-after.
2. Reading list as a root folder (`BookmarkId::READING_LIST`) instead of C1's ninth kind; `sha256(url)` in history deletion directives instead of plaintext URLs on the wire.
3. Install provenance and Chromium parity: record a `Verification` enum per install, reject zip EOCD magics inside the CRX header and fail on any invalid RSA proof (`crx3.rs`), and add `progress: &mut dyn FnMut(Phase)` to `InstallJob::run` for the download dialog.

From Candidate 3:
1. `Record<T> = Live | Tombstone` as the shape for C1's `EngineRecord`, which currently keeps `deleted: Option<Stamp>` beside live fields (an illegal state C1 otherwise avoids).
2. Name the reserved credential wire shape now (`CredentialBlob { ciphertext, wrapped_by }`) instead of a schema comment, so kinds 10/11 have an agreed body before any engine exists.

## Defects and risks

**Candidate 1**
- History retention breaks convergence: `history::normalize` prunes by a per-device `cutoff_ms` inside the join, and `HISTORY_RETENTION_DAYS` is a `Scope::Local` pref. Device A (pruned) and B (not pruned) each compute merged != incoming on every exchange and re-mark the page dirty: an infinite echo through the dumb server. `devices_converge` cannot see it because `expire()` is never an op. Fix: retention is a local read filter, never part of `join`/`normalize`.
- `PageRecord` body is the whole visit set keyed by URL: every visit re-uploads all visits of that page, and the wire id is an unbounded URL (ExtStorage hashed its key; history did not).
- Terminal bookmark tombstones silently drop a concurrent move-out (accepted, but it is loss of a user action); consider the TRASH graft.
- Cycle rule 2: after the loser is parked under OTHER, any later move of the other cycle member dissolves the cycle and the loser silently jumps back under its raw parent. Convergent but surprising. `move_to`'s raw-chain check must be bounded by a visited set (the raw graph can contain cycles not involving `id`) and over-refuses legitimate moves.
- `BookmarkWire` flattens unknown fields into `Extra = BTreeMap<String, Lww<JsonText>>`; a newer build adding any non-`{v,at}` field makes the whole record fail `TryFrom` and be rejected forever on old devices. The wire rule needs a serde test.
- `Lww<Option<SessionSnapshot>>` equality includes the serde-skipped `restore_state`, so nearly every debounced save mints a stamp and seq despite the "no-op if unchanged" claim.
- CRX: "keep only valid proofs" is more lenient than Chromium (which fails on any invalid proof); no EOCD-in-header check; `CWS_PUBLISHER_KEY_SHA256 = [0;32]` fails closed until pinned (fine, but the fixture test is load-bearing).
- `BookmarkId::is_root` (`<= 4`) treats the nil UUID as a root.
- `synchronous=NORMAL`: power loss can roll `clock_last` back below a stamp already uploaded; HLC wall time makes reuse unlikely, not impossible.
- Every read takes `&mut Profile` through `RefCell`; any synchronous shell callback during a core call panics. "Core never calls back" must stay true.

**Candidate 2**
- `MergeOutcome` all-`KeptLocal` -> `Unchanged` -> no seq bump: on the promised dumb key/value server a stale upload that overwrites a record is never repaired; late joiners get the stale copy. Fix: bump seq whenever merged != incoming.
- `Lww::merge` `Equal -> Same`: a copied profile directory shares a `DeviceId`, so two devices mint equal stamps for different values and never converge. Add a value tiebreak.
- "Local ops never need repair" is false once a skipped raw placement exists. After concurrent moves X->N (t1) and N->X (t2), repair shows N at OTHER with X under it; a local move X->BAR (t3) makes t2 valid again, so a full `repair()` puts N under X while the incremental `bookmark_tree` leaves N at OTHER. Rebuild after every local move too and drop incremental maintenance.
- Hoist is HLC-based, not causal: a concurrent add with an earlier HLC is trashed though the deleter never saw it; a mere reorder inside F after F's deletion revives the node under OTHER. The pinned scenario tests depend on wall-clock ordering between two in-memory profiles and can flake.
- `Staged`/`VerifiedPackage`/`Package` are all-`pub`; `Verification::Crx3` is a claim any caller can fabricate before `commit`.
- Store installs require only a developer proof plus id match; Chrome additionally requires the Web Store publisher proof.
- `uninstall` tombstones `storage.sync` rows, propagating a settings wipe to every device; Chrome keeps `storage.sync` across uninstall/reinstall.
- Windows sample calls `AddBrowserExtensionAsync` for every installed extension on each launch although WebView2 persists them; no engine-id mapping for XPI/keyless ids.
- `commit` step 3 removes other version dirs while WebView2 may still hold the previous one open; `<id>/<version>` dirs mean a different build of the same version silently reuses the existing dir.
- `set_cursor` is a separate `write` from `apply`, so "cursor commits with the apply" does not hold (harmless, but overstated).
- `Arc<Mutex<Connection>>` lets a worker-thread `commit` (rename + tx) stall the UI thread on the mutex; nothing stops `install_blocking` on the UI thread; `busy_timeout=5000` is meaningless with one connection.
- No injectable time source, so HLC skew is untested; per-visit rows with `seq` make history dominate outgoing changes.

**Candidate 3**
- Bookmark merge is non-convergent. A: move X under Y (t1); B: move Y under X (t2 > t1). Device C receives t2 then t1: Y under X applied, then X under Y hits the cycle guard and X is written to RECOVERY_ROOT. Device D receives t1 then t2: X under Y applied, then Y is written to RECOVERY_ROOT. C and D differ permanently, and the redirected parents are never uploaded (remote applies do not log), so no later exchange fixes it. Per-entry determinism does not give order independence.
- Orphan guard is order-dependent and permanent: child before parent creation -> child parked at RECOVERY_ROOT forever; child before parent tombstone -> child dangling under a tombstone (invalid tree); tombstone before child -> redirected. `remove_bookmark` does not specify subtree tombstoning, so even a local folder delete leaves orphans.
- CRX3 verification is wrong: `Crx3` keeps only the first RSA `public_key`/`signature` and the zip, dropping `signed_header_data` and `crx_id`; the signed message is `"CRX3 SignedData\0" || u32le(len) || signed_header_data || zip`, not "header+zip". Store CRXs carry developer and publisher proofs in unspecified order, so deriving the id from the first proof falsely rejects store items when the publisher proof comes first. No `crx_id` binding.
- UI thread blocks on network: `install` returns a `Future` the shell polls on its main context and the pipeline uses blocking `ureq`; core has no executor or worker thread, so the download runs inside `poll` on the UI thread.
- Extension functions take only `&ProfileDir` (no connection, no clock): they must open a second SQLite connection and cannot mint HLCs from the profile clock. `Store` has no `HlcClock` field and `conn` is not behind interior mutability, so `&Profile` cannot start a transaction or tick a clock.
- No per-row dirty marking or re-upload rule; `change_log` grows one row per visit forever; `changes_since` is unpaged; `Record<T>` has no value tiebreak for equal stamps.
- Synced `HistoryVisit.url_id: UrlId(i64)` is a local rowid; sessions are absent from `EntityKind`/`ChangeBatch`, so "tabs on this device" never syncs; unpacked extensions sit in the synced `installed_extension` table with `ExtensionSource::Unpacked`, so peers would try to install them.
- `profile.lock` with PID + start time: TOCTOU between staleness check and steal, platform-specific start-time lookup, `Drop` removal; `std::fs::File::try_lock` exists and the OS releases it on crash.
- No `user_version`/migration mechanism; `CURRENT` is a third durable step outside the rename+tx; `synchronous=FULL` costs an fsync per navigation on the UI thread for a guarantee WAL+NORMAL already gives against process crashes.
- Missing from the grounding's list: `storage.local`, `storage` get-all/remove/clear/onChanged/quotas, enable/disable reload flow, pending-install reconcile API (`reconcile_after_sync` is referenced, never defined), typed prefs.