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
| `tab_layout` | vertical tabs default, and the setting | a fresh profile shows the tab sidebar on the left; setting `tabs.position` to `right` moves it to the right and `top` shows the horizontal strip, each confirmed from widget geometry (sidebar x-position relative to the web view), then restored to `left` |
| `popup` | action popup page | opening the probe's action popup shows a document whose title is `visits=N` with N >= 1 |
| `omnibox` | input classification | `resolve("vsesvit fixture")` is a search on the default engine; `resolve("127.0.0.1:<port>/page2.html")` is that http URL |
| `session` | session persistence | after `session().save`, `restore()` returns at least one window with at least one tab |
| `new_tab_page` | the new tab page | a new tab shows the page's search box and a tile linking to the fixture server's origin, while still reading as blank (`about:blank`, the shell's "New tab" title); `new-tab.png` is written, then the tab is closed |
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
