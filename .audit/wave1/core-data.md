The Linux processes listed are another implementer's GTK soak run (target dir `gtk-84b33c6b`), not mine; nothing I started is still running. Nothing else is needed, so here is the report.

## UNIT A report: vsesvit-core data model

### Files created or changed (owned files only)
Under `C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\core-data\crates\vsesvit-core\`:

- `src/lib.rs` — `Profile::open` (LOCK via `File::try_lock`, pragmas, migrate, meta, bookmark model, `extensions::on_open`), `default_root`, `ProfilePaths`, `Profile::write`, removed the `#![allow(...)]` sketch attribute.
- `src/crdt.rs` — `DeviceId`, `Clock`, `Stamp` (bytes/hex), `Lww`, `Lattice`, new `Record<T> = Live(T) | Tombstone(Stamp)`, `JsonText`, `Extra`, `Seq`; unit tests.
- `src/db.rs` — open/migrate/meta, `Tx` (lazy stamp/seq, persisted in `commit`), column helpers.
- `src/schema.sql` — `search_engines` columns nullable with CHECKs for the live/tombstone shapes.
- `src/bookmarks.rs` — `Position` (midpoint + short append successor), record join, `BookmarkWire` boundary, `materialize` (rules 1–4), `Model`, full `Bookmarks` API incl. `import`, `BookmarksTable`; `is_root` rejects nil.
- `src/history.rs` — page/directive records, `normalize` (prune then cap 64), `record_visit/set_title/search/visits_between/delete_url/delete_range`, stats/frecency, `PagesTable`, `DeletionsTable`; `expire` removed.
- `src/session.rs` — save/restore with `tab_restore_state`, `other_devices`, `TabSnapshot` equality/ordering ignoring `restore_state`, `SessionsTable`.
- `src/prefs.rs` — get/set/reset, `PrefsTable` (only `synced = 1` exported); `HISTORY_RETENTION_DAYS` key removed with retention.
- `src/search.rs` — `EngineRecord { id, state: Record<EngineFields> }` with flat `EngineWire`, built-ins (DuckDuckGo default, Google, Bing, Wikipedia `w`), overlays, `classify`/`classify_url`, `Omnibox::resolve/suggest`, `EnginesTable`.
- `src/ext_storage.rs` — both areas, quotas (local writes only), change events, `StorageTable`.
- `src/sync.rs` — `SyncTable` (+ `max_stamp`, `compatible`, `settle`, `delete` hooks), `apply_one` rule, `apply` with per-kind post-steps, `changed_rows` paging, `engine_state`.
- `src/extensions/sync_table.rs` — `ExtensionRecord` join, `ExtensionsTable`.
- `tests/convergence.rs` — `lattice_laws` now covers all nine record types (folder and url bookmarks, pages, directives, sessions, extensions, storage items, prefs, engines) with `Extra` fields; rest unchanged.
- New `tests/core_{omnibox,wire,bookmarks,history,session,prefs,ext_storage,search,sync,profile}.rs`.
- `Cargo.toml` `[dev-dependencies]`: added `rusqlite` (schema-version test).
- Throwaway, not owned, discarded at merge: a minimal `ExtensionId::parse` body in `src/extensions/mod.rs` so my tests compile.

### What works (observed)
Windows (`cargo test -p vsesvit-core --no-fail-fast`), final lines per target:
```
running 15 tests  test result: ok. 15 passed  (lib)
running 3 tests   test result: ok. 3 passed; ... finished in 23.21s  (convergence: materialize_always_valid, lattice_laws, devices_converge)
core_bookmarks 10 passed, core_ext_storage 3, core_history 4, core_omnibox 10, core_prefs 3,
core_profile 4, core_search 3, core_session 3, core_sync 5, core_wire 9 — all "test result: ok"
```
Linux (`bash scripts/wsl.sh test -p vsesvit-core --no-fail-fast`): same targets, all `test result: ok` (convergence `3 passed ... finished in 6.43s`).

Clippy (`cargo clippy -p vsesvit-core --all-targets`, both hosts): zero warnings in my files; the remaining 41 warnings are all `unused`/`dead_code` in the other unit's stubbed `extensions/{mod,install,crx,manifest}.rs`. Last line: `Finished 'dev' profile [unoptimized + debuginfo] target(s)`.

`PROPTEST_CASES=1000 cargo test -p vsesvit-core --test convergence devices_converge` (Windows):
```
test devices_converge ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 94.37s
```
Default counts (256) are kept; run more with `PROPTEST_CASES=<n>` as above.

Synthesis items covered: no retention in the join and `History::expire` gone; 64-visit cap on every stored/exported page (tested); `move_to` raw-chain check with a visited set (tested with a raw cycle not involving the moved node); session equality ignores restore state (blob-only save mints no stamp/seq, tested); `is_root` rejects nil (tested); engine records are `Record<T>`; `{"v","at"}` wire rule tested with serde (`core_wire.rs`: shape of every kind, unknown LWW fields round-trip intact, other shapes rejected). Extensions desired-state merging is covered by `lattice_laws` and `core_sync.rs`, with no installs.

### Deviations from the design and why
1. `PageRecord::join` is plain union; the cap is applied in `normalize` (prune covered visits, then keep newest 64) before every store. Capping the raw union first is non-monotone: a stale record of since-deleted visits could evict live ones. Every stored/exported record still holds at most 64 visits.
2. Rows received from sync that equal the server's copy sit at `Seq::ZERO`; `changes_since` uses `seq >= since` and `upto` is the next cursor (last seq + 1). This is what lets a full export (`since = ZERO`) include them while normal cursors never re-upload them. `SyncTable::store` therefore takes `Seq` rather than `Option<Seq>`.
3. A change batch never splits rows sharing one seq (they would be skipped forever). The cut moves back before the group; a single group larger than `limit` is returned whole, so `records.len()` can exceed `limit`.
4. `Position::between(lo, None)` uses a length-prefixed successor (leading `z` run) instead of the midpoint, so appending stays short (20 000 appends ≤ 5 chars, tested). Keys remain plain base-62 strings under the documented validity rule.
5. Frecency is a pure function of the visit set (recency of the newest visit, count, typed count), so stats need no clock.
6. `Tree::children(ROOT)` lists the three visible roots and `Bookmarks::get/children` synthesize nodes for them; `ROOT` itself is not a valid add/move target.
7. `Omnibox::resolve` falls back to URL-only classification (`classify_url`, also public) when every engine has been deleted.
8. `engine_cache` is `<root>/cache` for non-default roots (tests never touch the real cache dir).

### Open problems
- The crate as a whole still warns because of the other unit's `todo!()` stubs; my files are clean.
- The raw-chain check over-refuses some legitimate moves (accepted by the design; tested behaviour).
- History directive pruning acts on stored state, so two devices agree once both hold the directive (which sync delivers); analysed and exercised by `devices_converge`, but a directive-heavy profile issues one `history_deletions` query per visit.
- `StorageTable::load` scans an extension's sync rows to match the hashed wire id (bounded by the 512-item quota).
- Windows `devices_converge` is ~4x slower than Linux (SQLite on NTFS): 23 s per 256 cases, still under a minute in debug.