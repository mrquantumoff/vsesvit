# End-to-end self-test contract

Both shells implement the same scripted end-to-end check. It is the definition of "the browser works" for this project. A reviewer runs it and reads the report instead of trusting anyone's summary.

```
vsesvit --self-test <out_dir> [--network]
```

The run uses a fresh profile at `<out_dir>/profile` and never touches the user's real profile. It writes `<out_dir>/report.json` and `<out_dir>/window.png`, then exits with 0 only if every check passed. Every check has a timeout (default 15 s). A timeout is a failure with the last observed value in `detail`.

Both shells also accept `--profile-dir <path>` in normal runs, so development never touches the default profile.

## Inputs, all from `vsesvit_core::testkit` (feature `testkit`)

- `FixtureServer::start()` serves `tests/fixtures/site/` (embedded in the binary) on `127.0.0.1:<random port>` and records every request path. `hits()` returns the paths seen so far.
- `testkit::probe_crx()` returns the bytes of a CRX3 built from `tests/fixtures/extensions/probe/` and signed with the committed test key, and `testkit::PROBE_ID` is the id that key derives to.

## Checks, in order

| name | what it proves | pass condition |
|---|---|---|
| `profile_open` | core opens a new profile | `Profile::open` succeeds |
| `install_crx` | CRX3 parse, verify, unpack, key injection, commit | install of `<out_dir>/probe.crx` succeeds, id == `PROBE_ID`, verification == `LocalCrx` |
| `engine_loaded_extension` | the engine runs the installed dir | Windows: `AddBrowserExtensionAsync` returns id == `PROBE_ID` (proves the injected `key`). Linux: the runtime lists the extension as loaded |
| `navigate` | tab + engine + fixture server | a tab loads `http://127.0.0.1:<port>/index.html` and its title becomes `Vsesvit fixture` |
| `history_recorded` | committed navigations reach core | core history has a visit for that URL |
| `content_script` | content script, `runtime.sendMessage`, background, `storage.local` | `document.documentElement.dataset.vsesvitProbe == "background-replied"` |
| `dnr_blocked` | declarativeNetRequest static rules | the server saw `/allowed.png` and never saw `/vsesvit-blocked/pixel.png` (checked 1 s after load) |
| `bookmark` | bookmark write + star state + bar | after bookmarking the current page into the bookmarks bar, `is_bookmarked` is true and the bar shows the item |
| `tabs` | multi-tab lifecycle | open a second tab on `/page2.html`, switch back, close it; one tab remains and it shows `index.html` |
| `tab_animation` (Linux so far) | tab rows grow in and shrink out without holding up the tabs | opening a tab adds its sidebar row at once, starting transparent when animations are on, and every row ends fully opaque (`tab-opened.png`); closing it drops the tab count and the live rows at once while its row shrinks out; reopening the closed tab brings back `page2.html`; after closing it again the rows settle to one per tab, none leaving |
| `tab_layout` | vertical tabs default, and the setting | a fresh profile shows the tab sidebar on the left; setting `tabs.position` to `right` moves it to the right and `top` shows the horizontal strip, each confirmed from widget geometry (sidebar x-position relative to the web view), then restored to `left` |
| `popup` | action popup page | opening the probe's action popup shows a document whose title is `visits=N` with N >= 1 |
| `omnibox` | input classification | `resolve("vsesvit fixture")` is a search on the default engine; `resolve("127.0.0.1:<port>/page2.html")` is that http URL |
| `address_completion` (Linux) | Chrome's omnibox keys in the address bar | with the fixture pages in history, typing `127.0.0` into the address bar shows `127.0.0.1:<port>` with `.1:<port>` selected, row 0 highlighted and the list as wide as the bar (`address-completion.png`); Down highlights row 1 and the box reads that row's fill; Escape goes back to row 0 and the completion; Enter opens `http://127.0.0.1:<port>/` |
| `address_completion` (Windows) | Chrome's omnibox keys in the address box | with the fixture pages in history, typing `127.0.0` (text put in the box with the caret at its end, then the box's own change handler) shows `127.0.0.1:<port>` with `.1:<port>` selected, the list open with row 0 highlighted in the list itself (`omnibox-inline.png`); Down, through the box's key handler, highlights row 1 and the box reads its `fill` with nothing selected; a second Down shows row 2's URL, and 300 ms later the rows and the highlight are unchanged, so writing it asked for no suggestions; Escape highlights row 0 again and brings back `127.0.0` with its selected completion; Enter loads `http://127.0.0.1:<port>/` in the tab and closes the list; the tab then goes back to `index.html` |
| `selection_search` (Linux) | the page context menu's item for selected text | with a fixture engine (`<origin>/search?q={searchTerms}`) as the default, selecting the page's heading reaches the tab through its selection script; the item the context menu adds for it reads `Search Fixture Search for “Vsesvit fixture page”` right after Copy, and choosing it opens `<origin>/search?q=Vsesvit+fixture+page` in a new selected tab next to the page |
| `selection_search` (Windows) | the page context menu's item for selected text | with a fixture engine (`<origin>/search?q={searchTerms}`) as the default, the page's heading selected and right-clicked through DevTools input, WebView2's `ContextMenuRequested` menu holds `Search <engine> for “Vsesvit fixture page”` right after `copy`; choosing it (the check's own handler sets it as the selected command and keeps the native menu from showing) opens `<origin>/search?q=Vsesvit+fixture+page` in a new foreground tab right after the page, which the fixture server sees as `/search`; the tab is closed and the default engine restored |
| `session` | session persistence | after `session().save`, `restore()` returns at least one window with at least one tab |
| `download` | downloads into the chosen folder, and the list | with the download folder preference set to `<out_dir>/downloads` while the browser runs, loading `/download.bin` (served as `application/octet-stream`) writes that folder's `download.bin` with the served bytes, core's list has a Completed entry for it, and the downloads toolbar button is shown; opening the Downloads view writes `downloads.png` |
| `new_tab_page` | the new tab page | a new tab shows the page's search box and a tile linking to the fixture server's origin, while still reading as blank (`about:blank`, the shell's "New tab" title); `new-tab.png` is written, then the tab is closed |
| `settings` (Linux so far) | the Settings pages and the home button | a fresh profile's engine blocks pop-ups, scrolls smoothly and uses the GPU; turning on the home button shows it in the toolbar (`home-button.png`) and clicking it with the homepage set to `/page2.html` loads that page in the selected tab; Settings opens on each of its General, Appearance, Search, Privacy and Shortcuts pages (`settings-<page>.png`); the home button and homepage are then restored |
| `shortcuts` | the keymap drives the shortcuts, and reassigning them in Settings applies at once | Linux: through the path Settings uses, assigning Ctrl+Shift+Y to History gives `win.show-history` that accelerator and leaves Ctrl+H on no action; assigning Ctrl+J to History takes it from Downloads, which is left with none, and the Shortcuts page shows Ctrl+J on History and "Disabled" on Downloads (`shortcuts-edited.png`); the capture dialog, given Ctrl+T, says "Also used by New tab. Saving moves it here." (`shortcut-capture.png`). Windows: the Keyboard shortcuts page lists every command the shell implements (`settings-shortcuts.png`); capturing for History turns the window's accelerators off while open, notes the same for Ctrl+T (`shortcut-capture.png`) and saves Ctrl+Shift+Y, which the window's accelerators then hold instead of Ctrl+H; in the already-loaded fixture page Ctrl+Shift+Y opens History and Ctrl+H no longer does, without a reload; Ctrl+R given to New tab opens a tab from the page without reloading it; Reload moved to Ctrl+Shift+E leaves F5 prevented before the page sees it, the page cannot see the key sets, and Ctrl+Shift+E reloads. Both: Reset All brings back every default, including Ctrl+Shift+S on Save Page As and Ctrl+S on the tab list |
| `save_page` | Save Page As writes an HTML page as MHTML | Linux: the save step `win.save-page` runs after its file dialog writes `<out_dir>/saved-page.mhtml`. Windows: Ctrl+Shift+S typed into the fixture page, with `SaveAsUIShowing` suppressing the dialog and choosing a single file at `<out_dir>/saved-page.mhtml`, writes it, and WebView2's own download flyout is closed afterwards. Both: the file contains `Vsesvit fixture` |
| `site_permissions` (Linux so far) | the permission prompt, stored settings, site info and Settings | a location request from the fixture page shows the prompt bubble on the site-info icon (`permission-prompt.png`); "Allow while visiting the site" stores Allow and the page's `permissions.query` reads `granted`, also after a reload, when a new request prompts no more; choosing Block in the site-info popover's Location row stores Block (`site-info-permissions.png`) and, after a reload, the next request fails with PERMISSION_DENIED without a prompt; Settings' Site Permissions page lists Location as Block (`settings-site-permissions.png`), and its Remove button goes back to Ask, leaving the empty state "Sites you allow or block show here." (`settings-site-permissions-empty.png`) |
| `capture_in_use` (Linux so far) | capture indicators and stopping capture | with WebKit's mock capture devices, `getUserMedia({video, audio})` shows one prompt "Use your camera and microphone?" with the four answers in core's order; "Allow this time" gives the page a live audio and a live video track, the address bar's in-use button reads "Using your camera and microphone" and the tab shows the camera icon with that tooltip, in the sidebar and the top strip (`capture-in-use.png`, `capture-in-use-top.png`); site info opened from that button shows both as "Allowed this time" with Stop (`site-info-capture.png`); Block on Camera stores Block and stops the camera only; Stop on Microphone ends the rest: both tracks end and the indicators go. The check restores the tab position and the mock-device setting and resets the site even when it fails |
| `welcome` (Linux so far) | the first-run welcome, opened directly (the self-test itself never shows it) | stepping through every page with its Next button, and once Back and Next again: Search lists one checked row per engine, with the default checked, and choosing another makes it core's default; Import shows an Import button per browser `installed_browsers()` finds (or "No other browsers found") and Choose File…; Extensions shows each of `RECOMMENDED_EXTENSIONS` with Install or Installed; Default Browser shows a status; Start Browsing closes it and sets `onboarding.done`. Each page is captured in light and dark (`welcome-<page>.png`, `welcome-<page>-dark.png`); the engine and theme are restored |
| `screenshot` | the window really rendered | `window.png` is written, from an in-app capture that does not steal focus, and is not a single flat color |
| `cws_install` (only with `--network`) | real Chrome Web Store install | install `ddkjiahejlhfcafbddmgiahcphecmpfh` (uBlock Origin Lite); verification == `ChromeWebStore { publisher_verified: true }`; the engine loads it |

## report.json

```json
{
  "platform": "windows | linux",
  "ok": true,
  "checks": [ { "name": "navigate", "ok": true, "ms": 812, "detail": "title=Vsesvit fixture" } ]
}
```
