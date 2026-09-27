The Windows shell is built and runs: in `--ui-smoke` runs the page renders, tabs open and close, the address box shows the URL, and the extension popup works. All 13 scripted steps passed in both the debug and release builds, and I looked at the PNGs myself. A few items remain unexercised because testing them would take focus or need real mouse or keyboard input; they are listed at the end.

## Files (all under `C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\winui\crates\vsesvit-winui\`)
- `Cargo.toml`: adds build-deps `sha2` and `zip`.
- `build.rs`: copies the WebView2 DLL next to the exe (details below).
- `bindings.txt`: the bindgen filter.
- `.gitignore`
- `tools/nuget.rs`: pinned packages, SHA-256 check, download through Windows' `curl.exe`, zip extraction.
- `tools/bindgen/{Cargo.toml,Cargo.lock,src/main.rs}`: a separate workspace. It downloads the four pinned packages into `target/nuget` and regenerates bindings.
- `src/`: `lib.rs`, `bindings.rs` (generated), `app.rs`, `automation.rs`, `bookmarks_bar.rs`, `browser.rs`, `capture.rs`, `cli.rs`, `config.rs`, `dialogs.rs`, `engine.rs`, `exec.rs`, `logging.rs`, `omnibox.rs`, `platform.rs`, `popup.rs`, `shortcuts.rs`, `tab.rs`, `window.rs`, `xaml.rs`.

Outside my files: the root `Cargo.lock` changed by two lines (`sha2` and `zip` added under vsesvit-winui).

## What I observed working
- **Build pipeline:**
  - The bindgen tool downloaded all four pinned packages and they matched the pinned SHA-256s. Running it again produced a byte-identical `bindings.rs`.
  - After I deleted the cached WebView2 package, `build.rs` downloaded it again, verified it, and extracted the DLL into `target/debug` and `target/release`. The DLL's hash matches the package entry.
- **Startup:**
  - The OS dynamic dependency loaded `Microsoft.WindowsAppRuntime.2_2.5.1.0_x64`.
  - The shared environment was created with extensions enabled (WebView2 154.0.4258.37), using `<profile>/engine` as the user data folder.
- **Smoke run** (`vsesvit http://127.0.0.1:18765/index.html --ui-smoke <copy>\shots --profile-dir <copy>\target\smoke-profile --load-extension <probe copy without _metadata>`): exit code 0, `smoke.json` has ok=true for all 13 steps, and no screenshot is a flat colour.
  1. First tab: title "Vsesvit fixture", address box shows the URL.
  2. A second tab: its favicon is shown, and it is active.
  3. Find returned 1 match, and the highlights are visible.
  4. Closing the second tab leaves 1 tab showing index.html.
  5. Ctrl+Shift+T reopened the closed tab.
  6. Ctrl+T and Ctrl+W typed into the page opened and closed a blank tab.
  7. `window.open` opened a tab right after its opener, and `window.opener` is set.
  8. Ctrl+click opened a background tab; the first tab stayed active.
  9. The bookmarks bar rendered link and folder buttons (the smoke run supplies these items itself).
  10. Enter in the address box navigated to `#typed`, and Back became enabled.
  11. A second window opened, sharing the same engine.
  12. All five dialogs built without error (they were not shown).
  13. The probe extension's popup rendered "Visits recorded: 1", sized to its content.
- **Screenshots:** the tab strip sits in the title bar with the caption buttons, and the toolbar, bookmarks bar and page render. The window stayed inactive in every shot (caption glyph brightness 102 in all of them).
- **Release build:** the smoke passed 3 times with exit code 0. An earlier version crashed on exit (exit code 139). The fix was to release XAML and WebView2 objects before `Application.Exit`.
- **Command line:** `--self-test` exits 1 with a "not part of this build yet" message, `--help` exits 0, and an unknown flag exits 2.

## Deviations
- **Two extra flags:** `--ui-smoke OUT_DIR`, which is the scripted run the self-test can build on, and `--load-extension DIR`, which loads an unpacked folder for one session, Chrome-style. They let me verify tabs, capture and the popup now.
- **Shortcuts while the page has focus:** WinUI's WebView2 never forwards keys to XAML, so XAML shortcuts can't fire there. A script injected into every page reports shortcut keys to the shell, with a random per-process code so pages can't fake them. When XAML has focus, normal XAML keyboard shortcuts are used.
- **Zoom:** Ctrl+plus/minus/0 only work while the page has focus, through WebView2's own zoom. WinUI does not expose the zoom factor, so zoom from the toolbar or address box is not possible.
- **Programmatic focus:** focus is only set when our window is already in the foreground, so the app never steals activation.
- **Address box input:** addresses on the local machine (localhost, 127.x) get `http://` and Windows paths become `file:///`; everything else gets `https://`.
- **Where core plugs in:** each hook in `browser.rs` logs what it would do. `star_clicked` and `install_extension` report "not connected yet". about:blank commits are not passed to the history hook.

## Open problems
- **Not exercised:** tab drag-reorder, real middle-click, the XAML keyboard shortcuts, fullscreen (it would cover your screen), dialogs actually shown (showing one takes focus), the missing-runtime message box, and interactive-mode activation.
- **Find bar:** the find itself works, but WebView2's find bar did not appear in the window capture. Whether it appears on Ctrl+F from XAML focus is unconfirmed.
- **Closed web views:** WebView2 calls on a web view that has been closed never complete. The smoke run guards them with `exec::timeout`; the self-test should do the same.
- **Data URLs:** WebView2 reports an empty current URL for `data:` pages, so the shell falls back to the requested URL.
- **Your input during testing:** twice, someone used the test window (typed sites, opened tabs, tried a Chrome Web Store install). I discarded those runs and ran again.

## Final output
`cargo test -p vsesvit-winui`:
```
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
`cargo clippy -p vsesvit-winui --all-targets`: my crate has no warnings. The only two come from vsesvit-core:
```
warning: `vsesvit-core` (lib) generated 2 warnings ...
    Checking vsesvit-winui v0.1.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.79s
```
`bash scripts/wsl.sh check --workspace` (clippy of this crate in WSL is also clean):
```
    Checking vsesvit-winui v0.1.0 (/mnt/c/.../ws/winui/crates/vsesvit-winui)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.31s
```
No test processes are left running.

Screenshots and the report are in `C:\Users\mrqua\AppData\Local\Temp\claude\C--Users-mrqua-git-vsesvit\f74805b8-a346-4e4e-84ab-b88787d8eea0\scratchpad\ws\winui\shots\` (`smoke.json` and the `01-…` to `13-….png` files).