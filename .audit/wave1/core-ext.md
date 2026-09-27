## Report: UNIT B (extension install pipeline and testkit)

The install pipeline and the testkit are done and pass on Windows and Linux. The real Chrome Web Store install of uBlock Origin Lite passed publisher verification on both.

### Files (owned only), under `C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\core-ext\`
- `crates\vsesvit-core\src\extensions\mod.rs`:
  - `ExtensionId` (charset, reserved names, path ids like Chromium's), `Verification`, `InstalledExtension.verification`.
  - `list`, `get`, `prepare_install`, `commit`, `set_enabled`, `uninstall`, `reconcile`, `set_engine_id`, `reload_unpacked`, `on_open`.
  - Unit tests, including offline store/reconcile tests.
- `crates\vsesvit-core\src\extensions\install.rs`: `InstallSource::parse`/`from_path`, the store URLs, `InstallJob::run(progress)` with `InstallPhase`, ureq fetch (204 or empty body means `NotAvailable`, AMO v5 with its sha256 checked), `unpack_zip`, `inject_key`.
- `crates\vsesvit-core\src\extensions\crx.rs`:
  - Hand-rolled protobuf reader.
  - RSA PKCS#1 v1.5 and ECDSA P-256 proofs. Every proof must verify, a developer proof is required, and store downloads need the publisher proof.
  - Rejects zip EOCD magic in the header. The publisher key hash is pinned.
- `crates\vsesvit-core\src\extensions\manifest.rs`: `Manifest::load`/`parse` with all the normalizations, `__MSG_*__` lookup, tolerant JSON, `key_id()`. `MatchPattern` and `RelPath` are unchanged and their tests still pass.
- `crates\vsesvit-core\src\extensions\schema.sql`: added `extension_installs.verification`.
- `crates\vsesvit-core\src\testkit\{mod.rs, crx_writer.rs, fixture_server.rs}`: `PROBE_ID = "eonajgebgeenbhiiobbhmkafolkeghdb"`, `probe_crx()` (same bytes every call), `write_crx3`/`sign_crx3`/`encode_crx3`/`zip_files`, `CrxKey`, `FixtureServer`.
- `crates\vsesvit-core\tests\extensions_{crx,manifest,source,install,testkit,network}.rs`.
- `tests\fixtures\keys\{test-only-probe-key.pem, test-only-second-key.pem, README.md}`: RSA-2048 keys, labelled test-only both inside each PEM and in the README.
- `[dependencies]` and `[features]` needed no changes.

### Evidence
Final Windows run (`cargo test -p vsesvit-core --features testkit --lib --test extensions_*`):
```
test result: ok. 19 passed; 0 failed; ...   (lib unit tests)
test result: ok. 11 passed; 0 failed; ...   (extensions_crx)
test result: ok. 13 passed; 0 failed; ...   (extensions_install)
test result: ok. 9 passed; 0 failed; ...    (extensions_manifest)
test result: ok. 0 passed; 0 failed; 2 ignored; ... (extensions_network)
test result: ok. 3 passed; 0 failed; ...    (extensions_source)
test result: ok. 4 passed; 0 failed; ...    (extensions_testkit)
```
Linux (`bash scripts/wsl.sh test ...`) gave identical counts, all ok.

Clippy with `--features testkit --all-targets` on both OSes ends like this:
```
warning: `vsesvit-core` (lib) generated 2 warnings (run `cargo clippy --fix --lib -p vsesvit-core -- ` to apply 1 suggestion)
warning: `vsesvit-core` (lib test) generated 2 warnings (2 duplicates)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.72s
```
Both warnings are in unit A's files (`crdt.rs` infallible_try_from, `session.rs` new_without_default). My files have zero warnings, with or without the feature. I also checked with lib.rs's blanket `#![allow(unused…)]` temporarily removed, on both OSes. `cargo build --workspace` succeeds on Windows.

**Network test**, run explicitly on Windows and on Linux (`-- --ignored --nocapture`):
- **uBlock Origin Lite from the Chrome Web Store:** publisher verification passed. It installed as `ChromeWebStore { publisher_verified: true }`.
  - The file is a 9,763,168-byte CRX with a 1,309-byte header, version 2026.926.2202.
  - It carries 3 proofs, and all of them verified:
    - RSA from a Google key, SHA-256 `b5e40962…43786b0` (neither the developer nor the publisher key).
    - RSA from the developer key, SHA-256 `33a98074…aa5adafe`, which derives to the requested id.
    - ECDSA P-256 from the publisher key, SHA-256 `61f7f2a6…8366367cf`.
  - The install dir is named `2026.926.2202_e27ade0e` on both OSes.
- **uBlock Origin 1.75.0 from AMO:** installed as `uBlock0@raymondhill.net`, `AmoHash`.

Tests cover everything the task listed; the less obvious cases:
- **End to end:** the probe runs on a worker thread. Re-installing the same bytes keeps the same dir, `engine_id` and enabled state. Changed bytes with the same version go to a new dir, and the old one is removed at the next open. An older version never replaces a newer one. Staging is wiped at open.
- **Hostile archives:** zip-slip is rejected on both the XPI and the signed CRX path, and a scan confirms nothing lands outside the staging dir. Also rejected: absolute paths, backslashes, symlinks, reserved names, top-level `_*` names, case-only duplicates, and an 8 MiB zip bomb. `_metadata/` is stripped.
- **Store and reconcile paths** are tested offline: a probe install is relabelled as a Web Store download, and a direct edit of the synced record stands in for another device.

### Deviations
- **`run` signature:** `InstallJob::run` takes `progress: &mut dyn FnMut(InstallPhase)`, as the synthesis says. The shell examples that call `job.run()` need to pass a closure.
- **Download handling:** it stays in memory, capped at 256 MiB, and is hashed there. There is no `staging/archive` file.
- **API additions:** `CrxError::UnsupportedProof` is replaced by `InvalidProof(ProofAlgorithm)` and `ZipMagicInHeader`. New `InstallError` variants: `BadStoreResponse`, `TooLargeUnpacked`, `NotUnpacked`. `VerifiedCrx` has `publisher_verified`, and a doc-hidden `crx::verify_with_publisher` lets tests stand in for the Web Store key.
- **Bare AMO slugs** such as `ublock-origin` are not accepted, because they can't be told apart from a relative path. AMO URLs and bare gecko ids (`x@y`, `{uuid}`) are accepted.
- **`reconcile()` does not delete removed dirs;** the next open's cleanup does, because the engine may still hold the files. `uninstall()` deletes them right away where it can.
- **Unpacked ids (`for_unpacked_dir`):** whether the path is hashed as UTF-16LE (Windows) or UTF-8 is decided by its shape (`X:` prefix), because core has no `cfg(target_os)`. `from_path` also resolves `..` itself, because on Linux the standard library keeps it and the same dir would otherwise get two ids.
- **Manifest locale** comes from `LC_ALL`/`LC_MESSAGES`/`LANG` and defaults to `en`. On Windows that usually means the extension's `default_locale` is used.
- **XPI ids:** an XPI with no gecko id gets an id from its file path. For AMO, the manifest gecko id must equal the API `guid`.
- **Top-level `_*` names:** `_locales` and `_platform_specific` are allowed, `_metadata` and `__MACOSX` are skipped, and everything else is rejected.

### Open problems
- **Tests skipped by plain `cargo test`:** the testkit-dependent test files are gated with `#![cfg(feature = "testkit")]`, so `cargo test -p vsesvit-core` without `--features testkit` does not run them. Adding `vsesvit-core = { path = ".", features = ["testkit"] }` under `[dev-dependencies]` fixes this; I tried it and it works. That section isn't mine, so I reverted it; the merger should decide.
- **Firefox version strings:** manifest versions use Chrome's rules (parts up to 65535, no letters). An AMO add-on with a Firefox-only version string would be refused.
- **Throwaway code to discard:** I wrote stand-ins for unit A's functions in `src\lib.rs`, `src\db.rs`, `src\crdt.rs` and `src\extensions\sync_table.rs` so my tests could run. My code relies only on the declared API: `Profile::write`, `db::Tx` (`sql`/`stamp`/`seq`), `ExtensionsTable::load`/`store`, and the `extensions` table layout.
- **Stray fixture dir:** `tests\fixtures\extensions\probe\_metadata\` looks like something Chromium wrote when a shell loaded the probe dir directly. `probe_crx()` lists its files explicitly and leaves that dir out; I didn't delete it.

No processes are left running, and the temp dirs the tests create are removed on drop.