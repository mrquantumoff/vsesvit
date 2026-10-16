# Behavior inventory: address bar, omnibox, site info, permissions, cookies, tracking protection, content blockers

Scope: the Linux GTK shell's
`crates/vsesvit-gtk/src/{address_bar.rs, omnibox.rs, site_info.rs, permissions.rs, cookies.rs, trackers.rs, blocker.rs}`,
plus the code in other GTK files that drives them (`window/mod.rs`, `window/actions.rs`, `browser.rs`, `tab.rs`,
`app.rs`, `lib.rs`, `dialogs/settings.rs`, `dialogs/site_data.rs`, `self_test.rs`) and the `vsesvit_core` modules they
call (`address`, `search`, `suggest`, `history`, `certificate`, `permissions`, `cookies`, `trackers`, `private`,
`prefs::keys`, `zoom`, `sync::Changed`). Read from the worktree `/home/dy/git/vsesvit-cef` on branch
`linux-cef-gpui` at commit `27896e0`.

This document describes behavior only. It proposes no architecture.

## Legend

- **(KEEP)**: product behavior the new shell must reproduce.
- **(WEBKIT-ONLY)**: a WebKitGTK workaround or limitation. It goes away with Chromium, and the code behind it should not be ported.
- **(NEEDS-CEF)**: needs a CEF equivalent. The likely CEF API is named. Every CEF type and function named below is checked to exist in `cef-dll-sys 154.5.0+154.0.34` (`src/bindings/x86_64_unknown_linux_gnu.rs`) or `cef 154.5.0+154.0.34` (`Impl*` traits). What a CEF call does at runtime beyond its doc comment is marked *(inference)*.
- **Verified** means I read the code. **Inference** means it is my reading of how Chromium/CEF behaves, and must be checked before relying on it.

Core types are written with their full paths the first time they appear. `Profile` is `vsesvit_core::Profile`. It is `!Send` and lives on the UI thread (`docs/PLAN.md`, "Threading model").

---

## 1. The address bar (`address_bar.rs`, `AddressBar`)

### 1.1 Layout and resting look (KEEP)

The address bar sits in the header bar's title slot, inside an `adw::Clamp` (`window/mod.rs::build_ui`). Its parts:

- **The text field.** It has the CSS class `address-entry` and fills the width (hexpand).
  - Placeholder text: **"Enter address"**.
  - Accessible label: **"Address"**.
  - Input purpose is URL, with spell checking off (`InputHints::NO_SPELLCHECK`).
- **Text alignment.** The text is **centered** (`xalign 0.5`) while the bar rests. It is **left-aligned** (`0.0`) while the field has keyboard focus or the user has unsubmitted text (`align_text`).
- **Security icon.** At the field's start, as the primary icon. It is hidden while the user edits. Clicking it runs `win.show-site-info`. See §1.3.
- **In-use button.** Placed just after the security icon, 2 px apart (`ICON_GAP`), vertically centered and mirrored in RTL (`after_security`).
  - It is a flat button with the class `address-in-use`.
  - It is visible only while the selected tab captures camera, microphone or screen, and the user is not editing.
  - Its icon and tooltip come from `permissions::indicator` (§4.9). Clicking it runs `win.show-site-info`.
  - While shown, the text's start margin grows by the button width plus 2 px, so text never runs under it.
- **Zoom chip.** Placed just before the star, 2 px apart (`before_star`, mirrored in RTL).
  - It is a flat pill with the class `address-zoom`. CSS: `min-height: 20px; padding: 0 6px; font-size: 0.85em; border-radius: 10px`.
  - It is visible only when the zoom level differs from 1.0 by more than 0.001. Its label reads like "110%" (`zoom::percent`, `format!("{:.0}%", level*100)`), with the tooltip **"Zoom"**.
  - The text's end margin grows to make room for it.
  - Clicking it opens the zoom bubble (§1.11).
- **Bookmark star.** At the field's end, as the secondary icon. It is always shown, even on blank pages.
  - Not bookmarked: `non-starred-symbolic`, tooltip **"Bookmark This Page"**.
  - Bookmarked: `starred-symbolic`, tooltip **"Edit Bookmark"**.
  - Clicking it runs `win.bookmark-page` (§1.11).
- **Load progress line.** A line inside the field's bottom edge. CSS: `margin: 0 10px 1px`, 2 px high, no border or shadow, with a `linear-gradient(to right, transparent → @window_fg_color)`. See §1.10.
- **Compact width.** With the synced pref `address_bar.compact` (`keys::COMPACT_ADDRESS_BAR`, default **true**), the bar is at most **720 px** wide (`COMPACT_ADDRESS_WIDTH`) and centered. Otherwise it fills the space between the header's buttons (`set_compact_address_bar`). Settings > Search > Address Bar > "Compact Address Bar" ("A narrow address bar in the middle of the toolbar") toggles it.
- **Private windows.** The header shows `user-not-tracked-symbolic` (tooltip "Private browsing") before the address bar, and has dark header bars (CSS `window.private`). This is window chrome, listed here because it sits next to the bar.

### 1.2 What URL text shows (KEEP, with one WEBKIT-ONLY piece)

- **Input.** The window pushes the selected tab's **committed URI** with `show_uri` and `restore`, then derives two strings (`Shown::of`):
  - `full`: `tab::display_uri(uri)`. This is `""` for `about:blank`; otherwise it is `webkit::functions::uri_for_display(uri)`, which is WebKit's decoding of punycode and percent escapes. **(WEBKIT-ONLY.)** Windows uses core's `vsesvit_core::address::readable_url(display_url(url))`, and the new shell should do the same. `readable_url` decodes only safe punycode labels (script-mixing and homograph checks in `safe_label`) and only visible non-ASCII escapes.
  - `simplified`: `vsesvit_core::address::simplified_url(&full)`.
    - It strips `https://`, a leading `www.` (only if another dot remains: `www.com` stays) and a lone trailing `/`.
    - It keeps `http://` in full, so an insecure page never reads like a secure one.
    - Other schemes show in full, as does an authority that holds `@`.
    - Examples, from the unit tests: `https://www.example.com/` reads `example.com`; `https://uk.wikipedia.org/wiki/Київ` reads `uk.wikipedia.org/wiki/Київ`.
- **Resting text (`show_resting`):**
  - When the field has focus **or** the synced pref `address_bar.full_urls` (`keys::SHOW_FULL_URLS`, default false) is on, the field shows `full`.
  - Otherwise it shows `simplified`.
  - Settings > Search > Address Bar > "Always Show Full URLs" ("Otherwise https:// and www. show only while editing the address") sets the pref, and every window follows at once (`Browser::set_full_urls`).
- **Focus in** (`focus_changed(true)`), when not editing: the field shows `full`, then selects all on idle (after the focusing click has placed the caret).
- **Focus out:** closes the suggestion list, cancels search-suggestion queries, and shows `simplified` again (unless editing). **Text the user typed stays when focus leaves.**
- **Programmatic writes don't count as edits.** All of them go through `set_text_quietly` (the `updating` flag), so they never look like user edits.
- **Committed URI.** The URI is the committed one, not the one loading. Error pages are loaded *at the failed URI* (`load_alternate_html(html, uri)`), so the bar shows the address that failed. A new tab page is HTML loaded at `about:blank`, so the bar is empty with the placeholder.

### 1.3 Security state (KEEP; its inputs NEEDS-CEF)

`address_bar::Security::of(uri)`, from the committed URI's scheme:

| URI | `Security` | Icon | Tooltip |
|---|---|---|---|
| `https:` | `Secure` | `channel-secure-symbolic` | "Secure connection" |
| `http:` | `Insecure` | `channel-insecure-symbolic` | "This connection is not secure" |
| any other scheme except `about:blank` (`file:`, `about:version`, `data:`, `view-source:`, `vsesvit:`) | `Internal` | `dialog-information-symbolic` | "View site information" |
| `about:blank` or no URI | `NotApplicable` | none | none |

The tab can override this (`Tab::security`):

- An error page of kind `Certificate` or `HttpsOnly` makes the tab **Insecure**.
- `Other` (load failure, crash) makes it **NotApplicable**.

HTTPS pages with certificate errors are never shown, because the engine session fails TLS errors (`engine.rs`: `set_tls_errors_policy(Fail)`; `tab.rs` `load-failed-with-tls-errors` shows `error_page::tls_error`, titled "This connection is not secure"). So every displayed HTTPS page counts as Secure.

- **NEEDS-CEF.** `CefRequestHandler::OnCertificateError` (`ImplRequestHandler::on_certificate_error`): cancel the load, then show the shell's own error page at the URL.
- **Optional with Chromium:** `CefSSLStatus::GetContentStatus` (`ImplSslstatus::content_status`) reports mixed content, which Linux does not show today.
- While the user edits, the icon is hidden (`show_security` treats editing as `NotApplicable`), and so is the in-use button.

### 1.4 The edit model (KEEP)

`Edit { typed, inline, rows, highlighted }` exists from the user's first change until they submit, cancel or choose a row.

- **`text_changed`** runs on every user change.
  - Starting an edit hides the security icon and the in-use button, and left-aligns the text.
  - Each change sets `typed` to the new text, clears `inline`, sets `highlighted` to 0, selects row 0 in the list, and emits **`edited(text, inline_allowed)`**.
- **`inline_allowed`** is `!deleted && caret_at_end`.
  - `deleted` is set by the field's delete-text and cleared by insert-text.
  - A replacement (select, then type) deletes and then inserts, so it *is* allowed.
  - Backspace, Delete and cut get no inline completion.
  - Typing with the caret before the end gets none either.
  - From the unit test: typing `git` completes to `github.com`; one Backspace removes only the completion (shows `git`); a second shows `gi`; typing `t` again completes again.
- **`Edit::shown()`** gives what the field reads:
  - Row 0 highlighted with an inline completion: `typed + inline`, with the inline part selected (selection from `char_count(typed)` to the end).
  - Row 0 without inline: `typed`.
  - Any other row: that row's `fill`, with the caret at the end.
- **`move_highlight(delta)`** clamps at the first and last row. It does not wrap.
- **`accept_inline()`** (Right or End while row 0 is highlighted) appends `inline` to `typed`, so the completion becomes typed text.
- **Signals the bar emits:**
  - `edited(String, bool)`
  - `submitted(String)`
  - `cancelled`
  - `bubble-changed`, emitted when a bubble opens from the bar or closes.
- **Window wiring** (`window/mod.rs::connect_signals`):

  | Signal | Handler |
  |---|---|
  | `edited` | `Browser::omnibox_changed` |
  | `submitted` | `Browser::omnibox_activated` |
  | `cancelled` | `BrowserWindow::focus_page` (the web view takes focus) |
  | `bubble-changed` | `BrowserWindow::sync_permission_prompt` |

### 1.5 Keyboard (KEEP)

The key handler runs in the capture phase on the field. "Open" means the list popover is visible and has at least one row.

| Key | Condition | Effect |
|---|---|---|
| Enter / KP_Enter / ISO_Enter with **Ctrl** | any | `submit_as_com`: if the typed text is one bare word of `[A-Za-z0-9-]`, `vsesvit_core::search::ctrl_enter_url` gives `https://www.<word>.com/`, which is emitted as `submitted`. Otherwise it acts as Enter. |
| Enter | open | Activates the highlighted row (`choose`) |
| Enter | not open | Emits `submitted(field text)` |
| Down / KP_Down | open | Highlight +1 (clamped). The field shows the row's `fill`. **No new query.** |
| Up / KP_Up | open | Highlight −1 (clamped). Back on row 0, the typed text plus the selected completion return. |
| Tab | open, no Ctrl/Alt | Highlight +1 |
| Shift+Tab / ISO_Left_Tab | open, no Ctrl/Alt | Highlight −1 |
| Up / Down | not open | Swallowed: **focus never leaves the address bar** |
| Escape | open and highlight ≠ 0 | Back to row 0, showing the typed text plus the selected completion |
| Escape | otherwise | Cancel: end the edit, show the resting URI, emit `cancelled`. The page takes focus. |
| Shift+Delete / Shift+KP_Delete | open | If the highlighted row has `forget` (history rows only), delete every visit to that URL, then query again for the same typed text (`query_again` emits `edited(typed, inline.is_some())`). Otherwise nothing happens. |
| Right / KP_Right / End / KP_End without Shift | row 0 highlighted | `accept_inline`, then default handling moves the caret past the completion |

Shortcuts that focus the bar (`win.focus-location`): **Ctrl+L, Alt+D, F6** (`vsesvit_core::shortcuts`, `FocusAddress`). They go through `focus_for_typing`, which grabs focus and selects all. Ctrl+D runs `win.bookmark-page`. Zoom keys: Ctrl+Plus/Equal/KP+, Ctrl+Minus/KP−, Ctrl+0/KP0. The keymap is reassignable, as are all shortcuts.

### 1.6 Mouse (KEEP)

- Clicking into the field focuses it, shows the full URI and selects all.
- A single click on a suggestion row activates it (`activate_on_single_click`). The list takes no focus: rows are `focusable(false)`, and neither the list nor the popover takes focus.
- The security icon, in-use button, zoom chip and star act as described in §1.1.

### 1.7 The suggestion list (KEEP)

- **Popover.** Positioned below the bar, with no arrow and **no autohide**. It never takes focus, and has the class `address-suggestions`.
  - Its width is **the bar's width**, set on every open. The self-test asserts that the list is at least as wide as the bar.
  - Its content scrolls vertically: no horizontal scrollbar, natural height, **max 440 px**.
- **When it opens and closes (`show_list`).** It opens only while the field has focus **and** the current edit has at least one row. Otherwise it closes. It also closes on focus out, on submit, cancel or choose, and on a tab switch.
- **Row layout (`suggestion_row`).** Each row is a horizontal box with 12 px spacing and margins of 6 px top/bottom and 8 px left/right. It holds:
  - the icon (`icon_name`);
  - a vertical box with the **title** (left-aligned, ellipsized at the end) and, under it, the **subtitle** (dim-label and caption style, ellipsized in the middle).
- **The highlighted row** is the list's selected row (single selection).
- **`set_suggestions(Suggestions{rows, inline})`** replaces the rows for a new typed text. Highlight goes to 0, the field shows typed text plus the selected completion, and the list opens or closes. It does nothing when no edit is in progress.
- **`refill_suggestions(rows)`** is used when the engine's suggestions arrive for the *same* typed text.
  - It keeps the highlighted row highlighted, found by equal `fill`, at its new index. If that row is gone, row 0 is highlighted.
  - It leaves the field's text, inline completion and caret untouched.
  - It never triggers a query.
  - The unit test `a_refill_keeps_the_highlighted_row_and_the_entry_as_they_are` pins all of this.
- **Each row's contract:** `{title, subtitle, icon_name, fill, activate: Fn(), forget: Option<Fn()>}`. See §2 for how rows are built.

### 1.8 Submission paths (KEEP)

- **`submit`** (Enter): if rows exist and the popover is visible, run `choose(highlighted)`. Otherwise end the edit and emit `submitted(field text)`.
  - After a plain submit the field **keeps showing the submitted text**. `submit` does not call `show_resting`. The text stays until the tab commits (`sync_location` on commit) or the load ends without committing (`TabChange::Loading` with `!loading`, which calls `sync_location`: "A load that ends without committing (a download, a stop) leaves the submitted text behind; show the page that is actually there again").
- **`choose(i)`**: end the edit, **show the resting URI of the current page**, then run the row's `activate`.
- **`Browser::omnibox_activated(window, text)`** calls `Profile::omnibox().resolve(text)`:
  - `Ok(Some(target))` calls `window.navigate_with(target.url(), Transition::Typed)`.
  - `Ok(None)` (blank text) calls `window.focus_page()`.
  - `Err(e)` shows a toast: **"Cannot resolve the address: {e}"**.
- **`navigate_with`** picks the selected tab (or opens one if the window has none), sets `pending_transition`, calls `tab.load(uri)` (a browser-initiated load that passes the navigation gate), and **gives the web view focus**.

### 1.9 Per-tab state across tab switches (KEEP)

`BrowserWindow::selection_changed`:

1. The previous tab gets `set_typed(address.take_edit())`, saving its unsubmitted text. This is held in memory on `Tab` only, **not** in the session file.
2. `address.clear_progress()` runs.
3. The new tab's `take_typed()` is passed to `address.restore(typed, committed_uri)`. This cancels queries and either restores an edit with that text (no rows, list closed, nothing queried until the user types) or shows the resting URI. Security and alignment are refreshed.
4. Location, security, zoom, star, the in-use button and loading are re-synced.
5. If the tab had typed text **or is blank** (`Tab::is_blank`: no URI or `about:blank`, and not asleep), the bar takes focus with everything selected (`focus_for_typing`). Otherwise the web view takes focus. A new tab (`win.new-tab`) therefore focuses the bar.
6. The permission prompt is re-synced (§4.4).

The home button with homepage `about:home` loads the new tab page and focuses the bar.

### 1.10 Load progress line (KEEP; input NEEDS-CEF)

- **`set_progress(fraction)`** shows `max(fraction, currently shown, 0.08)` (`PROGRESS_START`), so a navigation shows at once and the line never moves back during one load. If a hide was pending, it is cancelled and the line restarts from 0.08.
- **`finish_progress()`** (load ended): the line fills to 1.0, stays **200 ms** (`PROGRESS_HOLD`), then drops to 0. It does nothing if the line is already 0 or a hide is pending.
- **`clear_progress()`**: drops to 0 at once, as on a tab switch.
- **Driven by** `sync_loading`. While loading, `set_progress(web_view.estimated_load_progress())`; on stop, `finish_progress()`. The reload button flips between Stop (`process-stop-symbolic`, "Stop", `win.stop`) and Reload (`view-refresh-symbolic`, "Reload", `win.reload`).
- **NEEDS-CEF.** `CefDisplayHandler::OnLoadingProgressChange` (`on_loading_progress_change`, a value from 0 to 1) and `CefLoadHandler::OnLoadingStateChange`.
- **Self-test `address_progress`.** A data: page whose image stalls shows the line strictly between 0 and 1 while loading, and 0 after the load ends.

### 1.11 Bubbles from the address bar (KEEP)

- **One at a time.** At most one "bubble" is open from the bar: site info, zoom or the bookmark star. `show_popover(popover, anchor)` closes any previous bubble, points the new one at its anchor's rectangle (security icon area, zoom chip, star icon area) with position Bottom, and emits `bubble-changed` on open and on close. The bubble is parented to the bar and released when it closes or the bar is unrealized (`crate::popup`).
- **The permission prompt is a separate slot** (`show_prompt`), also anchored on the security icon. Opening the prompt leaves bubbles alone. The window shows the prompt only while **no** bubble is open (§4.4).
- **Zoom bubble** (`show_zoom_bubble`, class `zoom-bubble`). It holds, in one row with 8 px spacing:
  - the label **"Zoom:"**;
  - the level (for example "110%"), 5 characters wide in numeric heading style, **updated live** while open (`zoom_label`);
  - linked buttons Zoom Out (`zoom-out-symbolic`, tooltip "Zoom Out", `win.zoom-out`) and Zoom In (`zoom-in-symbolic`, "Zoom In", `win.zoom-in`);
  - **"Reset"** (tooltip "Reset to 100%", `win.zoom-reset`).

  Zoom steps follow Chrome's presets (`zoom.rs::LEVELS`: 25%…500%), and per-site zoom is in core (`Profile::site_zoom`). Both belong to the zoom inventory. **NEEDS-CEF:** `CefBrowserHost::SetZoomLevel` and `GetZoomLevel`. CEF levels are logarithmic (factor = 1.2^level) *(inference: Chromium convention)*, so the chip's percent must be converted.
- **Star / bookmark bubble.** `win.bookmark-page` calls `Browser::star_clicked`:
  - It does nothing without a parseable committed URL.
  - If the page is already bookmarked (`bookmarks().find_by_url`), it opens **"Edit bookmark"** on that bookmark.
  - Otherwise it adds the page to the **bookmarks bar** (`BookmarkId::TOOLBAR`, `InsertAt::End`, using the tab's display title), saves its favicon, refreshes the bookmark views, and opens **"Bookmark added"**.
  - The bubble (`bookmark_editor::bubble`) has a close button, a name/folder form, **"_Remove"** (removes every bookmark of the URL) and **"_Done"** (suggested action, also Enter). Closing it any other way keeps the bookmark as saved.
  - On error, a toast says "Cannot bookmark the page: {e}".
  - The star's state follows `Browser::is_bookmarked(committed_uri)` (`sync_star`, run on commit, on tab switch and on bookmark changes). The `bookmark-page` action carries the boolean state too.
  - Details are in the bookmarks inventory.
- **Site info:** see §3.

### 1.12 Test and self-test hooks the shell exposes (KEEP as capabilities)

The unit tests and `self_test.rs` drive the bar through:

- `type_text`: deletes the selection, then inserts at the cursor, as two changes like the keyboard makes.
- `press(key, mods)`: a key, ending in field activation for Enter.
- `observe()`, which returns `Observed { text, selection, highlighted, fills, open }`.
- `submit_text`: makes an edit, then submits it.
- `focus_for_typing`.
- `suggestions_popover`.
- `click_security`, `click_in_use`, `click_zoom`.
- `shown_zoom`, `shown_in_use`.
- `prompt()`, `bubble()`, `progress()`.

Popovers open without autohide under the self-test (`SCRIPTED`; `lib.rs::popup`), because a Wayland popup grab is refused without real input. **(WEBKIT-ONLY / GTK-only detail.)** The new shell needs an equivalent observation surface for the self-test checks in §9.

### 1.13 Core APIs and state used by the bar

- `vsesvit_core::address::simplified_url(shown: &str) -> String`.
- `vsesvit_core::address::readable_url(url) -> String`. Not used on Linux today; used by Windows.
- `vsesvit_core::search::ctrl_enter_url(text) -> Option<Url>`.
- `vsesvit_core::suggest::Queries`: one per bar. `cancel()` makes every query asked so far stale. It is called when the edit ends, focus leaves, or another tab's state is restored.
- `vsesvit_core::zoom::DEFAULT` (1.0).
- Prefs (all synced; defaults in parentheses):
  - `address_bar.full_urls` (false)
  - `address_bar.compact` (true)
  - `address_bar.suggest.search` (true)
  - `address_bar.suggest.history` (true)
  - `address_bar.suggest.bookmarks` (true)
  - `search.default` (built-in default)
- Sync: a remote pref change calls `BrowserWindow::apply_prefs`, which sets compact width and full URLs live (`Browser::sync_applied`).

---

## 2. Omnibox suggestions and resolution (`omnibox.rs`, `Browser::omnibox_changed` / `omnibox_activated`)

### 2.1 Flow on every edit (KEEP)

`Browser::omnibox_changed(window, text, allow_inline)` runs these steps:

1. `address.set_suggestions(omnibox::suggestions(browser, window, text, allow_inline, None))` sets the local rows synchronously.
2. `Profile::omnibox().suggest_request(text, address.queries(), private)` is called with `private = window.browsing() == Browsing::Private`. This **always** makes this query the newest, so every earlier one goes stale. It returns `Ok(None)` when nothing may be sent (see §2.4).
3. If a request is returned, it runs `SuggestRequest::run()` on a worker thread (`gio::spawn_blocking`). `run` blocks for the 100 ms debounce, sends nothing if a newer query was made meanwhile, fetches with a 5 s limit, and returns `Option<SearchSuggestions>`.
4. Back on the UI thread, a result is dropped unless `found.is_current()` and the window still exists. Otherwise it recomputes `omnibox::suggestions(..., Some(&found)).rows` and calls `address.refill_suggestions(rows)`.

Self-test `search_suggestions`: typing `vsesvit` makes **exactly one** `/suggest` request.

### 2.2 Building rows (`omnibox::suggestions`) (KEEP)

- A trimmed empty text gives no rows and no inline completion.
- Core rows come from `Profile::omnibox().suggest(needle, 8 /*MAX_CORE_SUGGESTIONS*/, allow_inline)`. An error is logged and treated as no rows. If engine suggestions arrived, `Suggestions::add_search_suggestions(found)` runs.
- Each core `vsesvit_core::search::Suggestion { source, title, target: NavTarget, fill }` becomes a row:

  | `source` / `target` | icon | subtitle | activation transition | `forget` |
  |---|---|---|---|---|
  | `Search` with `NavTarget::Search`, or `SuggestedSearch` | `system-search-symbolic` | **"Search"** | `Transition::Typed` | none |
  | `Search` with `NavTarget::Url`, or `Typed` | `web-browser-symbolic` | `display_uri(url)` | `Typed` | none |
  | `Bookmark` | `user-bookmarks-symbolic` | `display_uri(url)` | `Transition::Bookmark` | none |
  | `History` | `document-open-recent-symbolic` | `display_uri(url)` | `Typed` | `history().delete_url(url)` (Shift+Delete) |

  Activation calls `window.navigate_with(url, transition)`. The title is core's title. `fill` is core's `fill`.
- **Open-tab rows** are appended after the core rows (`open_tab_suggestions`), at most 4 (`MAX_TAB_SUGGESTIONS`).
  - They cover tabs in **all windows** of the **same `Browsing` kind** as the current window, excluding the current window's selected tab, and only tabs that have a committed URI.
  - A tab matches when its lowercase display title or committed URI contains the lowercase trimmed needle.
  - Row contents: title = tab's display title; subtitle **"Switch to tab · {display_uri}"**; icon `view-paged-symbolic`; fill = `display_uri`.
  - Activation selects that tab in its window and presents the window.
- The inline completion is core's `Suggestions.inline`.
- Maximum rows: up to 8 core rows plus up to 4 engine suggestions inserted by `add_search_suggestions` (it does not re-apply the limit), plus up to 4 tab rows.

### 2.3 Core omnibox contract (verified in `vsesvit-core/src/search.rs`, `suggest.rs`)

- **`Omnibox::resolve(text) -> Result<Option<NavTarget>>`** runs `classify` against the engine list and the default engine (`search.default`, falling back to the first live built-in, then to any engine). Rules (`classify`):
  1. Trim; empty gives `None`.
  2. A URL with a navigable scheme (`http`, `https`, `file`, `about`, `data`, `view-source`, `vsesvit`; http(s) need a host) gives `Url`. An absolute file path (`/…`, `C:\…`, `\\server\share`) gives a `file:` URL.
  3. No whitespace and host-shaped (`localhost`, an IP literal, dotted labels ending in an alphabetic TLD, IDN included, optional `:port` and `/path`) gives `https://`+text, or `http://` for localhost and IP literals.
  4. A first word equal to an engine keyword, followed by more text, searches that engine.
  5. Anything else searches the default engine.

  The engine list is read on every call, so a new keyword works at once.
- **`Omnibox::suggest(text, limit, allow_inline) -> Result<Suggestions>`**:
  - **Typed row.** For a URL it is `Typed` (title = full URL, fill = `fill(url, typed)`). For a search it is `Search`, titled **`Search {engine name} for "{terms}"`** with fill = typed text.
  - **Candidates.** Bookmarks come from `bookmarks().search(typed, limit)`, unless `address_bar.suggest.bookmarks` is off. History comes from `history().search(typed, limit)`, unless `address_bar.suggest.history` is off; an empty history title becomes the URL. Candidates are deduplicated by URL, URL-prefix matches sort first (stable sort, bookmarks ahead of history), and the result is cut to `limit`.
  - **Inline completion** (`completion`), only with `allow_inline` and no whitespace in the text:
    - It starts from the best URL-prefix candidate.
    - While the typed text is still a host prefix (no `/` after the scheme), the candidate is widened to its origin root. If that root is neither a bookmark nor a history row, it becomes a synthetic `Typed` row.
    - `inline` is the case-insensitive remainder of the row's `fill` after the typed text.
    - The completion row becomes `items[0]`, ahead of the typed row.
    - A ranked row for the typed URL takes the typed row's place.
  - **`fill(url, typed)`** omits `http(s)://` and a leading `www.` unless the typed text has them, and drops the trailing `/` of a bare origin.
- **`Suggestions::add_search_suggestions(found)`** inserts the engine's rows after the leading search rows and above the URL rows. It skips rows whose target is already listed (such as the typed text's own search). `items[0]` and `inline` stay untouched.
- **`Omnibox::suggest_request(text, queries, private)`** returns `None` (nothing sent) in any of these cases:
  - `private` is true;
  - `address_bar.suggest.search` is off;
  - there is no default engine;
  - the default engine has no `suggest_url`, or it does not expand to http(s);
  - the text is not a **plain search of the default engine**: a URL, a file path, or a keyword search sends nothing.

  Engines added in the editor have no suggest URL, so only built-ins suggest.
- **`SuggestRequest::run`**:
  - waits `DEBOUNCE` (100 ms);
  - uses a shared `ureq` agent with a 5 s global timeout and core's `USER_AGENT`;
  - caps the body at 64 KiB;
  - decodes Latin-1 when the charset says so;
  - parses OpenSearch JSON `[query, [texts…], …]`, trimming, skipping blanks and duplicates, keeping at most 4;
  - logs at debug level only the host;
  - treats a failure as no suggestions.

  Engine rows have source `SuggestedSearch`, title = fill = text, and target = the default engine's search URL for that text.
- **`History::delete_url(url)`** writes a synced deletion directive for every visit to the URL.

### 2.4 Persistent state

The edit itself stores nothing. Searches and visits are recorded by navigation (history inventory). `Transition::Typed` or `Bookmark` on the visit comes from `navigate_with`. Private windows write no history (private-browsing inventory).

### 2.5 Classification

All of §2 is **(KEEP)**. It is engine-independent, except that it calls `display_uri` (WEBKIT-ONLY; use `readable_url`). The worker thread for `SuggestRequest::run` is any thread; the result must come back to the UI thread.

---

## 3. Site information popover (`site_info.rs`, `BrowserWindow::show_site_info`)

### 3.1 Opening (KEEP)

`win.show-site-info` opens it, triggered by a click on the security icon or on the in-use button. There is no default shortcut. `show_site_info`:

1. Builds sections in this order. Each is optional:
   1. tracking protection (`trackers::site_info_section`, §6.3);
   2. cookies (`cookies::site_info_section`, §5.3);
   3. permissions (`permissions::site_info_section`, §4.10).
2. Builds `site_info::popover(&Connection::of(&tab), &sections)` and shows it with `show_popover(..., Anchor::Security)`. Opening it **withdraws** any visible permission prompt (§4.4).

### 3.2 Content (KEEP; wording matters)

- **Container.** A vertical box with 8 px spacing, 8 px margins and **380 px** width, inside a vertical scroller (natural height, **max 680 px**). The popover has the class `site-info`.
- **Headline row:** an icon plus a title in heading style. The summary table is in `summary(connection)`:

  | Condition | Title | Icon | Explanation |
  |---|---|---|---|
  | `Secure` and the certificate has no problems | **Connection is secure** | `channel-secure-symbolic` | "What you send to this site, such as passwords or card numbers, is private." |
  | `Secure` with certificate problems, or `Insecure` | **Connection is not secure** | `channel-insecure-symbolic` | "Do not enter passwords or card numbers on this site: others on the network could read or change them." |
  | `Internal` | **This is a local or built-in page** | `dialog-information-symbolic` | "It was not loaded over the network." |
  | `NotApplicable` | **No connection details** | `dialog-information-symbolic` | "This page was not loaded from a site." |

- **Host line** (dim style): the committed URL's host, or else the whole URI. It is omitted when empty.
- **Explanation:** wrapped text, max 48 characters per line.
- **Sections.** Each section is preceded by a horizontal separator.
- **Certificate section**, only for `https` pages where the engine reported TLS info (preceded by a separator):
  - Heading **"Certificate"**.
  - If there are no problems: **"Valid: issued by a trusted authority for this site."** (style `success`). Otherwise one line per problem (style `error`). Problem texts come from the verification flags:

    | GIO flag | Text |
    |---|---|
    | UNKNOWN_CA | "It is not issued by an authority this device trusts." |
    | BAD_IDENTITY | "It is not valid for this site's name." |
    | NOT_ACTIVATED | "It is not valid yet." |
    | EXPIRED | "It has expired." |
    | REVOKED | "It has been revoked." |
    | INSECURE | "It uses an insecure algorithm." |
    | GENERIC_ERROR | "It could not be verified." |

  - If the leaf parses, a grid follows. Group captions use the `caption-heading` style, labels are dim, and values are selectable and wrap by word/char at 44 characters:
    - **Issued to** / **Issued by** (`name_rows`): "Common name", "Organization" and "Unit" when present; with none of them, "Name" = `Name::display()`, which reads "Unknown" when empty.
    - **Validity**: "Valid from" and "Valid until", as a local date formatted `%-d %b %Y` (for example `5 Mar 2026`).
    - **Details**: "Public key" (for example `RSA 2048 bits`, `ECDSA P-256`), "Signature" (for example `SHA-256 with RSA`), "Serial number" (colon hex).
  - Then **"SHA-256 fingerprint"** (caption) and a monospace selectable value: 95 characters, uppercase colon hex.
  - Then an expander **"Subject alternative names (N)"**, collapsed, listing the DNS names. It is insensitive when N = 0.
  - If the leaf does not parse: **"The certificate could not be read: {e}"** (error style).
  - If the chain has more than one certificate: **"Certificate chain"** (caption), then each certificate's subject display (or "(unreadable: {e})"), indented 12 px per depth. The chain is cut at 10 certificates (`MAX_CHAIN`).
- **Rebuilt each time.** The popover is rebuilt on every open. It does not update live, except for the sections' own rows, which update themselves.

### 3.3 Engine inputs

- **(WEBKIT-ONLY).** `webkit::WebView::tls_info()` returns `(GTlsCertificate leaf, TlsCertificateFlags)`. The chain is walked with `GTlsCertificate::issuer`, and the DER comes from each certificate's `certificate` property.
- **(NEEDS-CEF).** `CefBrowserHost::GetVisibleNavigationEntry` → `CefNavigationEntry::GetSSLStatus` (`ImplNavigationEntry::sslstatus`) → `CefSSLStatus`:
  - `IsSecureConnection`, `GetCertStatus`, `GetSSLVersion`, `GetContentStatus`;
  - `GetX509Certificate` → `CefX509Certificate::GetDEREncoded` and `GetDEREncodedIssuerChain` (`ImplX509Certificate::derencoded`, `derencoded_issuer_chain`).

  Problem texts would map from `cef_cert_status_t`: `CERT_STATUS_AUTHORITY_INVALID`, `COMMON_NAME_INVALID`, `DATE_INVALID` (covers both not yet valid and expired; the dates distinguish them), `REVOKED`, `WEAK_SIGNATURE_ALGORITHM` / `WEAK_KEY`, `INVALID`, and others.

  An alternative is DevTools `Security.visibleSecurityStateChanged` through `CefBrowserHost::ExecuteDevToolsMethod` plus `AddDevToolsMessageObserver`. This is what Windows uses (`vsesvit-winui/src/connection.rs`), and it also gives the TLS protocol, key exchange and cipher.
- **(KEEP).** `vsesvit_core::certificate::parse_der(&[u8]) -> Result<Certificate, CertificateError>` produces `Certificate { subject, issuer: Name, not_before_unix, not_after_unix, dns_names, serial_hex, sha256_hex, public_key, signature_algorithm }`, with `Name { common_name, organization, organizational_unit, country }` and `Name::display()`.
- **Windows parity, not Linux behavior today.** Windows' popup also shows the TLS parameters and a "Show certificate" button, and does not call an HTTPS page secure without a security report. Chromium would make these possible on Linux. That is a product decision, not something to reproduce.

### 3.4 Self-test (`connection_info`)

On the HTTP fixture, the heading is "Connection is not secure" and there is no "Certificate" label (`connection-http.png`). With `--network`, `https://example.com/` reads "Connection is secure", with a 95-character fingerprint, a SAN expander and a "Certificate chain" label (`connection-https.png`).

---

## 4. Site permissions (`permissions.rs`, prompt logic in `window/mod.rs`)

### 4.1 The model (core, `vsesvit_core::permissions`) (KEEP, verified)

- **`Permission`**: `Camera`, `Microphone`, `Location`, `Notifications`, `ScreenShare`, `ClipboardRead`, `Midi`, `PictureInPicture`, `Trackers`, `Http`, `Cookies`.
  - `key()` gives the wire/SQL name.
  - `label()` gives the UI name: "Camera", "Microphone", "Location", "Notifications", "Screen sharing", "Clipboard", "MIDI devices", "Picture-in-picture", "Trackers", "Insecure connections", "Cookies and site data".
  - `asks()` is false for PiP, Trackers, Http and Cookies; with nothing stored, those **block**.
  - `in_site_info()` is false for Trackers, Http and Cookies.
  - `remembers_allow()` is false for ScreenShare only. A screen share remembers only Block, and "Allow this time" covers that one request.
  - `settings()` lists what can be stored: Http has [Allow]; Cookies has [Allow, Block, ClearOnExit]; ScreenShare has [Block]; the others have [Allow, Block].
- **`Setting`**: `Allow`, `Block`, `ClearOnExit` (labels "Allow", "Block", "Clear on exit"). `None` means Ask.
- **`Origin`**: the `scheme://host[:port]` ASCII serialization. Opaque origins (`file:`, `data:`, `about:`) have none, so nothing is stored for them and only one-time grants apply. `host_for_display()` gives the decoded host, plus the port when it is not the scheme default.
- **`SitePermissions`** comes from `Profile::site_permissions()` (stored/normal) or `Profile::site_permissions_in(Browsing)`:
  - `get(origin, p) -> Option<Setting>`: in private, the private session's value wins. A stored value that `p.settings()` does not allow reads as `None`.
  - `set(origin, p, Option<Setting>)`: no-op when unchanged. Storing Allow for a permission that is asked every time is `Error::AlwaysAsks`.
  - `for_site`, `all`, `by_site` (Settings' grouping, with the heading as host, or the full origin when two sites share a host).
  - `reset_site(origin)`: every permission back to Ask **except Trackers and Cookies**.
  - `decide(origin, permissions, &TabGrants) -> Decision`:
    - Any Block wins.
    - A never-asked permission with nothing stored blocks.
    - Everything allowed (stored or granted) means Allow.
    - Otherwise `Ask(undecided)`.
    - **In private, Notifications always Block.**
  - `answer(origin, permissions, Answer, &mut TabGrants) -> Result<bool>`:
    - AllowWhileVisiting with an origin stores Allow.
    - AllowThisTime, or AllowWhileVisiting without an origin, grants for the tab.
    - NeverAllow with an origin stores Block and returns false.
    - Dismiss returns false.
  - Private-session writes go to `Profile.private.sites` (in memory) and are dropped by `Profile::end_private_session`.
- **`TabGrants`**: "Allow this time" grants, held per tab.
  - `allows(origin, p)`, `granted()`, `revoke(p)`.
  - `committed(url)` ends all grants when the origin changes. Opaque origins end on any commit.
  - A grant never includes ScreenShare.
- **`prompt(origin, permissions) -> Prompt { heading, body, answers }`**:
  - Heading: the capitalized request phrase plus "?", for example **"Know your location?"**, **"Use your camera and microphone?"**, **"Use your microphone?"**.
  - Body: **`“{host}” wants to {phrase}.`** (curly quotes), or "This page wants to …" without an origin.
  - Phrases are grouped by verb, in `Permission` enum order:

    | Permission | Phrase |
    |---|---|
    | Camera | "use your camera" |
    | Microphone | "use your microphone" |
    | Location | "know your location" |
    | Notifications | "show notifications" |
    | ScreenShare | "share your screen" |
    | ClipboardRead | "see text and images copied to the clipboard" |
    | Midi | "use your MIDI devices" |

  - Buttons, in order:
    1. **"Allow while visiting the site"**: only with an origin, and only if every asked permission remembers Allow.
    2. **"Allow this time"**.
    3. **"Never allow"**: only with an origin.
    4. **"Not now"** (`Answer::Dismiss`).
- **`Capturing { camera, microphone, screen }`**:
  - `any()`, `uses(p)`.
  - `description()` gives "Using your camera and microphone", "Using your camera", "Using your microphone", "Sharing your screen", or "Sharing your screen and using your {devices}".
- **`site_rows(storable, stored, granted, capturing) -> Vec<SiteRow { permission, current: SiteChoice, choices, live }>`** builds the site-info Permissions rows. Rows appear in `Permission::ALL` order, for permissions that are in site info and that the site has stored, was granted this time, or uses live. `ClearOnExit` stored gives no row.
  - `current` is Allow or Block from storage, or `AllowedThisTime` (only for asking permissions that are granted or live).
  - `choices`: `AllowedThisTime` (only if current), then Ask (if `asks`), then Allow (if storable and remembers Allow), then Block (if storable).
  - `SiteChoice` labels: "Allowed this time", "Ask", "Allow", "Block". `SiteChoice::setting()`: Allow and Block map to settings; AllowedThisTime and Ask map to `None`.
- **Persistent state.** The SQLite table `site_permissions(origin, permission, setting, setting_at, seq)` holds a synced LWW record per (origin, permission), as `Kind::SitePermissions`. A reset keeps the row with a NULL setting so the reset syncs.

### 4.2 Request intake (Linux today)

`tab.rs` connects these WebKit signals: `permission-request` → `permissions::handle`; `query-permission-state` → `permissions::query`; `show-notification` → veto (§4.8). Kinds and decisions (`kind_of`, `ask_for`):

| WebKit request | Decided as | Linux policy |
|---|---|---|
| `GeolocationPermissionRequest` | `[Location]` | settings, grants, else prompt |
| `NotificationPermissionRequest` | `[Notifications]` | settings, grants, else prompt (private: always Block) |
| `ClipboardPermissionRequest` | `[ClipboardRead]` | settings, grants, else prompt |
| `UserMediaPermissionRequest` with display | `[ScreenShare]` | settings, grants, else prompt (Allow never stored) |
| `UserMediaPermissionRequest` video and/or audio | `[Camera]`, `[Microphone]` or both | **one request, one prompt** for both; neither asked means deny |
| `DeviceInfoPermissionRequest` (labels for `enumerateDevices`) | — | **allowed iff** Camera or Microphone decides Allow; never prompts |
| `PointerLockPermissionRequest` | — | **always allowed**, without asking, as in other browsers (Esc releases) |
| anything else | — | **denied** |

- **The requesting document** is taken to be the **tab's committed URI**. **(WEBKIT-ONLY.)** WebKit requests carry no origin. During a provisional load, the web view's URI is already the target, which has not run anything, so the committed URI is used instead. A unit test pins this.
  - **NEEDS-CEF.** CEF passes `requesting_origin`.
  - **KEEP** the rule that an answer applies to the document on screen and is checked against the site still on screen (`holds`, §4.5).
- **NEEDS-CEF.** `CefPermissionHandler` (`ImplPermissionHandler`):
  - `OnShowPermissionPrompt(browser, prompt_id, requesting_origin, requested_permissions bitmask, CefPermissionPromptCallback)`, with `cef_permission_request_types_t`:
    - `CEF_PERMISSION_TYPE_GEOLOCATION` → Location
    - `NOTIFICATIONS` → Notifications
    - `CLIPBOARD` → ClipboardRead
    - `MIDI_SYSEX` → Midi
    - `POINTER_LOCK` → allow
    - `CAMERA_STREAM` / `MIC_STREAM` → Camera / Microphone
    - many others: `STORAGE_ACCESS`, `TOP_LEVEL_STORAGE_ACCESS`, `LOCAL_FONTS`, `IDLE_DETECTION`, `WINDOW_MANAGEMENT`, `FILE_SYSTEM_ACCESS`, `MULTIPLE_DOWNLOADS`, `REGISTER_PROTOCOL_HANDLER`, `KEYBOARD_LOCK`, `SENSORS`, `PROTECTED_MEDIA_IDENTIFIER`, `LOCAL_NETWORK`, `LOOPBACK_NETWORK`, `HAND_TRACKING`, `AR_SESSION`, `VR_SESSION`, `CAMERA_PAN_TILT_ZOOM`, `CAPTURED_SURFACE_CONTROL`, `IDENTITY_PROVIDER`, `WEB_APP_INSTALLATION`, `DISK_QUOTA`.

    Today's Linux policy denies "Other", while Windows keeps the engine default for unknown kinds. See the open questions.
  - The answer is `Continue(cef_permission_request_result_t)` with `CEF_PERMISSION_RESULT_ACCEPT`, `DENY` or `DISMISS`. `IGNORE` is CEF's value when a prompt goes away for other reasons.
  - `OnDismissPermissionPrompt(prompt_id, result)` reports CEF withdrawing a prompt (navigation, browser close).
  - `OnRequestMediaAccessPermission(browser, frame, requesting_origin, requested_permissions, CefMediaAccessCallback)`, with `cef_media_access_permission_types_t`: `DEVICE_AUDIO_CAPTURE` → Microphone, `DEVICE_VIDEO_CAPTURE` → Camera, `DESKTOP_VIDEO_CAPTURE` / `DESKTOP_AUDIO_CAPTURE` → ScreenShare. `CefMediaAccessCallback::Continue(allowed)` must match the required permissions for getUserMedia, or `Cancel()` is called.
  - Per the doc comments: with Chrome style, returning false shows **Chromium's own prompt UI**, so the shell must return true to keep its own prompt. With Alloy style, the default denies or ignores.
  - *(Inference.)* Which of the two callbacks fires for getUserMedia under Chrome style vs Alloy style must be checked in CEF 154. So must whether `Continue(ACCEPT)` makes Chromium **persist** an Allow in its content settings. If it does, Chromium would stop asking and bypass core's model. Windows prevents this with `SavesInProfile(false)`.

### 4.3 Decision and the waiting queue (KEEP)

Per tab, `TabPermissions { grants: TabGrants, pending: VecDeque<Pending>, dismissed: Vec<Permission> }` is held in memory. `Pending` is `{ request, permissions, asked_by: committed URI at request time }`.

- **`handle`:**
  - No window: deny.
  - Otherwise, map the kind (§4.2), then `decide_request`.
- **`decide_request`** is `decide` via `site_permissions_in(tab.browsing())`, plus one rule: if the result is `Ask(asked)` and **every** asked permission was dismissed or never-allowed **in this document**, it becomes **Block** (not asked again).
- **Results:**
  - Allow: allow now.
  - Block: deny now.
  - Ask with **8 or more** requests already waiting in the tab (`MAX_WAITING`): deny.
  - Ask otherwise: push onto the queue and run `window.sync_permission_prompt()`.
- **Every request gets an answer**, now or later. `handle` always returns true.
- **`next_prompt(browser, tab)`** walks the queue from the oldest. Waiting requests that settings, grants or a dismissal now decide are answered and popped along the way. It stops at the first one still `Ask(asked)` and returns `NextPrompt { request, asked, prompt: core::prompt(origin, &asked) }`. `asked` is only the still-undecided part. For example, if the camera was allowed meanwhile, a camera+microphone request asks only for the microphone.

### 4.4 The prompt bubble and its lifecycle (KEEP)

- **Look (`prompt_popover`).** A vertical box with 12 px spacing and margins, **320 px** wide.
  - Heading in heading style, wrapped at 40 characters.
  - Body, wrapped.
  - A vertical stack of buttons, one per answer, in core's order. The **first has the `suggested-action` style**.
  - The popover has the class `permission-prompt`, **autohide off**, and takes **no grab and no focus**: typing continues where it was, in the page or in the address bar (unit-tested).
  - It is anchored on the security icon via `AddressBar::show_prompt`.
- **Click guard.** Buttons ignore activation for **500 ms after the bubble is mapped** (`PROMPT_GUARD`), so a click or key meant for the page cannot answer it (as Chrome guards its prompts). Unit-tested: an early click leaves the request waiting.
- **When it shows (`BrowserWindow::sync_permission_prompt`).**
  - It shows only the **selected tab's oldest** waiting prompt, one at a time per window.
  - It shows nothing while **any bubble is open from the address bar**, or while the **window is fullscreen** (the bar is hidden).
  - If the prompt on screen is already for the same request with the same `asked`, nothing changes.
  - Otherwise any shown prompt is **withdrawn** (popped down without answering) and the next is shown.
  - `sync_permission_prompt` is called on: a new waiting request; tab selection change; commit in the selected tab; `bubble-changed`; fullscreen change; and `permissions::enforce`, which runs it for every window.
- **Answering.** A button press calls `answer_permission_prompt(request, answer)`: if the prompt is still the one for that request, it is popped down, then `permissions::answer(...)`, then `permissions::enforce(browser)`, which shows the next prompt.
- **Dismissal by closing.** A close the window did not make counts as **"Not now"**: Escape in the bubble, or a click elsewhere that left the tab and bubbles unchanged. The `closed` handler calls `permissions::answer(..., Answer::Dismiss)`, then `enforce` on idle.
- **Click anywhere in the window.** `window/mod.rs` installs a capture-phase `GestureClick` (any button) on the window, `clicked_while_prompting`. After the click has done its work (idle), if the same prompt is still shown, the same tab is selected, and no bubble is open, the prompt is popped down, which counts as Not now. A click that switches tabs or opens a bubble withdraws the prompt instead; it comes back later. (Unit test: `another_bubble_or_a_tab_switch_withdraws_the_prompt_and_a_click_on_the_page_dismisses_it`.)
- **Cross-window settlement.** A kept answer (Allow while visiting) settles the same pending prompt in other windows. `enforce` re-runs `next_prompt` everywhere (unit test `a_kept_answer_settles_the_same_prompt_in_another_window`).

### 4.5 Applying an answer (`permissions::answer`) (KEEP)

1. Remove the request from the tab's queue. If it is gone, do nothing.
2. For Dismiss or NeverAllow, add `asked` to `dismissed`.
3. Call `site_permissions_in(tab.browsing()).answer(origin_of(asked_by), asked, answer, &mut grants)`. An error is logged and treated as not granted.
4. Settle the engine request with allow only if granted **and** the site of `asked_by` equals the site of the current committed URI (`holds`: same scheme, host and port-or-default; `asked_by` must exist).
5. The caller runs `enforce`.

### 4.6 Navigation and closure (KEEP; inputs NEEDS-CEF)

- **`permissions::committed(tab)`** runs on every committed navigation: new documents (WebKit `LoadEvent::Committed`) and same-document ones (`check_same_document_commit`).
  - It calls `grants.committed(url)`.
  - **If a load is in flight**, meaning a new document, it clears `dismissed`. Same-document navigations such as `history.pushState` keep it.
  - It denies every pending request whose `asked_by` site differs from the new committed URI's site.
  - Unit tests: `a_dismissed_prompt_is_not_asked_again_by_the_same_document` and `a_kept_answer_decides_the_next_request_and_leaving_the_site_denies`.
- **`permissions::closed(tab)`** runs on tab close and tab discard (window closing, a popup never shown). It denies all pending requests.
- **NEEDS-CEF.** Commit signals come from `CefLoadHandler::OnLoadStart` (main frame, new document) and `CefDisplayHandler::OnAddressChange` (also same-document) *(inference: distinguish same-document from new-document by whether OnLoadStart fired)*. Closure comes from `CefLifeSpanHandler::OnBeforeClose`. Pending CEF callbacks must be answered (`Continue(DENY)` / `Cancel()`) on these events, and on `OnDismissPermissionPrompt`.

### 4.7 `navigator.permissions.query` (KEEP; mechanism NEEDS-CEF)

`permissions::query` answers the names `geolocation`, `notifications`, `camera`, `microphone` and `clipboard-read` from `decide_request` for the committed origin:

| Decision | Answer |
|---|---|
| Allow | `granted` |
| Block | `denied` |
| Ask | `prompt` |

Other names answer `prompt`. The rule: **a frame never reads a state its requests would not meet**.

- **NEEDS-CEF.** CEF has no query hook. Chromium answers from its own permission state. The likely mechanism is mirroring core's stored settings into Chromium content settings with `CefRequestContext::SetContentSetting(url, url, cef_content_setting_types_t, value)` (`ImplRequestContext::set_content_setting`) for these types:
  - `CEF_CONTENT_SETTING_TYPE_GEOLOCATION`
  - `NOTIFICATIONS`
  - `MEDIASTREAM_CAMERA`
  - `MEDIASTREAM_MIC`
  - `CLIPBOARD_READ_WRITE`
  - `MIDI_SYSEX`

  with values `CEF_CONTENT_SETTING_VALUE_ALLOW`, `BLOCK`, `ASK` and `DEFAULT`. This is what Windows does (`vsesvit-winui/src/permissions.rs`, `settings_changed`, which copies stored settings into the engine at startup and after every change, in the normal profile only). *(Inference: one-time grants and per-document dismissals cannot be expressed that way; check what Chromium reports.)*

### 4.8 Notifications (KEEP; WEBKIT-ONLY mechanics)

- **Policy (KEEP).** A notification shows only if `decide([Notifications])` is Allow for the tab's committed origin. In private it is always blocked.
- **`seed_notifications(browser)`** passes `site_permissions().all()` Notifications Allow and Block origins to `WebContext::initialize_notification_permissions`. This runs at startup (`Browser::new`) and on every `enforce`. **(WEBKIT-ONLY):**
  - A running web process keeps the notification permissions it started with, even across reloads, and so does a process reused from a closed page. That is why `show-notification` re-checks `notification_allowed` and closes the notification otherwise.
  - WebKit reads a seeded Block as `Notification.permission == "default"`. A unit test pins this quirk.
- **NEEDS-CEF.** Mirror the settings into `CEF_CONTENT_SETTING_TYPE_NOTIFICATIONS` (§4.7). Notification display under CEF goes through Chromium's notification platform. *(Inference: Chrome style shows native Linux notifications; whether Alloy style supports Web Notifications at all must be checked.)*

### 4.9 Capture: state, indicators, stopping (KEEP; mechanics NEEDS-CEF)

- **State (`capturing(web_view)`).** Camera, microphone and display capture states each count as live when `Active` or `Muted`. `TabChange::Capture` fires on each change. **(WEBKIT-ONLY source.)**
  - **NEEDS-CEF:** `CefDisplayHandler::OnMediaAccessChange(browser, has_video_access, has_audio_access)` (`on_media_access_change`). It has **no screen flag**, so screen capture must be tracked from the `DESKTOP_*` media request the shell allowed *(inference)*.
- **Address-bar in-use button** (§1.1): `set_in_use(indicator(capturing))`.
  - Icon: `screen-shared-symbolic` if the screen is shared; else `camera-web-symbolic` if the camera is live; else `audio-input-microphone-symbolic`.
  - Tooltip: `Capturing::description()`.
- **Tab indicator** (`BrowserWindow::sync_indicator`): the same icon and tooltip on the tab (sidebar row and top tab bar). It is non-activatable and takes priority over the mute/playing speaker and over the group dot.
- **Screen-sharing banner.** An `adw::Banner` above the page inside the tab (`tab.rs`), revealed while the screen is shared.
  - Title (`sharing_title`): **"Sharing your screen with {host[:port]}"**, or **"…with this page"** without an origin.
  - Button **"Stop sharing"** sets the display capture state to None.
- **Stopping (`stop(web_view, permission)`).** Sets the camera, microphone or display capture state to None. **(WEBKIT-ONLY API.)**
  - **NEEDS-CEF:** CEF has no API to stop a page's live capture *(inference)*. Windows reloads the page instead (`reload_allowed_captures`).
- **`enforce(browser)`** runs after any settings change: prompt answer, site-info change, Settings change, sync.
  1. `seed_notifications`.
  2. For every tab, and every permission it is capturing, stop the capture if `ends(permission, decide(...))`. `ends` is true for Block; true for Ask on camera and microphone (a revoked grant); false for Ask on ScreenShare (a share allowed this time continues); false for Allow.
  3. `sync_permission_prompt` in every window.
- **Memory Saver** treats a capturing tab as awake (`TabActivity.capturing`, `browser.rs::sweeps`).

### 4.10 The site-info Permissions section (KEEP)

`permissions::site_info_section(browser, tab)` returns `None` when there are no rows.

- **`listed(p)`** excludes `PictureInPicture`. Linux has no PiP, so a PiP setting synced from Windows is neither shown nor offered (unit test `site_info_leaves_out_picture_in_picture`).
- **Row inputs:** stored = `site_permissions_in(browsing).for_site(origin)` filtered by `listed`; granted = the tab's grants that `allows(origin, p)`; `capturing`. These feed `core::site_rows(origin.is_some(), …)`.
- **UI:**
  - A heading **"Permissions"**.
  - A boxed list of combo rows (`adw::ComboRow`): title = `Permission::label`, model = the `SiteChoice` labels, selected = current.
  - Live rows get a **"Stop"** suffix button, which stops that capture and hides itself.
  - If the page has an origin and anything is stored, a **"Reset permissions"** button follows, aligned to the start.
- **Choosing an option (`choose`):**
  - AllowedThisTime: nothing happens.
  - Otherwise, if the new setting is not Allow, revoke the tab's grant for it. For ScreenShare, which has no grant, stop a live share directly.
  - Then `site_permissions_in(browsing).set(origin, p, choice.setting())`.
  - After any change (`changed`), `enforce(browser)` runs, then the section rebuilds itself on idle, after the combo row has finished emitting.
- **Reset (`reset_site`):** clear the tab's grants, stop a live screen share, then `site_permissions_in(browsing).reset_site(origin)` (which leaves Trackers and Cookies alone). Then `changed`.
- **Self-tests.**
  - `site_permissions`: Block on Location stores Block, and the next request after a reload fails with PERMISSION_DENIED without a prompt.
  - `capture_in_use`: Camera and Microphone show "Allowed this time" with Stop; Block on Camera stores Block and stops only the camera; Stop on Microphone ends the rest and the indicators.

### 4.11 Private windows (KEEP)

A private tab decides and answers with `site_permissions_in(Browsing::Private)`. Choices are kept in memory over the stored ones until the last private window closes (`Profile::end_private_session`). Notifications are always blocked. The Permissions section works the same in private, unlike the tracking protection and cookie sections.

### 4.12 Settings > Privacy > Site Permissions (interaction; another inventory owns the page)

- `dialogs/settings.rs::site_permissions_page` lists `site_permissions().by_site()`, filtered by `listed`, grouped by heading.
- Each setting is a combo row. Choices are `permission.settings()`; for Cookies they are `cookies::site_choices(Some(current), false)` flattened.
- Each row has a **Remove** button (`user-trash-symbolic`, tooltip "Remove") that sets `None`.
- The empty state reads **"No Site Permissions"** / **"Sites you allow or block show here."**
- After a change: `permissions::enforce`, then `trackers().apply()` for Trackers and `cookies().apply()` for Cookies.

### 4.13 Sync (interaction)

`Browser::sync_applied(&vsesvit_core::sync::Changed)`:

- If `changed.site_permissions`, run `permissions::enforce`.
- If site permissions or `privacy.tracking_protection` changed, run `trackers().apply()`.
- If site permissions or `privacy.third_party_cookies` changed, run `cookies().apply()`.

`vsesvit_sync` itself is not called from these files.

---

## 5. Cookie controls (`cookies.rs`)

### 5.1 Core contract (`vsesvit_core::cookies`) (KEEP, verified)

- **`ThirdPartyCookies`** is stored in the synced pref `privacy.third_party_cookies`. The default is **`BlockInPrivate`**.

  | Choice | `label()` | `description()` |
  |---|---|---|
  | `Allow` | "Allow third-party cookies" | "Sites can use cookies from other sites they embed, which can track you across sites" |
  | `BlockInPrivate` | "Block third-party cookies in private windows" | "In private windows, sites can't use cookies from other sites they embed" |
  | `Block` | "Block third-party cookies" | "Sites can't use cookies from other sites they embed. Some sites may not work" |

  `blocks(browsing)` says whether the choice blocks in that kind of window.
- **`third_party_blocked(profile, browsing, top: Option<&Origin>)`**: the Settings choice, lifted on a top site whose cookie rule is Allow. `top: None` asks for all sites at once, and Linux passes `None`.
- **`setting(profile, origin)`** and **`set(profile, origin, Option<Setting>)`** use `Permission::Cookies` via `site_permissions()`, the **normal** store, even from private tabs.
- **`site_choices(current, exceptions)`**: `[None(Default), Allow (only if exceptions || current == Allow), Block, ClearOnExit]`. **`choice_label`**: "Default", "Allow", "Block", "Clear on exit".
- **`site_status(third_party_blocked, setting)`**:

  | State | Text |
  |---|---|
  | Block | "This site can't use cookies" |
  | ClearOnExit | "This site's cookies and data are deleted when you close Vsesvit" |
  | not blocked, Default or Allow | "This site and sites it embeds can use cookies" |
  | blocked, Default | "Third-party cookies are blocked on this site" |
  | blocked, Allow | "This site can use cookies. Third-party cookies are blocked on it" |

- **`SITE_TITLE`** is "Cookies and site data".
- **`site_rules(profile) -> SiteRules`**:
  - `blocked_hosts()`: the ASCII hosts of Block sites, sorted and deduplicated.
  - `to_clear()`: the origins of Block and ClearOnExit sites.
  - `blocks_cookie(domain)` and `clears_cookie(domain)`: a domain cookie `.example.com` reaches subdomains; a host-only cookie matches its host only.
  - `clears(name)`: whether a website-data record name (a registrable domain) covers or is covered by a host to clear.
- **`block_script(blocked_hosts) -> Option<String>`**: JS run at document start in every frame. On a blocked host or its subdomain, judged by the document's origin (so `about:blank` frames are covered), `document.cookie` reads `""` and ignores writes, and `cookieStore` is deleted.
- **`group_by_site(domains)`**: counts cookies per site. It is used by engines that list cookies one by one (Windows), and is a likely fit for CEF's `CefCookieManager::VisitAllCookies`.

### 5.2 What the Linux shell does

The `Cookies` value, created in `Browser::new` with the normal `NetworkSession`:

- **The third-party policy** is one accept policy per WebKit network session. `apply_policies`:
  - normal session: `NoThirdParty` if `third_party_blocked(Normal, None)`, else `Always`;
  - private (ephemeral) session, while one exists (`set_private_session(Some/None)` from `Browser::network_session` and `end_private_session`): the same with `Browsing::Private`.

  The policy is therefore **(WEBKIT-ONLY)**: no per-site exception is possible. Settings and the site info say so: a site's Allow cannot lift blocking, and site info offers Allow only when it was synced from Windows (`site_choices(current, false)`).
  - **NEEDS-CEF.** The likely route is the Chromium pref `profile.cookie_controls_mode` per request context, set with `CefRequestContext::SetPreference` *(inference: pref name and values 0, 1 and 2 must be checked in Chromium 154)*. A per-site exception could use content setting `CEF_CONTENT_SETTING_TYPE_COOKIES`, or per request `CefResourceRequestHandler::GetCookieAccessFilter` → `CefCookieAccessFilter::CanSendCookie` / `CanSaveCookie` (`ImplCookieAccessFilter`). These act on the IO thread and on network cookies only, not `document.cookie`.
  - With exceptions, core's `site_choices(current, true)` would always offer Allow, as on Windows. That is a deliberate behavior change to decide on.
- **A site set to Block** (`apply`):
  1. A WebKit content blocker (`content_blocker(hosts)`) with one rule per blocked host. **(WEBKIT-ONLY form.)** Each rule is `{"trigger":{"url-filter":"^[^:]+://([^/:]*\\.)?{host-escaped}[:/]"},"action":{"type":"block-cookies"}}`, with `.`, `[` and `]` escaped, because WebKit patterns have no alternation. The rule keeps requests to the host and its subdomains from sending or storing cookies. The JSON is compiled with `Blocker` (§7) into `<profile root>/cookies`, identifier `vsesvit-cookie-rules`.
  2. A `webkit::UserScript` of `cookies::block_script(blocked_hosts)` (all frames, document start), swapped on every tab's `UserContentManager`.
  3. If any host is blocked, `delete_blocked_cookies`: list all cookies in the **normal** session and delete those for which `rules.blocks_cookie(domain)` holds.
  - **NEEDS-CEF**, with these candidates:
    - content setting `COOKIES` = `BLOCK` for the site, which also blocks site storage *(inference)*;
    - or `CefCookieAccessFilter`, returning false for the blocked hosts;
    - plus the block script, injected at document start. CEF has no user-script API; it needs a render-process `CefRenderProcessHandler::OnContextCreated` or DevTools `Page.addScriptToEvaluateOnNewDocument` via `ExecuteDevToolsMethod`, which Windows uses through its DevTools sessions *(inference)*;
    - deletion via `CefCookieManager::VisitAllCookies` and `DeleteCookies(url, name)`.
- **Clear on exit / Block data deletion** (`clear`):
  - It fetches every website-data record in the normal session (`WebsiteDataManager::fetch(ALL)`), keeps those with `rules.clears(name)`, and removes all their data types.
  - It runs **at startup** (`clear_at_start`, in `Browser::start`). Pages wait for it (§7.2).
  - It runs **at exit** (`clear_at_exit`, from `Browser::shutdown`), blocking on the main context for up to **3 s** (`EXIT_CLEAR_WAIT`), and logs if not done.
  - It runs on demand in the self-test (`cookies().clear().await`).
  - It covers the normal session only. Private data goes with the ephemeral session.
  - **NEEDS-CEF.** `CefCookieManager::DeleteCookies(url, "")` per site for cookies. Other storage (localStorage, IndexedDB, cache, service workers) has no `CefRequestContext` API. DevTools `Storage.clearDataForOrigin` (`ExecuteDevToolsMethod`) needs a browser target, which Windows uses; `CefRequestContext::ClearHttpCache` is all-or-nothing *(inference)*. The content-setting value `CEF_CONTENT_SETTING_VALUE_SESSION_ONLY` might make Chromium clear the data itself at shutdown *(inference: check whether CEF runs Chromium's session-only cleanup)*.
- **`holds(WebsiteDataTypes)`** gives the Settings Site Data row subtitle: "Cookies", "site storage" (local/session storage, IndexedDB, DOM cache, offline app cache), "service workers" and "cached files" (disk or memory cache), joined with ", " and capitalized; "Other site data" if none apply. Unit test: `COOKIES|LOCAL_STORAGE` reads "Cookies, site storage". **(WEBKIT-ONLY types; the wording is KEEP.)**
- **`remove_data(manager, types, records)`** is an awaited `WebsiteDataManager::remove`, used by `dialogs/site_data.rs`: "See All Site Data" lists records sorted by name, with Remove per site and "Remove All…" (confirm "Remove All Site Data?" / "Sites will forget you, and you will be signed out of them."). The empty state reads "No Site Data" / "Sites that keep cookies or other data on this device show here." **NEEDS-CEF** for listing (cookies via `VisitAllCookies` plus `group_by_site`, as Windows does).
- **`attach(manager)`** is called for **every tab, normal and private** (`Tab::new`, `Tab::new_related`), and for view-source's hidden view (`view_source.rs`). So private tabs follow the stored Block rules too. **(KEEP:** private tabs follow stored rules.)

### 5.3 The site-info cookies section (KEEP)

`cookies::site_info_section(browser, tab) -> Option<gtk::ListBox>`:

- It is `None` in a **private** tab: a private rule would have to be compiled into the blocker on disk. Windows hides it in private too (PLAN, "Private windows"), so the hiding is **KEEP** product behavior, even though its reason is WebKit-specific.
- It is `None` unless the committed URL is **http or https** with an origin.
- **UI:** a boxed list with one `adw::ComboRow`. Title "Cookies and site data". Subtitle `site_status(third_party_blocked(Normal, None), current)`. Options `site_choices(current, false)` with `choice_label`; selected = current.
- **On change:**
  1. `cookies::set(origin, chosen)`. On error, log and stop.
  2. Update the subtitle.
  3. `browser.cookies().apply()`.
  4. `when_applied`, then **load the tab's committed URI again** (`tab.load(uri)`, not `reload()`), because a reload would send the request as the old rules shaped it.
- **Self-test `cookies`:**
  - The default reads "Default" with "This site and sites it embeds can use cookies".
  - Block reads "This site can't use cookies"; the reloaded page reads an empty `document.cookie`; the site's cookies are deleted; `/set-cookie` stores nothing.
  - Under Clear on exit, `served` is stored; Site Data lists `127.0.0.1` starting with "Cookies"; `clear()` leaves no cookie and no record.
  - The Settings radio rows: `BlockInPrivate` checked by default; Block sets the session policy to `NoThirdParty`; back to the default sets `Always`.
  - Self-test `private_window`: the private session's policy is `NoThirdParty` while the normal one is `Always`, and private site info offers no cookie rule.

### 5.4 Settings (interaction)

Settings > Privacy > group **"Cookies"** has three radio rows, one per `ThirdPartyCookies` with its label and description (`add_third_party_cookies_rows`). They follow pref changes, including from sync (`watch_prefs`). The group also has "See All Site Data" ("Cookies and other data sites keep on this device"). `Browser::set_third_party_cookies` writes the pref, then calls `cookies().apply()`.

---

## 6. Tracking protection (`trackers.rs`)

### 6.1 Core contract (`vsesvit_core::trackers`) (KEEP, verified)

- **`TrackingProtection`**: `Off`, `Standard` (default), `Strict`. It is stored in the synced pref `privacy.tracking_protection`.
  - Labels: "Off", "Standard", "Strict".
  - Descriptions:
    - Off: "Sites can load trackers".
    - Standard: "Blocks known advertising and analytics trackers on other sites".
    - Strict: "Also blocks social media trackers, such as Like and Share buttons. Some sites may not work".
- **`TrackerList::bundled()`** parses `vsesvit-core/src/trackers.json` (MIT, by company):
  - Each company entity lists `sites` plus `advertising`, `analytics` and `social` tracker domains.
  - A company's trackers are first party on its own sites and tracker domains.
  - A domain covers its subdomains.
  - Standard blocks advertising and analytics; Strict blocks social too.
- **`TrackerList::with_tracker(host, Category)`** is used by the self-test to make `localhost` a tracker.
- **`TrackerList::blocks(level, page_host, request_host) -> Option<&str>`** is the per-request decision. Windows uses it, and it would give a count.
- **`TrackerList::dnr_rules(level, allowed: &[Origin]) -> String`** writes DNR `rules.json`:
  - one `block` rule per company, with `requestDomains` = its blocked domains and `excludedInitiatorDomains` = its first-party list;
  - then one `allowAllRequests` rule (priority 2) per allowed origin, with `urlFilter` = `|{origin}/` and `resourceTypes` = `["main_frame"]`;
  - empty if nothing is blocked.
- **`TITLE`** is "Tracking protection".
- **`site_status(on, blocked: Option<usize>)`**:

  | State | Text |
  |---|---|
  | off | "Off for this site" |
  | on, no count | "Blocking known trackers on this site" |
  | count 0 | "No trackers blocked on this page" |
  | count 1 | "1 tracker blocked on this page" |
  | count n | "{n} trackers blocked on this page" |

- **Site exceptions:** `level_for(profile, origin)`, `allowed(profile, origin)`, `set_allowed(profile, origin, allowed)` (stores `Permission::Trackers` = Allow, or `None`), `allowed_sites(profile)`. All use the **normal** store.
- **Known engine difference** (core doc): WebKit judges a frame's requests by the frame's own site, while Windows judges by the tab's page, so a Google frame on another site may load Google's trackers on Linux only. With CEF, the shell chooses the rule.

### 6.2 What the Linux shell does

- **`Trackers::new(core)`**: the list starts as the bundled one, and the blocker lives in `<profile root>/trackers` with identifier `vsesvit-tracking-protection`.
- **`apply()`** reads the level and `allowed_sites`, then `content_blocker(list, level, allowed)`:
  - It runs `vsesvit_webext::dnr::parse_rules(&list.dnr_rules(level, allowed))`.
  - It translates with `dnr::translate(&rules, "", &dnr::Grants { hosts: dnr::HostScope::All, host_access_only: false })`.
  - It logs skipped or malformed rules (`dnr::describe_skipped`).
  - It produces `Some(translation.to_json())` unless the result is empty. Off compiles no blocker (unit test).
  - The JSON goes to `Blocker::apply`.
  - **(WEBKIT-ONLY):** the WebKit content-blocker translation and on-disk compile. Note that `vsesvit_webext::dnr` lives in the crate expected to be deleted.
- **`attach(manager)`**: every tab, normal and private, and view-source's hidden view.
- **`when_applied(f)`** and **`use_list(list)`** (self-test only) are provided.
- **No blocked count.** WebKit reports no content-blocker hits, so the popover gives no count. **(WEBKIT-ONLY limitation.)** Windows shows `site_status(on, Some(n))`.
- **NEEDS-CEF.** Per-request blocking in `CefResourceRequestHandler::OnBeforeResourceLoad` (`ImplResourceRequestHandler::on_before_resource_load`, returning `RV_CANCEL`), obtained through `CefRequestHandler::GetResourceRequestHandler` *(inference: these run on the IO thread, while `Profile` is UI-thread-only; `TrackerList` is plain data)*.
  - This also allows a per-page count, as Windows does.
  - Alternatively, Chromium's declarativeNetRequest via a bundled extension *(inference)*.

### 6.3 The site-info tracking protection switch (KEEP)

`trackers::site_info_section(browser, tab) -> Option<gtk::ListBox>`:

- It is `None` in a **private** tab (PLAN: both shells hide it), unless the committed URL is http(s) with an origin, and when the level is **Off**.
- **UI:** a boxed list with one `adw::SwitchRow`. Title "Tracking protection". It is active when not allowed for the site. Its subtitle is `site_status(on, None)`.
- **On toggle:**
  1. Update the subtitle.
  2. `trackers::set_allowed(origin, !on)`. On error, log and stop.
  3. `browser.trackers().apply()`.
  4. `when_applied`, then `tab.reload()`.
- **Self-test `tracking_protection`:**
  - With `localhost` as an analytics tracker, `/trackers.html` at `127.0.0.1` reads "tracker blocked" and the server never sees `/tracker/pixel.png`.
  - The switch is on and reads "Blocking known trackers on this site".
  - Switching it off reads "Off for this site", stores the exception, and reloads into "tracker loaded".
  - Settings > Privacy's "Tracking Protection" row shows Standard with its description, and choosing Strict stores Strict.

### 6.4 Settings (interaction)

Settings > Privacy > group "Trackers" holds the combo row **"Tracking Protection"** (`TRACKING_PROTECTION_ROW`), with Off, Standard and Strict and the chosen level's description as its subtitle. It follows pref changes. `Browser::set_tracking_protection` writes the pref, then calls `trackers().apply()`. Sites the user turned protection off for appear in Site Permissions as "Trackers: Allow". Removing one re-protects the site.

---

## 7. Content blockers (`blocker.rs`) and startup gating

### 7.1 `Blocker` (WEBKIT-ONLY mechanism; the guarantees are KEEP)

- **`Blocker::new(dir, identifier)`** creates `dir` and opens a `webkit::UserContentFilterStore` there.
- **`attach(manager)`** adds the current filter to a new tab's `UserContentManager` and remembers the manager weakly.
- **`apply(Option<json>)`**:
  - Each call bumps a generation counter.
  - `None` removes the filter from every manager at once and settles the waiters.
  - `Some(json)` compiles asynchronously (`store.save(identifier, bytes)`). A result that a later `apply` overtook is dropped.
  - On success the filter is swapped on every live manager (old removed, new added) and "content blocker attached" is logged. On failure a warning is logged.
  - Either way the waiters settle.
- **`when_applied(f)`** runs `f` now if no compile is pending; otherwise after it finishes.
- **`managers()`**: the live managers. Closed tabs drop out.
- **Persistent state:** the compiled filter files in `<profile root>/trackers/` and `<profile root>/cookies/`.
- **Guarantees to keep:**
  1. A rule change reaches **every open tab** and every new tab.
  2. A page the shell reloads after a change **loads under the new rules**. This is the `when_applied` then reload pattern in §5.3 and §6.3.
  3. A later change wins over an earlier one still pending.

### 7.2 Startup gating (KEEP)

- **Ordering.** `Browser::start` calls `trackers().apply()`, `cookies().apply()`, then `cookies().clear_at_start()`. `app.rs::open_from_command_line` holds the application and opens windows (restored session or command-line URLs) only in `Browser::when_blockers_applied`, which waits for trackers applied, cookies applied, and the startup clear done. **No page of the restored session or the command line loads before tracking protection, the cookie rules and the startup data clearing are in place.** An invocation arriving meanwhile waits behind the first.
- **NEEDS-CEF.** The same ordering must hold before the first browser is created (or the first navigation). This applies equally if the rules live in the shell's request handlers rather than in compiled filters.

---

## 8. Cross-module interactions (summary)

| From | To | What |
|---|---|---|
| `tab.rs` commit / same-document commit | `permissions::committed`, `window.tab_changed(Committed)` | grants end, pending denied, dismissed cleared on new document; window re-syncs location, security, star, prompt |
| `tab.rs` load end | `window.sync_location` | shows the real page after a non-committing load |
| `tab.rs` progress, loading | `AddressBar::set_progress` / `finish_progress` | §1.10 |
| `tab.rs` zoom notify | `AddressBar::set_zoom`, the main menu zoom label | §1.1, §1.11 |
| `tab.rs` capture notify | in-use button, tab indicator, sharing banner | §4.9 |
| `tab.rs` TLS error | error page, Security Insecure | §1.3 |
| `browser.rs` close/discard tab | `permissions::closed` | §4.6 |
| `browser.rs` private session start/end | `Cookies::set_private_session`, `Profile::end_private_session` | §5.2, §4.11 |
| `browser.rs::sync_applied` | `permissions::enforce`, `trackers().apply()`, `cookies().apply()`, `window.apply_prefs()` | §4.13, §1.13 |
| `browser.rs::shutdown` | `cookies().clear_at_exit()` | §5.2 |
| `dialogs/settings.rs` | prefs, `set_tracking_protection`, `set_third_party_cookies`, `set_full_urls`, `set_compact_address_bar`, site permissions list | §1, §4.12, §5.4, §6.4 |
| `dialogs/site_data.rs` | `cookies::holds`, `cookies::remove_data` | §5.2 |
| `view_source.rs` hidden view | `trackers().attach`, `cookies().attach` | view-source fetches load under the same rules |
| Memory Saver | `tab.capturing().any()` | capturing tabs stay awake |
| Extensions runtime (to be replaced) | DNR blockers on the same `UserContentManager`; `dnr` translator reused by trackers | with CEF, extensions' DNR is Chromium's; tracking protection must coexist with it |
| `window.focus_page` | the address bar's `cancelled` signal and `resolve` → None | the page takes focus |

---

## 9. Self-test coverage map (checks touching these features)

From `docs/design/self-test.md` and `crates/vsesvit-gtk/src/self_test.rs`:

- **`omnibox`**: `resolve("vsesvit fixture")` is a default-engine search; `resolve("127.0.0.1:<port>/page2.html")` is that http URL.
- **`address_completion`**: typing `127.0.0` shows `127.0.0.1:<port>` with `.1:<port>` selected, row 0 highlighted, and the list at least as wide as the bar (`address-completion.png`). Down highlights row 1 and shows its fill with no selection. Escape brings back row 0 and the completion. Enter opens `http://127.0.0.1:<port>/`.
- **`search_suggestions`**: with a fixture engine whose `suggest_url` is set (`SearchEngines::set_suggest_url`), typing `vsesvit` lists `vsesvit`, `vsesvit one` and `vsesvit two` first, after exactly one `/suggest` request (`search-suggestions.png`). Down reads `vsesvit one`, and Enter loads `/search?q=vsesvit+one`. A typed URL, and text typed with suggestions off, send nothing within 400 ms.
- **`search_engines`**: `fx vsesvit fixture` plus Enter searches the `fx` engine.
- **`bookmark`, `star_bubble`**: star state and the bubble headings "Bookmark added" / "Edit bookmark".
- **`address_progress`**: §1.10.
- **`zoom_indicator`**: hidden at 100%; "110%" after zoom-in; the chip opens the bubble (`zoom.png`); hidden after reset.
- **`connection_info`**: §3.4.
- **`site_permissions`**: prompt "Know your location?" (`permission-prompt.png`); Allow while visiting stores Allow, and `permissions.query` reads `granted`, also after a reload, with no new prompt; site info Block (`site-info-permissions.png`); Settings lists it and Remove empties the list (`settings-site-permissions*.png`).
- **`capture_in_use`**: with mock devices, the prompt "Use your camera and microphone?" with the four answers in order; Allow this time gives live tracks; the in-use button and tab indicator read "Using your camera and microphone" (`capture-in-use.png`, `capture-in-use-top.png`); site info shows both as "Allowed this time" with Stop (`site-info-capture.png`); Block on Camera stops only the camera; Stop ends the rest. **NEEDS-CEF** fake devices: Chromium switch `--use-fake-device-for-media-stream` *(inference; avoid `--use-fake-ui-for-media-stream`, which bypasses prompts)*.
- **`tracking_protection`**: §6.3. **`cookies`**: §5.3. **`private_window`**: cookie policy and no cookie rule in private.
- **`https_only`**: the warning page is marked Insecure in the address bar (`https-only-warning.png`).
- Unit tests in `address_bar.rs` and `permissions.rs` (listed in §1 and §4) encode the exact keyboard, refill and prompt-queue semantics, and are worth porting as behavior tests.

---

## 10. Risks and open points specific to these features

1. **Who owns permission state.** Under CEF, Chromium keeps its own content settings and may persist answers. If it does, core's `site_permissions` model (one-time grants, document-scoped dismissals, private-session overlay, synced LWW records) can be bypassed. Mirroring into content settings, as Windows does, covers stored settings only.
2. **Chrome style shows Chromium's own permission prompts** unless the handler returns true. **Extensions** using `chrome.contentSettings` or `chrome.privacy` would write Chromium's state, not core's *(inference)*.
3. **Stopping a capture** has no CEF API *(inference)*. Windows reloads the page.
4. **Screen capture** is not reported by `OnMediaAccessChange` (camera and microphone flags only).
5. **Thread affinity.** Resource and cookie filters run on CEF's IO thread, while `Profile` is UI-thread-only.
6. **Per-site data clearing and listing** has no complete `CefRequestContext` API. It needs DevTools (`Storage.clearDataForOrigin`) or Chromium's session-only cookie setting *(inference)*.
7. **`vsesvit_webext::dnr`** is used by tracking protection today, and the crate is expected to be deleted.
8. **Committed vs visible URL.** The bar, security, site info and permission rules all use the **committed** URI. `GetVisibleNavigationEntry` may return a pending entry *(inference)*. The main frame's committed URL and SSL status must come from the committed entry.
9. **Extension omnibox keywords** (`omnibox` manifest key) and **`chrome_settings_overrides` search providers**, needed for "total extension compatibility", are driven by Chrome's own omnibox and TemplateURLService. Neither exists when the omnibox is GPUI plus core *(inference)*. They are unsupported in `vsesvit-webext/src/support.rs` today.

---

## 11. Checklist

- [ ] Address bar field: placeholder "Enter address", accessible label "Address", URL input purpose, no spellcheck
- [ ] Centered text at rest, left-aligned while focused or editing
- [ ] Compact address bar (max 720 px, centered) vs full width, following synced `address_bar.compact` (default on), live and on sync
- [ ] Private window icon `user-not-tracked-symbolic` "Private browsing" before the bar
- [ ] Resting URL: simplified (`simplified_url`) unless focused or `address_bar.full_urls` is on; `about:blank` shows empty
- [ ] URL display decoding via core `readable_url` (replacing WebKit `uri_for_display`)
- [ ] Focus in shows the full URL and selects all after the click; focus out re-simplifies; typed text survives focus loss
- [ ] Error pages show the failed URL; the new tab page shows empty
- [ ] Security icon per `Security::of` (Secure, Insecure, Internal, none) with the exact tooltips; tab overrides for Certificate/HTTPS-only error pages (Insecure) and other error pages (none)
- [ ] Certificate errors never show the page (cancel in `OnCertificateError`, show the shell error page)
- [ ] Security icon and in-use button hidden while editing
- [ ] Clicking the security icon opens site info
- [ ] In-use button after the security icon (icon per capture kind, tooltip `Capturing::description`), opens site info, reserves text space
- [ ] Zoom chip before the star when zoom ≠ 100% ("NN%", tooltip "Zoom"), reserves text space, opens the zoom bubble
- [ ] Zoom bubble: "Zoom:", live level, Zoom Out, Zoom In, "Reset" (tooltip "Reset to 100%")
- [ ] Star: non-starred "Bookmark This Page" / starred "Edit Bookmark"; click or Ctrl+D adds to the bookmarks bar and opens "Bookmark added", or opens "Edit bookmark"; state follows the committed URL
- [ ] One bubble at a time from the bar (site info, zoom, star), anchored to its icon; bubble open/close re-syncs the permission prompt
- [ ] Load progress line: starts at 8%, never goes back, fills and holds 200 ms then hides, cleared on tab switch; gradient style
- [ ] Reload button flips to Stop while loading
- [ ] Edit model: `edited(text, inline_allowed)` on every user change; inline completion only after an insertion with the caret at the end
- [ ] Inline completion shown selected after the typed text on row 0
- [ ] Down/Up/Tab/Shift+Tab move the highlight (clamped, no wrap, no re-query), showing the row's fill
- [ ] Up/Down never move focus out of the bar
- [ ] Escape: back to row 0 if elsewhere, else cancel (show resting URL, focus page)
- [ ] Right/End on row 0 accepts the inline completion
- [ ] Enter with the list open opens the highlighted row; otherwise submits the text
- [ ] Ctrl+Enter makes a single word `https://www.<word>.com/`, otherwise acts as Enter
- [ ] Shift+Delete forgets a history row (deletes all visits) and re-queries the same text
- [ ] Ctrl+L / Alt+D / F6 focus the bar and select all (reassignable)
- [ ] Suggestion list: below the bar, bar-wide, max 440 px tall, no focus, no autohide, opens only while focused with rows
- [ ] Row layout: icon + title (end-ellipsized) + dim caption subtitle (middle-ellipsized); single click activates
- [ ] `set_suggestions` resets the highlight; `refill_suggestions` keeps the highlighted row by fill, keeps text, caret and completion
- [ ] Choosing a row shows the current page's URL until the new one commits; a plain submit keeps the typed text until commit or load end
- [ ] Submitted text resolved by `Omnibox::resolve`: URL or search, navigate with Typed, give the page focus; blank focuses the page; error toast "Cannot resolve the address: {e}"
- [ ] Per-tab unsubmitted text saved and restored on tab switch (memory only); focus goes to the bar for blank tabs or tabs with typed text
- [ ] Core rows from `Omnibox::suggest(text, 8, allow_inline)` mapped to icons, subtitles and transitions (Bookmark rows use `Transition::Bookmark`)
- [ ] Search row subtitle "Search"; typed search title `Search {engine} for "{terms}"`
- [ ] Open-tab rows: same browsing kind, all windows, not the selected tab, up to 4, "Switch to tab · {url}", select the tab and present its window
- [ ] Engine suggestions: `suggest_request` (nothing in private, pref off, non-plain search, or engine without suggest URL), worker-thread `run` (100 ms debounce, 5 s timeout, 4 max), stale results dropped, rows merged by `refill_suggestions`
- [ ] Settings: Search Suggestions, Browsing History and Bookmarks switches take effect on the next keystroke
- [ ] Site info popover: 380 px, scroll max 680 px, headline, icon and explanation per security state, host line
- [ ] Site info section order: tracking protection, cookies, permissions, certificate; separators between them
- [ ] Certificate section: valid line or problem lines; Issued to/by, Validity (local `%-d %b %Y`), Public key, Signature, Serial; SHA-256 fingerprint (monospace, selectable); SAN expander with count; chain list indented; unreadable-certificate messages
- [ ] Certificate data from CEF SSL status / X509 DER (or DevTools Security domain) parsed by core `certificate::parse_der`; cert status flags mapped to the problem sentences
- [ ] Permission requests mapped: location, notifications, clipboard read, camera+microphone as one prompt, screen share, MIDI (new with Chromium), pointer lock auto-allowed, device labels follow camera/mic, others denied (pending decision)
- [ ] Decision via `SitePermissions::decide` plus the same-document dismissal rule; queue up to 8 per tab, oldest first; extra requests denied
- [ ] Prompt bubble: heading and body from `core::prompt`, buttons in core order, first suggested, 320 px, no focus, no grab, no autohide, 500 ms click guard
- [ ] Prompt shown only for the selected tab, one at a time, not while another bubble is open or fullscreen; withdrawn and restored on tab switch or bubble
- [ ] Prompt closed by Escape or by a click on the page counts as "Not now"
- [ ] Answers applied with `SitePermissions::answer`; Allow honored only if the same site is still on screen; then enforce (other tabs and windows settle)
- [ ] Commit: grants end on origin change; pending requests from another site denied; dismissals cleared only on a new document
- [ ] Tab close: pending requests denied (and pending CEF callbacks answered)
- [ ] `navigator.permissions.query` matches what a request would get (mirror stored settings into Chromium content settings)
- [ ] Notifications shown only when allowed; always blocked in private; stored settings reach the engine at startup and after each change
- [ ] Capture tracking: in-use button, tab indicator (overrides speaker), screen-sharing banner "Sharing your screen with {host}" + "Stop sharing"
- [ ] Enforce after any settings change: stop captures no longer allowed (Block; revoked camera/mic grant; screen share only on Block)
- [ ] Site-info Permissions section: rows from `site_rows`, PiP excluded on Linux, combo rows with SiteChoice labels, Stop on live rows, "Reset permissions" (keeps Trackers and Cookies), rebuild after change
- [ ] Private tabs: permissions use the private overlay (in memory until the last private window closes)
- [ ] Third-party cookies policy per Settings choice for normal and private contexts (default: block in private only)
- [ ] Site cookie rule Block: no cookies sent or stored for the host and subdomains, `document.cookie`/`cookieStore` hidden by core's block script in all frames, existing cookies deleted
- [ ] Clear on exit / Block sites' data deleted at startup (before any page loads) and at exit (bounded wait, 3 s today)
- [ ] Site-info cookies section: http(s) only, not in private; "Cookies and site data", Default/(Allow)/Block/Clear on exit, status subtitle; change applies then reloads the page from its URL
- [ ] Decide whether Allow is always offered (Chromium per-site exceptions) or only when synced, as today
- [ ] Settings: third-party cookie radio rows with labels and descriptions; See All Site Data list with Remove / Remove All… and "holds" wording
- [ ] Private tabs follow the stored cookie Block rules and tracking protection
- [ ] Tracking protection level Off/Standard/Strict from the synced pref; bundled tracker list with first-party exemptions; per-site exceptions
- [ ] Tracker blocking reaches every tab and view-source fetches; changes apply to all tabs; reload after a change loads under new rules
- [ ] Site-info tracking protection switch: http(s) only, not in private, hidden when Off; "Tracking protection", "Blocking known trackers on this site" / "Off for this site"; toggling stores the exception and reloads
- [ ] Optional with Chromium: blocked tracker count in the site-info subtitle (`site_status(on, Some(n))`), as Windows has
- [ ] Startup: no restored or command-line page loads before tracking protection, cookie rules and startup clearing are in place
- [ ] Sync: site permission and privacy pref changes re-apply enforce, trackers and cookies; address-bar prefs re-apply live
- [ ] Self-test observation hooks for the address bar, prompt, bubbles, in-use button and zoom chip, so the listed checks can be ported
