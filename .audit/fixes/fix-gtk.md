18 FIXED, test `permissions::tests::a_request_during_a_provisional_load_is_from_the_page_on_screen` (plus `an_answer_holds_only_for_the_site_it_was_asked_for`). The prompt now names the site from `Tab::committed_uri()`, and an Allow only takes effect if the same scheme, host and port is still committed when the user answers; otherwise the request is denied.

19 FIXED, tests `session::tests::a_tab_still_loading_its_first_page_is_saved` and `session::tests::a_restored_tab_keeps_its_back_forward_state_until_it_commits`. `window_snapshot` now uses a new `Tab::session_uri()`: the committed URI, or else the URI still loading. Tabs that have not committed, including restored ones, keep their URL and back/forward blob. Only blank tabs are skipped.

20 FIXED, tests `browser::tests::an_error_page_leaves_the_history_title_of_the_page_that_failed` and `browser::tests::leaving_a_page_leaves_its_history_title`. `title_changed` no longer writes to history while `Tab::shows_error_page()` is true, and it ignores empty titles. Testing showed the second part was needed: WebKit clears the title before it reports the commit, so every navigation set the previous page's history title to "". The second test failed with `left: Some("")`, `right: Some("A")` before the fix.

21 FIXED, test `window::ext_actions::tests::a_badge_update_leaves_the_open_popup_alone` (plus a guard test, `unloading_the_extension_closes_its_popup`). `ExtensionActions::rebuild` now updates each existing button in place (content, tooltip, order). It closes the open popup only when that popup's extension is no longer loaded. A `setPopup` call also leaves the open popup alone, as Chrome does.

22 FIXED, tests `window::ext_actions::tests::window_close_in_the_popup_page_closes_the_popup` and `window::ext_actions::tests::a_dismissed_popup_page_is_dropped`. The popup WebView's `close` signal now closes the popup. The popover's `closed` handler removes the popup from `ExtensionActions` through a weak reference, so the page is dropped. The report's `runtime.connect`/onDisconnect consequence does not apply, because ports are not implemented.

23 FIXED, test `extensions::tests::an_extension_the_runtime_cannot_load_is_not_reported_as_installed`.
- `load_into_runtime` now returns the `LoadError` and records it on `Browser`.
- `run_install_job` returns a new `InstallFailure::Load`, and `set_extension_enabled` returns a new `EnableFailure::Load`.
- The dialog shows the failure in a toast ("Installed X, but it cannot run: …" or "Enabled, but it cannot run: …").
- The extension's row shows "Not running" with the error. This also covers load failures at startup.

24 FIXED, test `app::tests::a_restart_frees_the_profile_before_the_new_process_starts`. It runs `app::tests::releasing_the_profile_frees_its_lock` in a child process, because the extension runtime allows one per process.
- Before spawning, the new `release_profile` force-closes open dialogs, destroys every window, drops the `Browser` and runs the pending main-loop work (bounded to 2 s). It logs a warning if the profile is still alive.
- The History, Bookmarks and Extensions dialogs no longer hold a strong `Browser` or profile. Their handler cycles are what kept the profile locked after a dialog had been opened; the test fails if one of them holds it again.
- `Browser::shutdown` now marks the session as final, so the teardown cannot overwrite it.

25 FIXED, test `browser::tests::extensions_see_the_committed_url_while_the_next_page_loads`. `TabHost::tabs` now reports `Tab::committed_uri()` as `TabInfo.url`. I did not add `pendingUrl`, because `TabInfo` lives in `vsesvit-webext`.

26 FIXED, test `tab::tests::saved_state_the_engine_cannot_read_falls_back_to_the_url`. Saved state is now decoded by calling `webkit_web_view_session_state_new` directly. A NULL result becomes `None`, the fallback is logged, and the tab loads the saved URL. Before the fix the test panicked with `assertion failed: !ptr.is_null()`.

Files changed (under C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\fix-gtk\crates\vsesvit-gtk\src\):
- app.rs
- browser.rs
- extensions.rs
- lib.rs
- permissions.rs
- session.rs
- tab.rs
- test_support.rs (new, test-only: a shared test browser, a loopback HTTP server and wait helpers)
- window\ext_actions.rs
- dialogs\extensions.rs
- dialogs\history.rs
- dialogs\bookmarks.rs

Nothing outside `crates/vsesvit-gtk/src` changed, and `src/updates/` is untouched. The new tests use `#[gtk::test]`, so the Linux tests now need a display (WSLg). They passed 5 times in a row.

Output observed:
- Before the fixes: `test result: FAILED. 54 passed; 12 failed; 1 ignored`. The failures were the new tests above, each for its finding's reason, for example `left: Some("“127.0.0.2”") right: Some("“127.0.0.1”")`, `left: [] right: ["http://127.0.0.1:45383/slow"]`, `the profile is still held: Some(Locked)`, `the install reported success: Ok(Some(...))`, `the popup was closed`.
- After (Linux, gtk crate): `test result: ok. 67 passed; 0 failed; 1 ignored` (the ignored one is the child-process test).
- `bash scripts/wsl.sh test --workspace`: exit=0, no failures.
- `cargo test --workspace` on Windows: every binary `ok`, including `63 passed` for vsesvit_winui.
- clippy `--workspace --all-targets`: `Finished` with no warnings on both Linux and Windows.
- Linux self-test, final run: `[self-test] PASS: 14/14 checks passed; report at /home/dy/st-fix-gtk/report.json`.

Of 4 self-test runs, 2 passed and 2 had one failure each, in code I did not change:
- One `screenshot` check timed out after 15 s waiting for a frame (the next run passed at 336 ms).
- One `content_script` check failed with "Receiving end does not exist". The probe's content script sends one message at document_end, and it arrived before the background page had loaded; that race is in the webext area.