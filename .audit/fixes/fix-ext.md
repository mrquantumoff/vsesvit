2 FIXED an_xpi_or_unpacked_dir_cannot_take_the_id_of_a_signed_install (tests/extensions_install.rs) + an_unpacked_dir_with_a_store_extension_key_cannot_take_its_id (extensions/mod.rs store_tests): XPI gecko ids and AMO guids that look like Chrome ids are now rejected (`xpi_id` in install.rs). `commit` has a new `check_id` step that refuses an install not verified by a developer key (unpacked, XPI, AMO) when the id belongs to a ChromeWebStore or LocalCrx row, or when the synced record wants that id from the Chrome Web Store (`InstallError::VerifiedIdTaken`).
3 FIXED an_xpi_manifest_key_never_reaches_the_engine: XPI staging now strips `key` from manifest.json before the manifest loads (`inject_key` became `set_key(root, Option<&[u8]>)`: CRX gets its verified key, XPI gets none), so WebView2 can never load an XPI under a Chrome id derived from a key.
4 FIXED gecko_ids_that_differ_only_in_case_never_share_a_dir + a_leftover_dir_in_another_letter_case_does_not_get_a_live_install_collected: `commit` refuses an id that differs from an installed id only in letter case (`InstallError::IdCaseConflict`, `COLLATE NOCASE` lookup), and the open-time GC compares version dirs against rows ignoring case.
5 FIXED a_held_file_never_leaves_a_partial_dir_for_a_reinstall_to_reuse (Windows only): uninstall and the GC now delete through `remove_whole`, which first renames the dir into `staging/` (wiped at every open) and only then deletes it. I checked on this machine that Windows refuses the rename while a file inside is held open, so the dir stays whole and a content-addressed dir that exists is always complete.
6 FIXED a_package_path_that_is_not_unicode_fails_cleanly: `prepare_install` now returns `InstallError::PathNotUnicode` for a CrxFile, XpiFile or Unpacked path that is not valid Unicode. `commit` no longer reaches the `to_json` panic.
7 FIXED the_version_dir_name_carries_128_bits_of_the_archive_hash: version dirs are now named `<version>_<first 16 bytes / 32 hex of the archive SHA-256>` instead of 4 bytes. Rows already on disk keep their stored dir, so no migration is needed.
8 FIXED removing_a_local_copy_leaves_the_store_extension_wanted: `uninstall` changes the synced record only when there is no local row or the row came from a store. Removing a local copy (.crx, .xpi, unpacked) under a store id only deletes local state, and `reconcile` reinstalls the store copy.

Two behaviour changes for the shells' wording:
- A developer loading an unpacked dir that carries their own store extension's `key` is now refused until they remove the store install.
- After finding 8, the store copy comes back after its local replacement is removed.

The comment `<version>_<hash8>` is now stale in files outside my ownership: crates/vsesvit-core/src/lib.rs:86, docs/design/core.md:325 and docs/PLAN.md:58. I left them for the orchestrator.

Files changed:
- C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\fix-ext\crates\vsesvit-core\src\extensions\mod.rs (CRLF line endings kept as in the original)
- C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\fix-ext\crates\vsesvit-core\src\extensions\install.rs
- C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\fix-ext\crates\vsesvit-core\src\extensions\schema.sql (dir column comment only)
- C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\fix-ext\crates\vsesvit-core\tests\extensions_install.rs (new tests; the existing hash8 length assertion now expects 32)

Output observed:
- Before the fix, all 9 new tests failed on Windows at their target assertions, including the mod.rs:675 "install metadata always serializes" panic. On Linux 7 failed: the case-leftover test passes there (case-sensitive filesystem) and the held-file test is Windows-only.
- Windows `cargo test --workspace`: every binary ok, including `extensions_install` 20 passed, `vsesvit_core` lib 34 passed, `vsesvit_winui` 63 passed; exit 0.
- Linux `bash scripts/wsl.sh test --workspace`: exit 0, all results ok. The `extensions_install` summary was garbled in that log, so I re-ran it alone: 19 passed, 0 failed.
- `cargo clippy --workspace --all-targets` (Windows) and `bash scripts/wsl.sh clippy --workspace --all-targets`: both "Finished" with no warnings or errors. An earlier Linux run warned about an unused Windows-only test helper; I moved it into its test.
- Windows self-test: `self-test: ok=true, report in ...\scratchpad\st\fix-ext`, `exit 0`.
- Linux self-test: `exit 0`. /home/dy/st-fix-ext/report.json has ok=True with all 14 checks passing: profile_open, install_crx, engine_loaded_extension, navigate, history_recorded, content_script, dnr_blocked, bookmark, tabs, tab_layout, popup, omnibox, session, screenshot.