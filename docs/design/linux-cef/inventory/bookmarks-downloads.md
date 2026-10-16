# Inventory: bookmarks bar, bookmark menus/editor/drag, favicons, downloads

Behavior inventory of the GTK/WebKitGTK Linux shell's bookmark and download UI, written so a
GPUI + CEF shell can reproduce every user-visible feature without reading the old code.

Scope (all in the worktree `/home/dy/git/vsesvit-cef`, branch `linux-cef-gpui`):

| File | Lines | What it is |
|---|---|---|
| `crates/vsesvit-gtk/src/bookmarks_bar.rs` | 501 | The bar under the tab bar: items, "»" overflow, "Other Bookmarks" |
| `crates/vsesvit-gtk/src/bookmark_menu.rs` | 545 | Folder menus (popovers of rows) and every bookmark context menu |
| `crates/vsesvit-gtk/src/bookmark_editor.rs` | 418 | The star's bubble and the Edit…/Rename…/Add Page… dialog |
| `crates/vsesvit-gtk/src/bookmark_drag.rs` | 212 | Drag-and-drop of bookmarks (bar and Bookmarks window) |
| `crates/vsesvit-gtk/src/favicons.rs` | 117 | Storing/reading bookmark favicons; faded sleeping-tab icon |
| `crates/vsesvit-gtk/src/downloads.rs` | 815 | The downloads controller, the unconfirmed-file warning, open/show file |

Callers and collaborators that were read to describe the contracts: `browser.rs` (star,
favicon recording, favicon preload, `bookmarks_changed`, bar visibility, private session),
`window/mod.rs`, `window/actions.rs`, `window/menu.rs`, `address_bar.rs` (star icon, bubble
anchoring), `keymap.rs`, `tab.rs` (sleep icon, response policy), `save_page.rs`,
`dialogs/{mod.rs,bookmarks.rs,downloads.rs,settings.rs,history.rs}`, `app.rs` (CSS),
`lib.rs` (`popup`), `self_test.rs`, and core: `vsesvit_core::{bookmarks, favicons, downloads,
prefs::keys, private, search::classify_url, shortcuts, history::Transition}`.

Legend:

- **KEEP**: product behavior to reproduce.
- **WEBKIT-ONLY**: a WebKitGTK workaround or limitation that disappears with Chromium.
- **NEEDS-CEF**: needs a CEF equivalent; the likely CEF API is named.
- "Verified" means read in code (the GTK shell, core, the `cef` 154.5.0+154.0.34 /
  `cef-dll-sys` bindings in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`, or the CEF
  source checkout at `/tmp/vsesvit-research/cef/cef-8037`, whose `CHROMIUM_BUILD_COMPATIBILITY.txt`
  names `refs/tags/154.0.8037.98`). "(inference)" marks anything not read directly.

---

## 0. Cross-cutting conventions these modules rely on

### 0.1 The "bookmarks changed" fan-out (KEEP)

Every bookmark write anywhere ends with `Browser::bookmarks_changed()` (`browser.rs:967`):

1. every browser window: `refresh_bookmarks_bar()` (rebuild or update the bar) and `sync_star()`
   (star state of the selected tab);
2. every registered bookmark view (`Browser::watch_bookmarks`, used by the Bookmarks window):
   refresh;
3. `preload_favicons()` (fetch icons of bookmarked sites that have none, §6.4).

Callers: the star (`star_clicked`), bubble Done/Remove, editor dialog Save, context-menu
Delete/Add Folder/Add Page, drag-and-drop, the Bookmarks window, import, favicon changes that
altered a stored icon, the favicon fetch committing new icons, and sync applying remote
bookmark records (`Browser::sync_applied`, `changed.bookmarks`). The new shell must have one
equivalent fan-out; nothing in core notifies the shell by itself.

### 0.2 Errors are toasts (KEEP)

Core errors from writes are shown as an `adw::Toast` in the window (`dialogs::plain_toast`,
markup off). Exact strings (format `{e}` is the core error's Display):

| Where | Text |
|---|---|
| star | `Cannot bookmark the page: {e}` |
| bubble Done, editor Save, context-menu edit | `Cannot change the bookmark: {e}` |
| bubble Remove | `Cannot remove the bookmark: {e}` |
| context-menu Delete | `Cannot delete the bookmark: {e}` |
| context-menu Add Folder… | `Cannot add the folder: {e}` |
| drag-and-drop on the bar | `Cannot move the bookmark: {e}` |
| download folder cannot be created | `Cannot save to {dir}: {e}` |
| download path not UTF-8 | `Cannot save the download: its path is not valid UTF-8` |
| download failed (not cancelled) | `Download of “{file}” failed` |

Core `BookmarkError` Display texts the user can see through these toasts
(`bookmarks.rs:632`): `no live bookmark {id:?}`, `{id:?} is not a folder`, `roots cannot be
moved, renamed or removed`, `a folder cannot be moved into itself or its descendants`,
`only url bookmarks have a url`.

### 0.3 Popovers (KEEP behavior, GTK mechanics are incidental)

`crate::popup(popover, anchor)` (`lib.rs:67`) parents a popover to its anchor; it is released
when it closes or when the anchor leaves the window (bar rebuilt, window closed). Under the
self-test (`SCRIPTED`), popovers open with autohide off because Wayland refuses popup grabs
without a real input event (a GTK-on-Wayland detail, not a WebKit one; a GPUI shell needs its
own answer for scripted runs). All bookmark menus, bubbles and the download warning
draw **over the web content** and can extend below/beside the window's toolbar; bookmark menus
may be as tall as 75% of the monitor (§2).

### 0.4 Stylesheet that defines the look (KEEP as design reference)

From `app.rs:31-50`:

```
.bookmarks-bar { min-height: 28px; padding: 2px 6px; }
.bookmarks-bar button { padding: 2px 6px; min-height: 24px; }
.bookmarks-bar .drop-before { box-shadow: inset 2px 0 @accent_bg_color; }
.bookmarks-bar .drop-after { box-shadow: inset -2px 0 @accent_bg_color; }
.bookmark-row.drop-before { box-shadow: inset 0 2px @accent_bg_color; }
.bookmark-row.drop-after { box-shadow: inset 0 -2px @accent_bg_color; }
.drop-into { background-color: alpha(@accent_bg_color, 0.25); border-radius: 6px; }
.bookmarks-bar .bookmarks-chevron { padding: 2px 8px; font-weight: bold; }
popover.bookmark-menu > contents { padding: 4px; }
.bookmark-menu-row { padding: 4px 8px; min-height: 24px; font-weight: normal; }
```

---

## 1. Bookmarks bar (`bookmarks_bar.rs`)

### 1.1 What the user sees and does

**Placement.** A horizontal strip in the window's top bars, in this order (`window/mod.rs:392-397`):
header bar (back/forward/reload/home, address bar, extension actions, downloads button, profile
button, main menu) → update banner → tab bar → **bookmarks bar** → find bar. It sits above the
web content and below the tab bar. In fullscreen all top bars hide (`set_reveal_top_bars(false)`,
`window/mod.rs:609-613`), the bar included.

**Visibility.** Shown/hidden with a slide-down/up reveal (`gtk::Revealer`, `SlideDown`).
Toggled by:

- `win.show-bookmarks-bar` (stateful bool action, checkmark in menus), default chord
  **Ctrl+Shift+B** (core `shortcuts::Command::ToggleBookmarksBar`, "Show or hide the bookmarks
  bar", reassignable through the keymap);
- main menu item `Show Bookmarks _Bar` (`window/menu.rs:34`);
- every bookmark context menu's `Show Bookmarks _Bar` (§3);
- Settings > Appearance switch `Show Bookmarks Bar` (`dialogs/settings.rs:652-658`);
- sync: a remote change of the pref re-applies window prefs (`sync_applied` → `apply_prefs`).

Toggling in one window writes the synced pref and applies it to **every** window
(`Browser::set_bookmarks_bar_visible`, `browser.rs:1157`). A new window starts per the pref
(`apply_prefs`).

**Layout of the strip** (left to right in LTR; mirrored in RTL):

1. The **row** (expands): the toolbar folder's items, as many as fit **whole**, then the
   **chevron** `»` at the row's far end when some do not fit.
2. The **empty-state label** (only when the toolbar folder has no items):
   `Bookmarks you add to the bar appear here` (dim, caption size, left-aligned, ellipsized,
   6 px start margin).
3. The **end box**: an `Other Bookmarks` folder button, only when the Other bookmarks root has
   at least one child.

Spacing between items is 1 px. Strip CSS classes `toolbar`, `bookmarks-bar`.

**Item kinds** (`item_widget`):

| Node | Widget | Content | Tooltip | Click | Middle-click |
|---|---|---|---|---|---|
| URL with a title | flat button | favicon (stored icon, else icon `web-browser-symbolic`) + title label | the URL | open in the current tab | open in a new foreground tab |
| URL with empty/blank title | flat button | favicon only (no label); accessible label = URL | the URL | same | same |
| Folder | flat button | icon `folder-symbolic` + title | the folder title | opens the folder's menu **below** the button (§2) | nothing |
| Separator | vertical separator line | — | — | — | — |

Labels are ellipsized at the end with a maximum of **18 characters** (`MAX_LABEL_CHARS`), single
line; icon-to-label spacing 4 px. A URL node whose `url` is `None` is not shown at all
(`bookmark_menu::collect` filters it).

"Open in the current tab" is `win.open-bookmark(url)` → `BrowserWindow::navigate_with(url,
Transition::Bookmark)`: loads in the selected tab (or a new tab if none), records the visit
with history transition `Bookmark`, and moves keyboard focus into the web view. Middle-click
is `win.open-in-new-tab(url)` → `open_tab(url, None, Focus::Foreground)` (foreground, unlike
Chrome's background middle-click; KEEP as is).

**Overflow chevron.** Label `»`, tooltip `More Bookmarks`, flat, bold, padding 2px 8px. Hidden
when everything fits. Clicking it opens a bookmark menu (§2) of the hidden items
(`items[shown..]`, in order, separators and folders included) below the chevron.

**Overflow algorithm** (`fitting`, pure, KEEP exactly):

```
widths = natural width of each item; chevron = chevron's natural width; SPACING = 1
if sum(widths) + SPACING*(n-1) <= width: show all, hide chevron
else: room = width - chevron - SPACING
      show items from the start while (used + w <= room), where used += w + SPACING after each
```

Unit-tested cases: `fitting(&[50,50,50], 152, 20) == 3`, `(…,151,20) == 2` (the chevron takes
the last item's place), `(…,100,20) == 1`, `(…,60,20) == 0`, `fitting(&[], 0, 20) == 0`.
Items are laid out at their **natural width** (never squeezed/ellipsized below natural), placed
left to right with 1 px gaps; the chevron is placed at the row's right edge (left edge in RTL).
The bar's **minimum width is only the chevron's width**, so a long bar never forces the window
wider (unit test `bookmarks_that_do_not_fit_move_to_the_chevron_menu_instead_of_widening_the_window`:
with 40 bookmarks the bar asks < 400 px; every shown item is unellipsized; no item reaches
under the chevron; the chevron is visible). Hidden items are not focusable or targetable.

**Context menus.** Right-click (or Menu key / Shift+F10 while focused) on an item opens that
node's context menu; on the strip's empty space opens the bar menu; on `Other Bookmarks` opens
the folder menu for the Other root. See §3.

**Drag-and-drop.** Every row item (links, folders, separators) can be dragged; items, the row's
empty space and `Other Bookmarks` accept drops. See §5. The chevron, the empty-state label and
menu rows are not drop targets (dropping on the chevron lands on the row = append to the bar;
inference from GTK picking the deepest widget with a drop target).

**Keyboard focus.** Bar buttons are ordinary focusable buttons in tab order: visible items,
chevron, `Other Bookmarks` (inference from widget order). Menu key or Shift+F10 on a focused
item opens its context menu below it.

### 1.2 Refresh behavior (KEEP)

`BookmarksBar::refresh(core)` runs on window creation and on every `bookmarks_changed()`:

1. Copies out the toolbar subtree (`bookmark_menu::collect(&bookmarks, BookmarkId::TOOLBAR)`,
   folders recursively up to depth 12), the Other root with its subtree, and the stored favicon
   of each **top-level** toolbar URL item.
2. If the copied toolbar items and Other subtree are **equal** to what the bar shows
   (`Item: PartialEq` compares node id, kind, parent, index, title, url, added_ms and all
   children recursively), it only updates the icons of existing buttons in place
   (`show_icons`). Reason (KEEP): bookmarked pages store icons as they load; rebuilding every
   button would close open bar menus and drop hover. Unit test
   `visiting_more_of_a_bookmarked_site_keeps_the_bar_buttons` (same button object before/after
   another page of the site changed the stored icon; it still shows a favicon).
3. Otherwise it rebuilds all buttons, the empty label visibility and the Other button.

### 1.3 Core contract

| Call | Decides |
|---|---|
| `Profile::bookmarks() -> bookmarks::Bookmarks<'_>` | handle |
| `Bookmarks::children(BookmarkId::TOOLBAR / OTHER / folder) -> Vec<BookmarkNode>` | items in display order (effective tree after CRDT materialization) |
| `Bookmarks::get(BookmarkId::OTHER / TOOLBAR) -> Option<BookmarkNode>` | root nodes (titles `Bookmarks bar`, `Other bookmarks`, `Mobile bookmarks` from `root_title`) |
| `bookmarks::{BookmarkId, BookmarkNode, NodeKind}` | `BookmarkNode { id, kind, parent, index, title, url: Option<Url>, added_ms }`; `NodeKind::{Folder, Url, Separator}`; roots `ROOT(1) TOOLBAR(2) OTHER(3) MOBILE(4)`, `is_root()`, `is_visible_root()` |
| `Profile::favicons().get(&url)` via `favicons::stored` | icon of the page or else its site (§6) |
| `Profile::prefs().get/set(&keys::SHOW_BOOKMARKS_BAR)` (through `Browser::switch/set_pref`) | visibility |

Note: the bar's own label for the Other root is the literal `Other Bookmarks` (capital B),
while core's root title is `Other bookmarks` (used in the editor's folder list).

### 1.4 Persistent state

- Pref `bookmarks_bar.visible` (`keys::SHOW_BOOKMARKS_BAR`, `Scope::Synced`, default `true`).
- Bookmarks themselves: core SQLite `bookmarks` table (synced CRDT records, never written by
  the bar except through drag/menus).
- No session-file fields.

### 1.5 Engine classification

| Piece | Class |
|---|---|
| Everything in the bar (layout, overflow, icons, clicks, menus, drag) | KEEP (no engine involvement) |
| Opening a bookmark in the tab | KEEP; load via the tab's CEF browser (`CefFrame::LoadURL` on the main frame, inference) with history transition `Bookmark` recorded by the shell |
| Drawing the bar's menus over web content | NEEDS-CEF consideration: if CEF is embedded as a native child window, GPUI-drawn overlays cannot appear above it (inference; see Risks) |

### 1.6 Interactions and tests

- `BrowserWindow::refresh_bookmarks_bar`, `set_bookmarks_bar_visible`, `bookmarks_bar()`
  (test accessor) in `window/mod.rs`.
- Test-only accessors used by self-test/unit tests: `button_for(url)` (finds the button whose
  tooltip is the URL), `overflow() -> (shown, hidden)`, `chevron()`, `item(index)`,
  `shows_favicon(url)` (the button's image is a paintable rather than the generic icon name).
- Self-test checks: `favicon_preload`, `bookmark`, `bookmarks_bar_menus` (§9).

---

## 2. Bookmark folder menus (`bookmark_menu.rs`, menus part)

### 2.1 What the user sees and does

Opened from: a bar folder button (below it), the chevron (below it, hidden items), `Other
Bookmarks` (below it), and a folder row inside a menu (beside it: to the **right** in LTR, to
the **left** in RTL). GTK model menus were avoided because they hide icons next to labels, so
menus are popovers of rows (KEEP the look: icon + label rows).

**Menu frame.** No arrow; CSS `menu`, `bookmark-menu` (4 px padding). Starts at its button's
start edge; a submenu starts at its row's top (`halign/valign Start`). The list scrolls
vertically (never horizontally) once it would exceed **75% of the monitor's height**
(`MAX_HEIGHT_FRACTION`; 800 px monitor assumed if unknown).

**Rows** (`row_button`): flat button, CSS `bookmark-menu-row` (padding 4px 8px, min-height 24,
normal weight), 16 px icon, 8 px spacing, label left-aligned, expanding, ellipsized at the end at
**40 characters** (`MAX_MENU_LABEL_CHARS`), single line; accessible label = the row's text.

| Row | Icon | Label | Extra | Tooltip |
|---|---|---|---|---|
| Link | stored favicon, else `web-browser-symbolic` | title, or the URL's host when the title is blank, or the whole URL when there is no host (`label_for`) | — | `"{title}\n{url}"` |
| Folder | `folder-symbolic` | folder title | trailing `go-next-symbolic` arrow | — |
| Separator | horizontal line, 3 px margins top/bottom | | | |
| Empty folder | `folder-symbolic` | `(Empty)` | insensitive | — |

**Link row actions.** Click: open in the current tab (`win.open-bookmark`, §1.1) and close
**every** bookmark menu it is in (innermost first). Middle-click: open in a new foreground tab
(`win.open-in-new-tab`) and close all menus. Hovering a link row closes the submenu a sibling
folder row opened. Right-click / Menu key / Shift+F10: the link's context menu (§3).

**Folder row actions.** Hover: opens its submenu (without moving focus). Click: opens the
submenu and moves focus to its first row. **Right** arrow (Left in RTL): opens the submenu and
focuses it. At most one submenu per menu: opening another closes the previous; hovering a link
closes it; closing a menu closes its open submenu. In a submenu, **Left** arrow (Right in RTL)
closes that submenu (back). Escape closes the menu (GTK default; inference). Right-click etc.:
the folder's context menu.

**Depth limit.** Menus show folders up to 12 levels below the folder being collected
(`bookmark_menu::MAX_DEPTH = 12`, separate from core's `bookmarks::MAX_DEPTH = 64`). A folder
deeper than that shows as `(Empty)` (its children were not copied). Deleting such a folder still
asks first because the count is read from core, not from the copy (unit test
`deleting_a_folder_nested_past_what_menus_show_still_asks_first`).

**Icons** are read from core each time a menu is built (one `favicons::stored` per link row).

### 2.2 Core contract

- `bookmark_menu::collect(&Bookmarks, folder) -> Vec<Item>` (shell helper): recursively
  `Bookmarks::children`, dropping URL nodes without a URL, stopping at depth 12.
  `Item { node: BookmarkNode, children: Rc<[Item]> }` is a snapshot so widgets are built without
  holding a core borrow.
- `favicons::stored(profile, url)` → `Profile::favicons().get(url)`.

### 2.3 Persistent state

None beyond bookmarks/favicons read.

### 2.4 Engine classification

All KEEP. Menu windows/overlays over web content: see Risks.

### 2.5 Interactions and tests

- `bookmark_menu::popup(anchor, items, side) -> Option<gtk::Popover>` used by the bar (folder
  buttons, chevron, Other) and recursively by folder rows. Returns `None` if the anchor is not in
  a `BrowserWindow`.
- `open_on_click(button, url, after)` used by bar URL buttons and menu link rows.
- Self-test `bookmarks_bar_menus` (§9) opens the chevron menu, a folder menu, an inner submenu,
  checks favicons and ellipsizing, and screenshots them.

---

## 3. Bookmark context menus (`bookmark_menu.rs`, context part)

### 3.1 What the user sees and does

Triggers (`attach_context_menu`): secondary click (claimed, so the strip's own menu does not
also open), or **Menu** key, or **Shift+F10** while the widget has focus. Attached to: every bar
item, the bar strip (`Target::Bar`), `Other Bookmarks`, every link row and folder row in menus.
Positioned at the pointer (click) or at the bottom-center of the widget (keyboard), below, no
arrow (`gtk::PopoverMenu` from a `gio::Menu`, i.e. a plain text menu with mnemonics).

Menu contents (sections separated by separators; `_` marks the mnemonic):

**Link** (`NodeKind::Url`):
1. `Open in New _Tab` · `Open in New _Window`
2. `_Edit…` · `_Copy Link` · `_Delete`
3. `Add _Page…` · `Add _Folder…`
4. `Show Bookmarks _Bar` (checkmark = current visibility) · `Bookmark _Manager`

**Separator**:
1. `_Delete`
2. `Add _Page…` · `Add _Folder…`
3. `Show Bookmarks _Bar` · `Bookmark _Manager`

(No Rename/Open All for separators: unit test `a_separator_can_only_be_deleted`.)

**Folder** (including the Other root via `Other Bookmarks`):
1. `Open _All ({n}) in New Tabs`, where n = the folder's **direct** links (not subfolders'), as
   Chrome; disabled when n = 0
2. `_Rename…` · `_Delete` (both disabled for roots)
3. `Add _Page…` · `Add _Folder…`
4. `Show Bookmarks _Bar` · `Bookmark _Manager`

**Bar empty space**:
1. `Add _Page…` · `Add _Folder…`
2. `Show Bookmarks _Bar` · `Bookmark _Manager`

Every action first closes all bookmark menus the anchor is in, then runs:

| Action | Behavior |
|---|---|
| Open in New Tab | `open_tab(url, None, Focus::Foreground)` |
| Open in New Window | `Browser::open_window(window.browsing(), [url])`: a new window **of the same kind** (a private window's menu opens a private window), presented |
| Copy Link | clipboard text = the URL string |
| Edit… / Rename… | editor dialog (§4.3) on the node: `Edit Bookmark` for a link, `Rename Folder` for a folder |
| Delete | link/separator: removed at once. Folder **with children**: confirm dialog first — heading `Delete Folder?`, body `“{title}” and the {count} items in it will be deleted.` where count = the folder's **direct** children counted in core, buttons `_Cancel` (default, close response) and `_Delete` (destructive). Empty folder: removed at once. Core `remove` tombstones the whole subtree. |
| Open All (n) in New Tabs | `open_tabs(links)`: first link foreground, the rest background, appended |
| Add Page… | editor dialog in "Add Page" mode (§4.3), prefilled with the selected tab's display title and committed URI (empty when the tab is at `about:blank`) |
| Add Folder… | text prompt: heading `Add Folder`, entry prefilled `New Folder`, buttons `_Cancel` / `_Add` (suggested, default); empty (after trim) or cancelled does nothing; else `add_folder(parent, InsertAt::End, title)` |
| Show Bookmarks Bar | `win.show-bookmarks-bar` toggle (§1.1) |
| Bookmark Manager | `win.show-bookmarks` (opens the Bookmarks window; default chord **Ctrl+Shift+O**) |

**Where Add Page/Add Folder put the new node** (`parent`): into the folder itself when the menu is
on a folder; into the node's parent when on a link or separator; into the bookmarks bar
(`TOOLBAR`) when on the bar's empty space. Always at the **end** of that parent
(`InsertAt::End`), not next to the clicked item (the code comment says "next to a link" but the
insert is `End`).

After each write: `bookmarks_changed()`; errors toast (§0.2).

### 3.2 Core contract

| Call | Use |
|---|---|
| `Bookmarks::children(id).len()` | delete confirmation count |
| `Bookmarks::remove(id)` | delete (tombstones subtree) |
| `Bookmarks::add_folder(parent, InsertAt::End, title)` | Add Folder |
| editor's calls (§4) | Edit/Rename/Add Page |
| `BookmarkId::is_root()` | disables Rename/Delete on roots |

### 3.3 Persistent state

Bookmarks; pref `bookmarks_bar.visible` through the toggle.

### 3.4 Engine classification

All KEEP. "Add Page…" reads the selected tab's title and committed URI: NEEDS-CEF only in that
the tab's title/URL now come from CEF (`CefDisplayHandler::OnTitleChange`,
`CefLoadHandler`/`OnAddressChange`; inference about which handler the tab module uses).

### 3.5 Interactions and tests

- `open_context_menu(widget, target, at) -> gtk::PopoverMenu` is called directly by the
  self-test `bookmarks_bar_menus` (opens a link's menu and activates `bookmark.edit`).
- Unit tests: `deleting_a_folder_nested_past_what_menus_show_still_asks_first`,
  `a_separator_can_only_be_deleted`, `labels_fall_back_to_the_host`.

---

## 4. Star, star bubble, and the bookmark editor dialog (`bookmark_editor.rs` + star flow)

### 4.1 The star (in `browser.rs`, `window/mod.rs`, `address_bar.rs`; KEEP)

- The star is the address entry's end icon: `non-starred-symbolic` with tooltip `Bookmark This
  Page`, or `starred-symbolic` with tooltip `Edit Bookmark` when the selected tab's committed URL
  is bookmarked (`AddressBar::set_starred`). `sync_star()` runs on tab selection, on commit of
  the selected tab, and after every bookmark change. It also sets the state of the stateful action
  `win.bookmark-page` (used by the self-test as "star active").
- Clicking the star, or `win.bookmark-page` (default chord **Ctrl+D**, core
  `Command::BookmarkPage`, "Bookmark this page"), runs `Browser::star_clicked`:
  1. needs a selected tab with a committed URI that parses as a URL (else nothing);
  2. if any bookmark has that exact URL (`Bookmarks::find_by_url`), opens the **"Edit bookmark"**
     bubble on the first one;
  3. else adds it: `add_url(BookmarkId::TOOLBAR, InsertAt::End, tab.display_title(), url)`, then
     stores the tab's current favicon (the icon arrived before the page was bookmarked, §6.2),
     then `bookmarks_changed()`, then opens the **"Bookmark added"** bubble.
- `display_title()` is the page title, else the display form of the URI, else `New Tab`.
- Observation (inference): the new tab page is committed at `about:blank`, so the star there
  would bookmark `about:blank` titled `New Tab`; nothing in the code prevents it.
- Only one bubble at a time opens from the address bar (star, zoom, site info): opening one
  closes the previous; while any is open, permission prompts wait (`bubble-changed` →
  `sync_permission_prompt`).

### 4.2 The bubble (`bookmark_editor::bubble`)

Anchored to the star icon, below it, width 340 px, 6 px margins, 12 px vertical spacing, CSS
`bookmark-bubble`:

```
[ Bookmark added | Edit bookmark ]                       (x)      ← heading style; flat circular close, tooltip "Close"
  Name    [ title.......................... ]
  URL     [ https://….......................]           ← only for URL bookmarks (always here)
  Folder  [ Bookmarks bar                 ▾ ]
                                     [ _Remove ] [ _Done ]   ← Done is the suggested (default) action
```

Behavior:

- On open (after the popover has focus) the **Name** field is focused with all its text
  selected (the self-test checks the selection).
- **Enter** in Name or URL activates the default (**Done**) when it is enabled.
- **Done**: validates, saves through core (§4.4), `bookmarks_changed()`, closes. Disabled while
  the form is invalid (§4.3).
- **Remove**: removes **every** bookmark whose URL equals the bubble node's URL
  (`find_by_url` → `remove` each), ignoring any edits in the form, `bookmarks_changed()`, closes.
- **Close (x)**, Escape, clicking elsewhere: closes and keeps the bookmark as it was saved (the
  star already added it; edits are discarded).
- Headings: `Bookmark added` when the star just added it, `Edit bookmark` otherwise (lower-case
  b; the dialog headings below are title case).

### 4.3 The form and the Edit…/Rename…/Add Page… dialog (`bookmark_editor::edit`)

**Form** (`Form::new`), a 2-column grid (row spacing 8, column spacing 12), right-aligned labels
with mnemonics:

| Row | Label | Widget | Present for |
|---|---|---|---|
| 0 | `_Name` | text entry, Enter activates default | always |
| 1 | `_URL` | text entry, input purpose URL, no spellcheck, Enter activates default | URL bookmarks and Add Page; not folders |
| 2 | `_Folder` | dropdown of folders | always |

**Folder dropdown** (`folder_choices`): every folder in tree order under each visible root
(`children(BookmarkId::ROOT)` = Bookmarks bar, Other bookmarks, Mobile bookmarks), each indented
16 px per depth level in the popup list (`FOLDER_INDENT`), at most core `MAX_DEPTH` (64) levels
below a root (unit test `folder_choices_stop_at_the_maximum_depth`: a 200-deep chain lists 64
and fits a 256 KiB stack). When editing a **folder**, that folder and its subtree are excluded
(cannot move into itself). **Mobile bookmarks** is listed only if it has children or is where
the bookmark currently is. The current parent is preselected (else the first entry).

**Validation** (`validate`, re-run on every change of Name or URL):

- URL text must classify as a URL the address bar would open
  (`vsesvit_core::search::classify_url(text)` → `NavTarget::url()`): trimmed; schemes `http`,
  `https` (with a host), `file`, `about`, `data`, `view-source`, `vsesvit`; absolute file paths
  become `file:` URLs; a bare host gains `https://` (`example.com` → `https://example.com/`).
  Rejected examples (unit test `urls_must_parse`): `ftp://example.com/`, `not a url`, empty.
  Accepted: `about:blank`, `  https://example.com/a  `.
- Invalid URL: the URL entry gets the `error` style and saving is disabled.
- A folder needs a non-empty (trimmed) name; a link may have an empty name (it then shows its
  icon only on the bar and its host in menus).
- The name is saved trimmed.
- Consequence: `chrome-extension://`, `chrome://`, `javascript:` URLs cannot be saved through
  the editor (classify_url rejects them). Import already drops `javascript:` bookmarks
  (`import.rs` header). No bookmarklet support exists.

**Dialog** (`edit(parent, core, subject) -> Result<bool>`), an `adw::AlertDialog` with the form
as extra child (form width 360 px):

| Subject | Heading |
|---|---|
| existing folder | `Rename Folder` (Name + Folder rows; can also move it) |
| existing link | `Edit Bookmark` |
| new link (`Subject::New { parent, title, url }`) | `Add Page` |

Buttons `_Cancel` (close response) and `_Save` (suggested, default; disabled while invalid).
Name focused with text selected when it opens. Returns whether it saved. Used by the bar's
context menus (Edit…, Rename…, Add Page…) and by the Bookmarks window's Edit
(`dialogs/bookmarks.rs:457`, existing nodes only).

### 4.4 Save semantics (`save`, KEEP exactly)

- Existing node: `rename(id, title)` only if the title changed; `set_url(id, url)` only if the URL
  changed (links only); `move_to(id, parent, InsertAt::End)` only if the folder changed (it goes
  to the **end** of the new folder). Each core call is a no-op write when unchanged (core skips
  stamping), but the shell also skips them.
- New: `add_url(parent, InsertAt::End, title, url)`.

### 4.5 Core contract

`bookmarks::{BookmarkId, BookmarkNode, Bookmarks, InsertAt, MAX_DEPTH, NodeKind}`,
`Bookmarks::{children, find_by_url, add_url, rename, set_url, move_to, remove}`,
`search::classify_url`. Root titles come from `Bookmarks::get/children` (`Bookmarks bar`,
`Other bookmarks`, `Mobile bookmarks`).

### 4.6 Persistent state

Bookmarks only.

### 4.7 Engine classification

| Piece | Class |
|---|---|
| Star, bubble, dialog, validation, save | KEEP |
| Star's URL/title source (committed URI, page title) | NEEDS-CEF: from CEF's address/title notifications (`CefDisplayHandler::OnAddressChange`/`OnTitleChange`, inference) |
| Chrome's own Ctrl+D / bookmark star in Chrome-style browsers | NEEDS-CEF: make sure Chrome's command does not run instead of Vsesvit's. CEF offers `CefCommandHandler::OnChromeCommand` (Chrome style only; return true when handled) and the bindings define `IDC_BOOKMARK_THIS_TAB = 35000`, `IDC_BOOKMARK_ALL_TABS = 35001`, `IDC_SHOW_BOOKMARK_BAR = 40009`, `IDC_SHOW_BOOKMARK_MANAGER = 40011`, `IDC_SHOW_DOWNLOADS = 40012` (the docs say to map names with `cef_id_for_command_id_name` for version safety); `CefCommandHandler::IsChromePageActionIconVisible` with `CEF_CPAIT_BOOKMARK_STAR` hides Chrome's own star if Chrome UI is shown. Whether Chrome accelerators reach these handlers in the chosen embedding is an open question. |

### 4.8 Interactions and tests

- `BrowserWindow::show_bookmark_bubble(node, added)` → `AddressBar::show_popover(&bubble,
  Anchor::Star)`.
- Unit tests: `urls_must_parse`, `the_form_saves_name_url_and_folder_through_core`,
  `folder_choices_stop_at_the_maximum_depth`; `address_bar.rs`
  `the_star_bookmarks_the_page_and_opens_its_bubble`.
- Self-test `bookmark` and `star_bubble` (§9).

---

## 5. Bookmark drag-and-drop (`bookmark_drag.rs`)

### 5.1 What the user sees and does

Bookmarks can be dragged **on the bookmarks bar** (between items, into folders, into `Other
Bookmarks`, to the end of the bar) and **in the Bookmarks window's tree** (between rows, into
folders), and between the two (the payload is a string, so it crosses windows; KEEP if
feasible). Not supported: dragging rows inside bookmark menus, dragging hidden (overflow) items,
dropping web links or text from pages/other apps to create bookmarks (inference: the drop
target accepts only a string that parses as a bookmark UUID).

- **Drag start**: on any bar item (MOVE only; captured before the button's own click handling).
  The drag icon is a snapshot of the widget, held at the pointer's offset. Roots refuse to be
  dragged (only matters in the Bookmarks window; bar items are never roots).
- **Payload**: the dragged node's id as text (`BookmarkId.0.to_string()`, a UUID string). Side
  effect (inference): dragging a bookmark out to another app drops that UUID as text.
- **Hover feedback** while over a target, by zone (§5.2):
  - before: a 2 px accent line on the target's leading edge (left in LTR on the bar; top in the
    Bookmarks window);
  - after: a 2 px accent line on the trailing edge (right on the bar; bottom in the window);
  - into: the target gets a 25%-accent background with 6 px radius.
  Feedback clears on leave and on drop.
- **Drop**: computes the destination (§5.3), runs one core `move_to`, then (deferred to idle so
  the widgets involved may be rebuilt) `bookmarks_changed()` on success; a no-op drop does
  nothing; a failing move toasts `Cannot move the bookmark: {e}` on the bar (the Bookmarks window
  reports through its own toast).

Drop targets on the bar: each item (its own node); the row's empty space (target = the
`TOOLBAR` root, read live, so the drop appends to the bar); `Other Bookmarks` (target = the Other
root, so the drop appends into Other bookmarks).

### 5.2 Zones (`zone`, pure, KEEP exactly)

`at = offset / extent` along the axis the target's siblings run in (x on the bar, mirrored in
RTL as `width - x`; y in the Bookmarks window); `0.5` if extent is 0.

- Folder that is a root (`id.is_root()`): always **Into**.
- Other folder: **Into** for `0.25 <= at < 0.75`, else Before (< 0.5) / After.
- Link or separator: **Before** for `at < 0.5`, else **After**.

Unit test `zones_split_leaves_in_halves_and_folders_in_quarters`.

### 5.3 Destination (`destination`, pure, KEEP exactly)

```
if dragged.id == target.id: no move
Into:   (target.id, InsertAt::End)
Before: gap = target.index;  After: gap = target.index + 1
same_parent = dragged.parent == target.parent
index = gap - 1 if same_parent && dragged.index < gap else gap
if same_parent && index == dragged.index: no move
else (target.parent, InsertAt::Index(index))
```

Core counts the index among the parent's children **without** the moved node, hence the `-1`.
Unit tests `indices_count_siblings_without_the_moved_node` (for `[A,B,C,D]`: D before A → 0; A
after C → 2; A after D → 3; A before C → 1; C after A → 1; B after A → none; B before C → none)
and `drops_into_other_folders_keep_the_gap`.

`apply(core, id, target, zone) -> Result<bool>`: re-reads the dragged node from core
(`Bookmarks::get`), computes the destination, calls `Bookmarks::move_to(id, parent, at)`;
`Ok(false)` when nothing moved. Core rejects cycles (`WouldCycle`), non-folders, roots; core
also treats "dropped back on its own slot" as a no-op that writes nothing.

### 5.4 Core contract

`Bookmarks::{get, move_to}`, `InsertAt::{End, Index}`, `BookmarkNode.{id, parent, index, kind}`,
`BookmarkId::is_root`, `NodeKind`.

### 5.5 Persistent state

Bookmarks (a move writes one placement register).

### 5.6 Engine classification

KEEP (no engine). GPUI note (verified in `/tmp/vsesvit-research/gpui-crate/gpui-0.2.2`):
`div` has `on_drag`, `on_drop`, `drag_over`, `can_drop`, `on_drag_move`; the active drag lives on
`App`. Whether a GPUI drag can cross between two GPUI windows (bar → Bookmarks window) is not
verified.

### 5.7 Interactions and tests

- `dialogs/bookmarks.rs` uses `drag_source`, `drop_target(Vertical, …)`, `apply`, `Zone`
  (`drop_on`, lines 305-312, 612-625) and has a unit test exercising Before/After/Into, a folder
  into its own child (`WouldCycle`), and roots not draggable.

---

## 6. Favicons (`favicons.rs` + `browser.rs` recording and preload)

### 6.1 What the user sees

- Bookmark links on the bar, in bookmark menus, in the Bookmarks window, and the "other
  devices' tabs" rows in History (`dialogs/history.rs:214`) show the **stored** favicon of the
  page, or of its site, else the generic `web-browser-symbolic` icon.
- A bookmarked site gets its icon **without being visited** (imported or synced bookmarks): a
  background fetch (§6.4).
- Icons update in place on the bar as pages load (§1.2).
- A **sleeping tab** (Memory Saver) shows its favicon at **half opacity** (`favicons::faded`,
  premultiplied channels halved; used by `tab.rs:591`). The memory_saver self-test checks the
  alpha halves.

### 6.2 Recording (KEEP; NEEDS-CEF for the icon source)

`Browser::favicon_changed(tab)` runs when a tab's favicon changes (`TabChange::Favicon` from
WebKit's `favicon` property notify). `save_favicon(tab)`:

- only for **normal** (non-private) tabs, with a committed URI and an icon (private tabs never
  store icons);
- `favicons::record(profile, uri, texture)`: parse the URI; **ask core first**
  `Profile::favicons().wanted(&url)` (true when the page or any page of its origin is
  bookmarked) so unbookmarked pages cost no scaling or encoding; scale down to fit **32×32**
  (bilinear, aspect kept, each side ≥ 1 px) if larger; encode PNG; `favicons().record(&url,
  &png) -> Result<bool>` (true when the stored icon changed);
- if changed → `bookmarks_changed()` (repaint bars/views).

Also run right after the star adds a bookmark (the icon arrived before the bookmark existed).

Core rules (`favicons/mod.rs`, KEEP via core): keeps an icon only when the page or its site is
bookmarked; a page that is not itself bookmarked stands in for its site (one row keyed by the
bare origin); empty PNGs or PNGs over 64 KiB (`MAX_BYTES`) are ignored; storing the identical
PNG is a no-op; icons of sites with no bookmark left are pruned when the profile opens
(`Favicons::prune`, called in `Profile::open`). Unit test
`a_large_icon_is_kept_at_the_size_the_bookmarks_show` (64 px and 512 px icons are stored at
32×32).

**NEEDS-CEF**: Chromium does not hand over a decoded favicon. The likely path (verified to exist
in cef 154): `CefDisplayHandler::OnFaviconURLChange(browser, icon_urls)` (CEF reports only
`kFavicon`-type candidates, and only when the list is non-empty: `libcef/browser/
browser_contents_delegate.cc:621-637`) → `CefBrowserHost::DownloadImage(image_url,
is_favicon=1 (no cookies), max_image_size, bypass_cache=0, CefDownloadImageCallback)` →
`OnDownloadImageFinished(image_url, http_status_code, image: CefImage)` →
`CefImage::GetAsPNG(scale_factor, with_transparency=1, &w, &h) -> CefBinaryValue`. CEF filters
results larger than `max_image_size` DIP and resizes the smallest if none fit (doc comment), so
asking for 32 yields ≤ 32 DIP. Core's PNG helpers (`favicons::{decode_image, rgba_png}`) are
`pub(crate)`, so the shell still needs its own scaling/encoding or a core change. Whether Blink
reports the implicit `/favicon.ico` when a page declares no icon is an open question (WebKitGTK
fetched it itself).

### 6.3 Reading (`favicons::stored`, KEEP)

`Profile::favicons().get(&url) -> Result<Option<Vec<u8>>>`: the page's own icon, else the newest
icon of its origin; errors are logged and treated as no icon. The PNG is decoded into a texture
(GPUI: `gpui::Image::from_bytes(ImageFormat::Png, bytes)`, verified to exist in gpui 0.2.2).

### 6.4 Preload of missing icons (in `browser.rs`, KEEP)

`Browser::preload_favicons()` (runs at the end of every `bookmarks_changed()`):

- state machine `FetchState::{Idle, Running, RunningStale}`: if a fetch is running, mark it stale
  and return;
- `Profile::favicons().missing(32)` (`FAVICON_BATCH = 32`): bookmarked http(s) pages with no
  stored icon for the page or its site, one per origin, bar items first then other roots then
  deeper folders, skipping origins that failed within 7 days (or within 1 hour when unreachable);
- if any: `vsesvit_core::favicons::FaviconFetch::new(pages).run()` on a worker thread (blocking,
  6 workers, public addresses only, 10 s timeout, size-capped, returns `Vec<Fetched>`), then on
  the UI thread `favicons().commit_fetched(results) -> Result<bool>`;
- if anything was stored → `bookmarks_changed()` (which looks for the next batch); else if the
  run went stale → run again.
- The self-test calls `vsesvit_core::favicons::allow_local_hosts()` so the fixture server's
  loopback page can be fetched.

This is engine-independent (core does the HTTP); KEEP unchanged.

### 6.5 Persistent state

Core SQLite, LOCAL (never synced): `favicons(page_url PK, origin, png, updated_ms)` and
`favicon_failures(origin PK, failed_ms)`.

### 6.6 Engine classification summary

| Piece | Class |
|---|---|
| `stored`, `show`/`image` fallback | KEEP |
| `record` scaling to 32 px + PNG + `wanted` pre-check | KEEP (implementation changes: CEF gives PNG/bitmaps) |
| WebKit `WebView::favicon()` texture and its notify | WEBKIT-ONLY; replaced by NEEDS-CEF `OnFaviconURLChange` + `DownloadImage` |
| `faded` (half opacity) | KEEP; in GPUI simply `Styled::opacity(0.5)` on the image (verified to exist) |
| Preload via `FaviconFetch` | KEEP (core) |

### 6.7 Interactions and tests

- Unit tests in `browser.rs`: `bookmarking_a_page_keeps_its_favicon_for_the_bar`,
  `visiting_more_of_a_bookmarked_site_keeps_the_bar_buttons`,
  `bookmarked_sites_get_their_icons_without_being_visited`.
- Self-test `favicon_preload` and `bookmarks_bar_menus` (favicons in a folder menu),
  `memory_saver` (faded icon). `docs/design/self-test.md` lists a Windows-only
  `bookmarks_bar_icons` check (a page changing its favicon four times updates the bar item in
  place) that Linux has no self-test for.

---

## 7. Downloads controller (`downloads.rs`)

### 7.1 What the user sees and does

- **Downloads button** in the header (icon `folder-download-symbolic`, tooltip `Downloads`,
  action `win.show-downloads`, default chord **Ctrl+J**), **hidden until a download that the
  window lists starts in this run**, then visible for the rest of the run (never re-hidden, even
  after Clear List). A window opened later shows it if such a download already started
  (`started_this_session(browsing)`). No progress or activity indication on the button (Windows
  has a busy indicator; Linux does not; PLAN.md: "The toolbar buttons show no progress text in
  either shell").
- **Toast when a download starts**: `Downloading “{file name}”` with a `Show` button that runs
  `win.show-downloads`.
- **Toast when it completes**: `“{file name}” downloaded` with an `Open` button that opens the
  file with the default application.
- **Toast when it fails** (not when cancelled): `Download of “{file name}” failed` (file name
  from the destination, or `file`).
- **Toast when it cannot be placed**: `Cannot save to {dir}: {error}` (download folder cannot be
  created) or `Cannot save the download: its path is not valid UTF-8`; the download is cancelled
  and **not** listed (unit test `a_download_into_a_folder_that_cannot_be_made_says_why`).
- Toasts go to the window of the tab that started the download, else the most recently used
  window that lists that kind of download (`window_for`).
- **Where files go**: straight into the download folder under a free name, **never overwriting**:
  `name`, `name (1).ext`, `name (2).ext`, … (core `unique_destination`; a name whose
  `.unconfirmed` sibling exists also counts as taken; two downloads starting at once never get
  the same name, via an in-memory reserved set; unit test
  `two_downloads_with_the_same_name_get_files_of_their_own` → `big.bin` and `big (1).bin`).
  The folder is created if missing.
- **Ask where to save** (pref on): a save dialog titled `Save File`, starting in the download
  folder with the sanitized suggested name; the dialog itself asks before replacing, so the
  chosen path may overwrite; cancelling the dialog cancels the download (nothing listed, no
  toast).
- **Files that can run code** (core `is_dangerous(name, mime)`; on Linux `.sh .bash .csh .ksh
  .tcsh .zsh .deb .rpm .run .desktop .appimage .flatpak .flatpakref .snap`, `.jar .jnlp`, and the
  matching MIME types such as `application/x-sh`, `application/x-executable`) are written beside
  their destination as `<name>.unconfirmed` and, once complete, wait as **Unconfirmed**. The
  window shows Chrome's **warning popover under the downloads button**:

  ```
  [ dangerous.sh                                   (x) ]   ← file name, heading style, ellipsized in the middle; close tooltip "Close"
    This type of file can harm your device                ← core status_line for Unconfirmed
                                   [ _Keep ] [ _Discard ]  ← Discard is the suggested-action button
  ```

  Width 300 px. It opens on its own and **takes no keyboard focus and does not autohide**, so a
  key press while the user types cannot answer it. **Keep** moves the file to its real name
  (numbered if that name was taken meanwhile; a hard link claims the name so nothing is ever
  replaced) and lists it Completed (no toast). **Discard** deletes the `.unconfirmed` file and
  the entry. (x) just closes the warning; the entry keeps waiting (the Downloads window also
  offers Keep/Discard). An unconfirmed file still waits after a restart. No "completed" toast is
  shown for an unconfirmed file. Unit test
  `a_script_waits_under_its_unconfirmed_name_until_kept_or_discarded`; a second download of the
  same script goes to `run (1).sh` because the kept file has the name.
- **No pause/resume on Linux** today ("WebKitGTK cannot pause a download"); Windows pauses,
  resumes, and resumes interrupted downloads (PLAN.md § Downloads).
- **Private windows**: their downloads are listed only in private windows (a private window
  lists normal ones too); when the last private window closes, its running downloads are
  cancelled (files stay) and core forgets the private rows, deleting private files still waiting
  to be kept.
- **Restart**: any download left in progress/paused/interrupted by the last run reads **Failed**
  at startup (core `interrupt_stale`, run before any download can begin).
- **Opening files**: `open(window, path)` opens a file (or a folder) with the default app;
  `show_in_folder(window, path)` opens the containing folder with the file selected where the
  file manager allows (GTK `FileLauncher::launch` / `open_containing_folder`, i.e. the portal).
  Used by the completed toast (Open) and the Downloads window (open, show in folder, Open
  Download Folder).

### 7.2 Controller API the Downloads window and others consume (KEEP as a contract)

`Downloads` (one per profile, `Rc`, in `Browser`), created by `Downloads::new(app, core,
engine.session(), profile::downloads_dir())`:

| Method | Behavior |
|---|---|
| `new(...)` | `core.downloads().interrupt_stale()`, then watches the normal engine session |
| `watch(session, browsing) -> SignalHandlerId` | takes downloads a session starts; used again for the private session's ephemeral session (`browser.rs:345`), disconnected when it ends |
| `cancel_private()` | cancel every running private download, reset the private "started" flag |
| `directory() -> PathBuf` | pref `downloads.directory`, else the platform Downloads folder (`glib::user_special_dir(Downloads)` or `~/Downloads`); read per download so a change applies to the next one |
| `download_to(view, uri, path)` | download `uri` into `path` already chosen by a save dialog (Save Page As for non-HTML documents, `save_page.rs:77`): remembers `(uri, path)`, starts the engine download, and the next started download whose request URI equals `uri` goes to `path` (overwrite allowed; still held if dangerous) |
| `started_this_session(browsing) -> bool` | normal started, or private started and the window lists private |
| `list(browsing) -> Vec<Download>` | core list, newest first, limit **500** (Windows: 200) |
| `progress(id) -> Option<Progress>` | live `Transfer::at(now)` while the engine holds it |
| `cancel(id)` | engine cancel |
| `remove(id)` | entry off the list, file stays; notify `List` |
| `keep(id)` / `discard(id)` | core keep/discard; notify `List` |
| `clear(browsing)` | remove every final (Completed/Failed/Cancelled) entry that kind of window lists; files stay; notify `List` |
| `subscribe(f) -> Subscription` / `unsubscribe(token)` | change feed: `Change::List` (added, finished, removed) or `Change::Progress(id)` (byte count moved; at most every **250 ms** per download) |
| `warning(&Rc<Downloads>, &Download) -> gtk::Popover` | the warning above |
| `file_name(path)`, `open(window, path)`, `show_in_folder(window, path)` | helpers |

The Downloads window (`dialogs/downloads.rs`, outside this scope) refreshes rows every 1 s while
open (`TICK`) so a stalled download's speed falls to `0 B/s` within 10 s, and shows rows with
`status_line(&download, progress, exists)`.

### 7.3 Engine-side flow in the GTK shell (what the new shell must re-create on CEF)

Per engine download (`track`), phases `Deciding → Running(id) → Ended`:

1. **Start** (WebKit `NetworkSession::download-started`): private downloads are remembered for
   `cancel_private`. If `download_to` armed a path for this exact request URI, take it.
2. **Decide destination** (WebKit `decide-destination(suggested)`):
   - armed path: allow overwrite, destination = `written(path)`;
   - else `decide_destination`: `create_dir_all(directory())` or refuse with toast; if pref
     `downloads.ask` is off → `unique_destination(dir, suggested, exists || reserved)` →
     `written(...)` → set destination and reserve it; else the `Save File` dialog (above).
   - `written(path)`: if `is_dangerous(file name, response MIME)`: if `<path>.unconfirmed`
     exists, renumber with `unique_destination(dir, name, Path::exists)`; the engine writes to
     `unconfirmed_path(path)` and `path` is "held" as the final destination.
   - destination paths must be UTF-8 (WebKit API takes `&str`), else refuse.
3. **File created** (WebKit `created-destination`): un-reserve; `started()`:
   `core.downloads().start(url, held-or-destination, total, now_ms, browsing)`; keep a live
   `Transfer::new(now, total)`; set the started flag (normal or private); show the downloads
   button in every window that lists it; toast `Downloading “…”` (`Show`); notify `List`.
4. **Bytes** (WebKit `received-data`): `transfer.update(now, received, total)`; notify
   `Progress(id)` at most every 250 ms.
5. **Failed** (WebKit `failed`): `CancelledByUser` → `State::Cancelled` (no toast); else
   `State::Failed` + toast + warn log. Un-reserve.
6. **Finished** (WebKit `finished`, also emitted after `failed`; first wins): Completed, or
   Unconfirmed if held → `ended()` writes `core.downloads().update(id, state, received, total
   or received)` and notifies `List`; then either the warning (Unconfirmed, in the originating
   window, `BrowserWindow::warn_about_download`) or the completed toast.
7. `total` = response Content-Length when > 0, else unknown.

Navigations whose main resource MIME type WebKit cannot show are turned into downloads by the
tab (`tab.rs:983-993`, `decision.download()`), clearing the tab's pending history transition so
the next visit is not mis-attributed (unit test `a_download_leaves_no_transition_behind`).
WebKit's stock context-menu items (Download Linked File, Download Image, …) are kept and reach
the same controller (inference: `page_menu.rs` only adds items).

### 7.4 Core contract (`vsesvit_core::downloads`, all verified)

| API | Decides |
|---|---|
| `Downloads::start(url, path, total, now_ms, browsing) -> Download` | new row (SQLite rowid for normal; negative in-memory id for private) |
| `Downloads::update(id, state, received, total)` | stores progress/outcome; no-op if removed |
| `Downloads::list(limit, browsing)` | newest first; private rows merged for private windows |
| `Downloads::remove(id)` | entry off the list |
| `Downloads::keep(id) -> Option<PathBuf>` | moves `.unconfirmed` to a free name (hard link, rename fallback), Completed |
| `Downloads::discard(id)` | deletes `.unconfirmed` file + row |
| `Downloads::clear(browsing)` | removes final entries |
| `Downloads::interrupt_stale() -> usize` | live states → Failed at startup |
| `listed_in(started, window) -> bool` | normal downloads everywhere; private only in private windows |
| `unique_destination(dir, suggested, exists)` | free name, also avoiding `.unconfirmed` siblings |
| `unconfirmed_path(path)` | `<name>.unconfirmed` |
| `is_dangerous(name, mime)` | per-OS dangerous types (Linux set above) |
| `sanitize(suggested)` | plain file name, no dirs/control chars/leading dots, `download` if empty |
| `Transfer::{new, update, restart, at}` / `Progress { received, total, per_second }` | Chrome-like speed over 10 s |
| `status_line(&Download, Option<Progress>, exists)` | the row text: `"{speed} - {a} of {b}, {n} secs left"`, `"0 B/s - … , Paused"`, `"Interrupted · …"`, `"This type of file can harm your device"`, `"Deleted"`, `"{size} · {host}"`, `"Failed"`, `"Cancelled"` |
| `State::{InProgress, Paused, Interrupted, Unconfirmed, Completed, Failed, Cancelled}`, `is_live`, `is_final` | states |
| `DownloadId(i64)`, `DownloadId::browsing()` | id; negative = private |
| `private::Browsing::{Normal, Private}`, `Browsing::loggable` | private addresses/paths never logged |
| `Profile::end_private_session()` | forgets private rows, deletes private `.unconfirmed` files |
| `prefs::keys::{DOWNLOADS_DIR, DOWNLOADS_ASK}` | folder and ask switch |
| `downloads::zone_identifier` | Windows only (Mark of the Web); not used on Linux |

### 7.5 Persistent state

- Pref `downloads.directory` (`keys::DOWNLOADS_DIR`, `Option<PathBuf>`, **Local**, default `None`
  = platform Downloads folder). Set/reset in Settings (`Download Folder` dialog, `Change…`, reset).
- Pref `downloads.ask` (`keys::DOWNLOADS_ASK`, **Synced**, default `false`), Settings switch `Ask
  Where to Save Each File`.
- Core SQLite `downloads(id INTEGER PRIMARY KEY AUTOINCREMENT, url, path, started_ms, state,
  received, total)`, LOCAL. Private rows only in memory.
- Files: the downloaded files; `<name>.unconfirmed` files waiting for Keep/Discard.

### 7.6 Engine classification

| Piece | Class | CEF equivalent (verified API names in cef 154 unless marked) |
|---|---|---|
| Taking engine downloads per session | NEEDS-CEF | `CefClient::GetDownloadHandler` → `CefDownloadHandler` (per client; the private request context's browsers identify the kind) |
| `decide-destination` → path or dialog | NEEDS-CEF | `CefDownloadHandler::OnBeforeDownload(browser, item, suggested_name, callback)` returning true, then `CefBeforeDownloadCallback::Continue(path, show_dialog=0)`, possibly asynchronously (after a GPUI/portal save dialog; gpui 0.2.2 has `App::prompt_for_new_path(directory, suggested_name)`). Verified in CEF source (`download_manager_delegate_impl.cc`): when handled, CEF sets `target_path = intermediate_path = path` (no `.crdownload` intermediate) and skips Chrome's `DownloadTargetDeterminer` (no Chrome danger checks); CEF creates a missing directory itself and falls back to the temp dir if it cannot (so the shell must create/check the folder first to keep its "Cannot save to" refusal); dropping the callback without calling it cancels. `suggested_name` is computed by CEF with `net::GenerateFileName` (Content-Disposition, charset, MIME, default `download`). |
| `set_allow_overwrite` | NEEDS-CEF (open question) | no flag; behavior of Chromium when the given target exists is unverified. Core's unique naming plus the reserved set avoid collisions for automatic paths |
| `created-destination` (file exists, row starts) | NEEDS-CEF | first `CefDownloadHandler::OnDownloadUpdated` after `Continue` where `CefDownloadItem::GetFullPath()` is non-empty (CEF's own pause code treats an empty full path as "target pending") |
| `received-data` | NEEDS-CEF | `OnDownloadUpdated`: `GetReceivedBytes`, `GetTotalBytes` (unknown → treat ≤ 0 as `None`, inference), `GetCurrentSpeed` (unused: core `Transfer` keeps the Chrome-like speed so both shells read alike) |
| `failed` / `finished` | NEEDS-CEF | `OnDownloadUpdated`: `IsComplete`, `IsCanceled`, `IsInterrupted` + `GetInterruptReason` (`CEF_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED = 40`, `USER_SHUTDOWN = 41`, network/server/file reasons), `IsPaused`. Map like the Windows `list_state`: complete → Completed/Unconfirmed; user-cancelled → Cancelled; paused → Paused; resumable interruption → Interrupted; else Failed. CEF exposes no `CanResume` |
| cancel | NEEDS-CEF | `CefDownloadItemCallback::Cancel` (from `OnDownloadUpdated`; CEF's impl holds a weak manager pointer + id, so keeping the callback is safe; `Cancel` is one-shot) |
| "WebKitGTK cannot pause" | WEBKIT-ONLY | CEF has `CefDownloadItemCallback::Pause`/`Resume`; the product can now follow the Windows behavior (Paused/Interrupted states, Resume) |
| `finished` after `failed` double event | WEBKIT-ONLY | — |
| `refuse()` toast because "WebKit reports the cancel as the user's" | KEEP the toast; the rationale is WEBKIT-ONLY | after `Cancel`, CEF reports cancelled; the shell still must say why |
| UTF-8-only destination paths | NEEDS-CEF (same constraint, inference) | `Continue` takes a `CefString`; non-UTF-8 Linux paths cannot round-trip |
| `download_to` + `view.download_uri` | NEEDS-CEF | `CefBrowserHost::StartDownload(url)`; match in `OnBeforeDownload` by `GetOriginalUrl()`/`GetURL()` (inference). `CanDownload` is not called for `StartDownload` (doc) |
| Response-policy "unsupported MIME → download" | WEBKIT-ONLY | Chromium downloads such responses and `Content-Disposition: attachment` itself. The "clear the pending transition" side effect still needs a CEF signal that the navigation became a download (open question) |
| `CanDownload` for user-initiated downloads (Alt+click, attachment) | NEEDS-CEF | `CefDownloadHandler::CanDownload(browser, url, method)`: return true to allow. Verified gotcha: CEF's C++ side allows by default only when no handler exists (`browser_contents_delegate.cc:248-260`), but cef-rs always installs the trampoline (`impl_cef_download_handler_t::init_methods`) and the trait's default `can_download` returns `Default::default()` = 0, so a Rust download handler that does not override it **cancels** every user-initiated download |
| Window for toasts/warning | NEEDS-CEF | `OnBeforeDownload`'s `browser` → tab → window |
| Private session end cancels running downloads | NEEDS-CEF | cancel through kept callbacks before destroying the private `CefRequestContext`; CEF continues downloads silently until the browser context is destroyed and stops calling back once the originating browser is destroyed (verified: `OnBrowserDestroyed` nulls the browser, so no more `OnDownloadUpdated`) |
| Downloads button, toasts, warning popover, Keep/Discard, list, unique names, dangerous hold, interrupt_stale | KEEP | — |
| `open` / `show_in_folder` | KEEP | gpui 0.2.2 `App::open_with_system(path)` (spawns `xdg-open`) and `App::reveal_path(path)` (portal `OpenDirectory` with the file, falls back to opening the parent) — verified in `platform/linux/platform.rs` |

### 7.7 Interactions and tests

- `BrowserWindow::{show_downloads_button, warn_about_download, download_warning (test),
  shows_downloads_button (self-test)}`; `window/mod.rs:220-223` shows the button on new windows.
- `save_page.rs` uses `download_to`, `directory()` (initial folder of `Save Page As`), and
  `file_name`; the Settings download-folder row uses `directory()`.
- `browser.rs` `network_session(Private)` → `watch(&session, Private)`; `end_private_session` →
  `cancel_private()`, disconnect, `core.end_private_session()`.
- Unit tests in `downloads.rs`: `a_download_shows_live_progress_and_stays_on_the_list_once_cancelled`
  (stalled fixture: `1.0 KB of 1.0 MB`, line contains `/s - 1.0 KB of 1.0 MB`, first and last
  change are `List`, no progress once cancelled), `two_downloads_with_the_same_name_get_files_of_their_own`,
  `a_script_waits_under_its_unconfirmed_name_until_kept_or_discarded`,
  `a_download_into_a_folder_that_cannot_be_made_says_why`. `tab.rs`:
  `a_download_leaves_no_transition_behind`. `view_source.rs` checks `view-source:` of a binary
  file does not create a download.
- Self-test `download`, `private_window` (download part), `download_safety`, `download_speed`
  (§9).

---

## 8. Cross-cutting CEF/GPUI notes found while inventorying (facts, not design)

1. **Two bookmark models.** Core owns Vsesvit's bookmarks (synced CRDT). Chrome-style CEF
   carries Chromium's own bookmark model and `chrome://bookmarks`; no CEF API reaches it
   (inference: none found in the bindings). Extensions using `chrome.bookmarks` would see
   Chromium's model, not core's, unless bridged. The current WebKit runtime does not support
   `chrome.bookmarks` at all (`vsesvit-webext/src/support.rs` treats `bookmarks` as an
   unsupported optional permission).
2. **Two download lists.** Core's list vs Chromium's `DownloadManager`/download history, which
   `chrome.downloads` and `chrome://downloads` read (inference). CEF's handled path bypasses
   Chrome's `DownloadTargetDeterminer` (verified), which is where Chrome dispatches
   `chrome.downloads.onDeterminingFilename` and honors `saveAs` (inference from Chromium layout).
3. **Downloads without a CefBrowser are cancelled.** Verified in
   `CefDownloadManagerDelegateImpl::OnDownloadCreated`: if the item has no WebContents that maps
   to a CefBrowser, CEF logs "Rejected download" and cancels it. Downloads started by an
   extension service worker via `chrome.downloads.download` likely have no WebContents
   (inference) and would be cancelled.
4. **Callbacks stop when the originating browser closes** (verified, see §7.6). A download
   started by a tab or popup that then closes keeps running but the shell hears nothing more.
5. **Chrome-style commands.** Ctrl+D, Ctrl+Shift+B, Ctrl+Shift+O, Ctrl+J are Chrome
   accelerators too; `CefCommandHandler::OnChromeCommand` (Chrome style only) is the documented
   interception point.
6. **Overlays over web content.** Bar menus (up to 75% of monitor height), the star bubble, the
   download warning and context menus all overlap the page and may extend beyond the window in
   GTK (Wayland `xdg_popup`). GPUI renders popovers inside its own window surface; a native CEF
   child window would cover them (inference).

---

## 9. Self-test checks touching these modules (`self_test.rs`) and what they assert

| Check | Steps and assertions | Engine-specific? |
|---|---|---|
| `favicon_preload` (l. 402) | add a toolbar bookmark of fixture `/icon.html`, `bookmarks_changed()`; wait until core has an icon for it **and** the bar's button shows a favicon; screenshot `favicon-preload.png`; detail notes whether the fixture saw `/icon.html` fetched by core | no (core fetch) |
| `bookmark` (l. 673) | `win.bookmark-page` on the fixture index: `is_bookmarked` true, the bar has a button whose tooltip is the URL, `bookmark-page` action state true | star/committed URL from engine |
| `star_bubble` (l. 690) | the address bar's bubble heading is `Bookmark added`; Name field fully selected; screenshot `star-bubble.png`; set Name `Renamed fixture`, click `_Done`; core has exactly that title; second `win.bookmark-page` opens `Edit bookmark` | no |
| `bookmark_export` (l. 724) | Bookmarks window menu holds `_Import Bookmarks…`/`bookmarks.import` and `_Export Bookmarks…`/`bookmarks.export`; export file contains `Renamed fixture` | no |
| `bookmarks_bar_menus` (l. 2544) | records a 16×16 red icon for the index URL; adds folder `Fixture folder` at index 0 with `Fixture index`, a very long title, `Inner folder` (with `Inner page`), plus 16 `Example site N` links; window 900 px: hidden > 0 and chevron mapped; screenshot `bookmarks-overflow.png`; chevron menu rows (`bookmark-menu-row`) count == hidden (`bookmarks-overflow-menu.png`); first item is the folder; its menu has ≥ 1 favicon, `Inner folder` opens a submenu (`bookmarks-folder-menu.png`), the long title is ellipsized; second item's context menu (`bookmarks-context-menu.png`), `bookmark.edit` opens a dialog whose URL field equals the link's URL (`bookmarks-edit-dialog.png`); cleans up | no |
| `memory_saver` (l. 1262) | a sleeping tab's icon is drawn at half alpha (`tab-asleep.png`) | sleep mechanics engine-specific; faded icon KEEP |
| `download` (l. 1973) | download folder pref = `<out>/downloads`; loading `/download.bin` writes `download.bin` with the served bytes; core entry Completed; downloads button shown; `win.show-downloads` opens the Downloads window; `downloads.png` | yes (engine download) |
| `private_window` (l. 2011, download part) | `/download.bin` in a private window → a private (negative id) row Completed, listed in the private window's Downloads window, not in a normal one, not in normal lists; after the private window closes no private rows remain and the file still exists | yes |
| `download_safety` (l. 2193) | `/dangerous.sh` → entry Unconfirmed, `dangerous.sh.unconfirmed` holds the script, nothing at `dangerous.sh`; the warning under the downloads button appears; `_Keep` → Completed and the file at `dangerous.sh`; second download → `dangerous (1).sh`, `_Discard` removes files and entry | yes |
| `download_speed` (l. 2250) | Downloads window open; `/slow.bin` row reads speed + counts + time left (`testkit::reads_speed_and_time_left`); cancel; `/stalled.bin` row comes to read exactly `0 B/s - 1.0 KB of 1.0 MB`; cancel | yes |
| `save_page` (l. 2739) | `win.save-page` save step writes MHTML (non-HTML documents go through `download_to`, not covered) | yes |
| `shortcuts` (l. 2650) | Ctrl+J reassignment leaves `win.show-downloads` with none, etc. | no |

Self-test widget lookups that a GPUI port must provide equivalents for: bar `button_for`,
`shows_favicon`, `overflow`, `chevron`, `item`; menu rows identifiable as bookmark menu rows with
their label; the address bar's open bubble and its heading; the window's open dialog; the
download warning and its `_Keep`/`_Discard` buttons; whether the downloads button is shown; the
Downloads window's newest row subtitle.

---

## 10. Checklist

### Bookmarks bar
- [ ] Bar placed below the tab bar and above the find bar/web content; hidden in fullscreen
- [ ] Visibility from synced pref `bookmarks_bar.visible` (default on), slide reveal, applied to all windows
- [ ] `win.show-bookmarks-bar` toggle with checkmark state; Ctrl+Shift+B (reassignable); main menu, context menus and Settings switch
- [ ] Synced pref change re-applies visibility in open windows
- [ ] Toolbar folder items as flat buttons: favicon/generic icon + title ellipsized at 18 chars; tooltip = URL
- [ ] Untitled links show only the icon, accessible name = URL
- [ ] Folder buttons with `folder-symbolic` icon, tooltip = title, click opens menu below
- [ ] Separators as vertical lines
- [ ] URL nodes without a URL are not shown
- [ ] Click opens in current tab with transition Bookmark and focuses the page; middle-click opens a foreground new tab
- [ ] Items shown only whole; overflow algorithm `fitting` exactly; 1 px spacing; RTL mirrored
- [ ] `»` chevron (tooltip `More Bookmarks`) at the row end only when overflowing; its menu lists hidden items
- [ ] Bar never forces the window wider (minimum width = chevron)
- [ ] Empty state label `Bookmarks you add to the bar appear here`
- [ ] `Other Bookmarks` button at the end only when Other bookmarks has children
- [ ] Refresh on every bookmark change; icon-only changes update in place without rebuilding (menus stay open, hover kept)
- [ ] Right-click / Menu / Shift+F10 context menus on items, bar space and Other Bookmarks

### Folder menus
- [ ] Popover-style menus of icon+label rows, no arrow, start-aligned under the button; submenus beside the row (right in LTR, left in RTL)
- [ ] Menu scrolls beyond 75% of monitor height; labels ellipsized at 40 chars
- [ ] Link rows: favicon or generic icon, title or host, tooltip `title\nurl`; click opens and closes all menus; middle-click opens new tab and closes all menus
- [ ] Folder rows: folder icon, title, `go-next` arrow; hover opens submenu; click/Right opens and focuses; Left closes submenu; one submenu at a time
- [ ] Hovering a link closes a sibling's submenu; closing a menu closes its submenu
- [ ] Separator rows; empty folder shows insensitive `(Empty)`
- [ ] Depth cap of 12 levels in menus

### Context menus
- [ ] Link menu: Open in New Tab, Open in New Window (same browsing kind), Edit…, Copy Link, Delete, Add Page…, Add Folder…, Show Bookmarks Bar, Bookmark Manager
- [ ] Separator menu: Delete + bar sections
- [ ] Folder menu: Open All (n) in New Tabs (direct links; disabled at 0), Rename…, Delete (disabled for roots) + bar sections
- [ ] Bar menu: Add Page…, Add Folder…, Show Bookmarks Bar, Bookmark Manager
- [ ] Each action closes all bookmark menus first
- [ ] Delete of a non-empty folder confirms `Delete Folder?` / `“{title}” and the {count} items in it will be deleted.` with count from core
- [ ] Add Page… prefilled from the selected tab (title, committed URI unless about:blank)
- [ ] Add Folder… prompt (`Add Folder`, `New Folder`, `_Add`); empty name ignored
- [ ] New pages/folders go to the end of: the folder / the item's parent / the bar
- [ ] Open All opens the first in foreground, the rest in background
- [ ] Copy Link puts the URL on the clipboard
- [ ] Bookmark Manager opens the Bookmarks window (Ctrl+Shift+O)

### Star, bubble, editor
- [ ] Star icon/tooltip follow the selected tab's committed URL (`Bookmark This Page` / `Edit Bookmark`); `win.bookmark-page` state
- [ ] Star / Ctrl+D adds to the end of the bar with the tab's display title, stores its favicon, opens `Bookmark added`; on a bookmarked page opens `Edit bookmark`
- [ ] Bubble: heading, close (x), Name/URL/Folder, Remove, Done (default); 340 px; anchored under the star
- [ ] Name focused and fully selected on open; Enter = Done
- [ ] Done saves (rename/set_url/move to end) and closes; Remove deletes every bookmark of that URL; other closes keep the bookmark
- [ ] One address-bar bubble at a time; permission prompts wait while it is open
- [ ] Editor dialog headings `Edit Bookmark` / `Rename Folder` / `Add Page`; `_Cancel` / `_Save`
- [ ] Folder dropdown: all folders in tree order, 16 px indent per level, cap 64 levels, excludes the edited folder's subtree, Mobile only if non-empty or current
- [ ] URL validation through `classify_url` (bare host → https), error style and disabled save when invalid; folders need a name
- [ ] Save semantics: only changed fields; changed folder appends at the end; new link appended at the end of the chosen folder
- [ ] Error toasts with the exact texts

### Drag-and-drop
- [ ] Drag bar items (links, folders, separators); drag image of the item
- [ ] Drop targets: items, empty row space (append to bar), Other Bookmarks (append into Other)
- [ ] Zones: leaves halves, folders quarters (middle half = into), roots always into; RTL mirrored
- [ ] Before/after accent line and into highlight feedback; cleared on leave/drop
- [ ] Destination arithmetic (index without the moved node; no-op drops write nothing)
- [ ] One core `move_to` per drop; `bookmarks_changed()`; error toast `Cannot move the bookmark: {e}`
- [ ] Same drag/drop in the Bookmarks window tree (vertical zones), and between bar and window

### Favicons
- [ ] Bookmark icons from core's store (page, else site), generic icon fallback, on bar, menus, Bookmarks window, other-devices tabs
- [ ] Record a normal tab's icon when it changes: `wanted` pre-check, scale to ≤ 32 px, PNG, `record`; repaint on change
- [ ] Never store icons from private tabs
- [ ] Store the icon right after the star adds a bookmark
- [ ] Obtain page icons from CEF (`OnFaviconURLChange` + `DownloadImage` + `GetAsPNG`)
- [ ] Background preload of missing icons via `missing(32)` / `FaviconFetch` / `commit_fetched`, with the Idle/Running/RunningStale loop
- [ ] Sleeping tab icon at half opacity

### Downloads
- [ ] Downloads button hidden until a download listed by the window starts this run; then shown in all such windows and new ones
- [ ] Start toast `Downloading “{file}”` with `Show`; completion toast `“{file}” downloaded` with `Open`; failure toast `Download of “{file}” failed`; no toast on cancel
- [ ] Toasts/warning in the originating tab's window, else the most recent window that lists it
- [ ] Download folder = local pref or platform Downloads folder; created if missing; refusal toast `Cannot save to {dir}: {e}` and nothing listed
- [ ] Never overwrite: numbered free names; concurrent downloads reserve names
- [ ] `downloads.ask` save dialog (`Save File`, download folder, sanitized name); cancel cancels the download
- [ ] Dangerous types written as `<name>.unconfirmed`, end Unconfirmed, warning popover under the downloads button (no focus, no autohide) with Keep/Discard/close
- [ ] Keep moves to a free name and lists Completed; Discard deletes file and entry
- [ ] Non-UTF-8 destination refused with its toast
- [ ] Live byte counts fed to core `Transfer`; progress notifications at most every 250 ms; list notifications on start/end/remove
- [ ] Core rows: start, update on end (total = received when unknown and completed), interrupt_stale at startup
- [ ] Subscribe/unsubscribe change feed for the Downloads window; list limit 500; progress/cancel/remove/keep/discard/clear API
- [ ] `download_to` (Save Page As for non-HTML documents) with an already chosen path
- [ ] Private downloads: rows in memory, listed only in private windows, cancelled when the private session ends, files kept
- [ ] Open file with default app; show in folder with the file selected
- [ ] Private paths/URLs never logged (`Browsing::loggable`)
- [ ] CEF mapping: `GetDownloadHandler`, `CanDownload` (override to return true; the cef-rs default refuses), `OnBeforeDownload` + `Continue`, `OnDownloadUpdated` state mapping, `CefDownloadItemCallback::Cancel`, `StartDownload`
- [ ] Decide whether to add pause/resume and Interrupted/resume (possible with CEF, as on Windows)
- [ ] Clear the tab's pending history transition when a navigation becomes a download

### Self-test parity
- [ ] `favicon_preload`, `bookmark`, `star_bubble`, `bookmarks_bar_menus`, `download`, `private_window` (downloads), `download_safety`, `download_speed` ported with equivalent widget lookups and screenshots
- [ ] Unit tests ported: `fitting`, `zone`, `destination`, `folder_choices` depth, URL validation, form save, label fallback, separator menu, deep-folder delete confirm, favicon 32 px, download naming/unconfirmed/refusal
