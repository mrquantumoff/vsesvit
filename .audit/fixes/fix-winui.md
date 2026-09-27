27 FIXED — `automation::page_forgery` (--ui-smoke step `05b-pages-cannot-forge-shortcuts`) plus unit tests `shortcuts::tests::only_calls_of_the_shortcut_binding_count` and `the_script_reports_only_through_its_binding`: the shortcut script now runs in an isolated world with a secret name, set up through DevTools (`Page.addScriptToEvaluateOnNewDocument`), and reports through a `Runtime.addBinding` binding that only exists in that world; web messages are turned off and the nonce is gone.
28 FIXED — --ui-smoke step `06b-popups-without-a-gesture`: `new_window_requested` blocks (`SetHandled(true)` with no new window) any request whose `IsUserInitiated` is false.
29 FIXED — --ui-smoke step `01b-planned-url-before-the-engine`: `open_tab` fills the tab's `requested` URL (and a restored tab's title, carried in the new `TabPlan.title`) before the engine view starts, so `session_url()` never saves about:blank in its place.
30 FIXED — `extensions::tests::a_new_version_of_a_keyless_install_keeps_its_engine_id` and --ui-smoke step `22-xpi-update-keeps-its-engine-id`: before the first load of a managed install that has no `key` (engine_id not yet recorded), the sync writes a fixed `key` into manifest.json, built from core's extension id, so every version gets the same engine id and keeps its storage.
31 FIXED — `extensions::tests::an_extension_core_recorded_follows_core_whatever_its_name` and `an_unrecorded_extension_is_removed_whatever_its_name`: sync pass 2 applies enable/disable to anything core recorded, and skips only the two built-in extensions, now matched by their fixed ids (dgiklkfkllikcanfonkcabmbdfmgleag, mhjfbmdgcfjbbpaeojofohoefgiehjai, read from the live engine) instead of by display name.
32 FIXED — `extensions::tests::a_disabled_extension_is_switched_off_right_after_it_is_added` and `a_failed_switch_is_reported_for_the_dialog`: pass 1 switches a disabled extension off immediately after adding it, and any enable/disable failure goes into `engine_errors` so the Extensions dialog shows it.
33 FIXED — `xaml::tests::characters_xml_cannot_hold_are_dropped` and --ui-smoke step `17a-history-rows-match-pages`: `xaml::escape` drops U+FFFE and U+FFFF, and the history dialog keeps an entry in `shown` only if its row was actually added.
34 FIXED — `instance::tests::the_program_is_dropped_however_the_launch_spelled_it`: `command_line_args` always drops the first token, which is always the program in a forwarded unpackaged command line.
35 FIXED — `window::tests::a_window_from_a_disconnected_display_moves_onto_the_nearest` (plus 5 related tests): `apply_bounds` asks `DisplayArea::GetFromRect` for the work areas, and the new `restored_rect` moves (and shrinks if needed) a window whose title bar cannot be grabbed onto the nearest display's work area.

Things to know:
- **27, reaching the controller:** `AcceleratorKeyPressed` cannot be used. The WinUI `WebView2` control keeps its `CoreWebView2Controller` private and never subscribes to that event (I checked microsoft-ui-xaml `WebView2.cpp`/`.h`).
- **27, spike results:** the isolated world design needs both `Page.enable` and `Runtime.enable`. Without them the script does not run and the binding never fires. The spike also showed:
  - `chrome.webview` does not exist in the isolated world.
  - The script runs before the page's own scripts, and its capture listener fires before the page's.
  - It keeps working after a cross-origin navigation.
  - With `Runtime.enable` on, the known detection trick (a page-defined `Error.stack` getter run by `console.debug`) does not fire in WebView2 154.
- **27, host-side check not added:** I did not add the review's "ignore messages from non-active tabs" filter. Once forgery is impossible it is not needed, and it could break repeated Ctrl+Tab if focus stays in the tab just hidden.
- **30, existing installs:** an XPI/AMO extension installed before this fix still changes its engine id once, on its next update. After that its id stays the same. Unpacked folders are never written to.
- **Bindings:** added `get_IsUserInitiated`, `GetDevToolsProtocolEventReceiver`, `DevToolsProtocolEventReceived`, `get_ParameterObjectAsJson`, `DisplayArea::GetFromRect`, `get_WorkArea` and `DisplayAreaFallback`, regenerated with `tools/bindgen`. The NuGet cache was copied from the `win2` copy and the tool checked its SHA-256.
- **Line endings:** the originally CRLF files are CRLF again; `cargo fmt --check` is clean.

Files changed (all in `C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\fix-winui\crates\vsesvit-winui\`):
- bindings.txt
- src\bindings.rs
- src\shortcuts.rs
- src\tab.rs
- src\browser.rs
- src\window\mod.rs
- src\session.rs
- src\extensions.rs
- src\engine.rs
- src\xaml.rs
- src\dialogs\history.rs
- src\instance.rs
- src\automation\mod.rs
- src\automation\dialog_steps.rs

Test output:
- **Red (before the fixes):**
  - Unit tests, 11 failed:
    - `extensions`: 5 (30, 31, 32)
    - `window::tests`: 4 (35)
    - `xaml::tests::characters_xml_cannot_hold_are_dropped` (33)
    - `instance::tests::the_program_is_dropped_however_the_launch_spelled_it` (34)
  - --ui-smoke failures (saved in `scratchpad\research\smoke-red.json`):
    - `01b`: `session_url ""`
    - `05b`: remapped key closed the tab, 1 secret seen, forged command ran
    - `06b`: 2 popups opened
    - `17a`: rows 1 vs pages 2
    - `22`: engine ids differed, visits `["1","1"]`
- **Windows `cargo test --workspace`:** exit 0, every suite ok; vsesvit-winui `test result: ok. 81 passed; 0 failed`
- **Windows `cargo clippy --workspace --all-targets`:** exit 0, no warnings; `-p vsesvit-winui --no-default-features` is also clean.
- **`bash scripts/wsl.sh clippy --workspace --all-targets`:** exit 0, no warnings, `Finished dev profile ... in 30.86s`
- **`bash scripts/wsl.sh test --workspace`:** exit 0, 33 `test result: ok` lines, none failed.
- **Windows self-test:** exit 0, `ok True 14 checks`, all PASS (profile_open … session, screenshot). I did not run the Linux self-test because this crate only runs on Windows.
- **--ui-smoke (final, debug build):** exit 0, `ok True, steps 30, passed 30, failed []`
  - `05b`: `remapped_key_closed_the_tab false, secrets_seen "0", forged_command_ran false, ctrl_click_still_opens_a_background_tab true`
  - `06b`: `popups 0`; the log shows `blocked a popup ... no user gesture` twice.
  - `22`: `engine_ids [ficobhcanhlepkmnolpkjhpldckiokpc, ficobhcanhlepkmnolpkjhpldckiokpc], visits ["1","2"]`
- No vsesvit processes were left running.