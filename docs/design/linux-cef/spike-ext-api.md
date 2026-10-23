# spike-ext-api: what extensions see under GPUI + Chrome-style CEF children

Date: 2026-10-23. Spike code: `spikes/ext-api` (README there says how to run it).
Versions: `cef` 154.5.0+154.0.34 (CEF 154.0.34, Chromium 154.0.8037.98), `gpui-pre` 0.3.9, rustc 1.99.1, Ubuntu 26.04 under WSL2, Xvfb 21.1.22 (no window manager), lavapipe.
The earlier run (2026-10-16) used Fedora 44 with the same crates. Its findings are marked "earlier run" and were not redone.

Labels: **VERIFIED** = run, command and output given. **NOT VERIFIED** = run, and the thing does not work. **INCONCLUSIVE** = run, but the setup cannot settle it. **INFERRED** = my conclusion, not run.

---

## 0. Summary

A toolbar-less Chrome-style child cannot show or invoke an extension action the way Chrome does.
`chrome.action.openPopup()` fails with "Browser window has no toolbar."
The `_execute_action` shortcut does nothing.
CDP `Extensions.triggerAction` kills the browser process.
The shell can still host the popup page itself in a child browser it creates.
That popup works, sizes itself through CEF auto-resize and closes on `window.close()`.
It never gets an `activeTab` grant.
Only keyboard commands and context-menu items grant `activeTab`, and both work through stock CEF.
Alloy style is the better popup host: the popup then sees the real active tab as its current tab.

| # | Item | Result |
|---|---|---|
| 1 | `--noerrdialogs` keeps a failed `--load-extension` from showing a modal | VERIFIED (earlier run) |
| 2 | Chrome-created native windows (`tabs.create`, `windows.create`) hidden by a synchronous reparent into an unmapped holder in `on_after_created`, then embedded as GPUI tabs with Chrome's UI cropped off (85 px at scale 1), still interactive | VERIFIED (earlier run) |
| 3 | At startup the first browser stays Chrome's active window until a real focus change | VERIFIED (earlier run) |
| 4 | Tab model: every shell tab is its own `"popup"` window with one tab; `windowType: "normal"` finds nothing; every tab is `active: true` | VERIFIED (§2) |
| 5 | Popup page in a shell child browser, Alloy style: overlay works, auto-resize works, `window.close()` closes it, current tab = the real active tab | VERIFIED (§3; overlay and close also in the earlier run) |
| 6 | Same, Chrome style: works, but the popup is a tab in its own popup window, so `tabs.query({active, currentWindow})` returns the popup itself | VERIFIED (§3) |
| 7 | `activeTab` granted when the popup opens | NOT VERIFIED: never granted, in either style (§3; Alloy also in the earlier run) |
| 8 | `chrome.action.openPopup()` | NOT VERIFIED: "Could not find an active browser window.", then "Browser window has no toolbar." after `windows.update({focused: true})` (§4) |
| 9 | `_execute_action` shortcut (Alt+Shift+U) | NOT VERIFIED: keys reach the page unhandled; no popup, no grant (§4) |
| 10 | Extension command shortcut (Alt+Shift+Y) | VERIFIED: `commands.onCommand` fires with the tab and grants `activeTab` (§5) |
| 11 | Extension context-menu items | VERIFIED: present in `CefMenuModel`; `callback.cont(id)` from a shell-run menu fires `contextMenus.onClicked` with tab and frame and grants `activeTab` (§5) |
| 12 | CDP `Extensions.triggerAction` on a child tab | NOT VERIFIED: the browser process dies at once (§6) |
| 13 | How a popup gets embedded and placed | VERIFIED: shell-created 24-bit host window raised over the page, CEF child inside, right edge pinned to the action button, sized from `on_auto_resize` (§3) |

`README.md` of the earlier run was a copy of spike-core's and listed nothing else as pending.

---

## 1. How the tests were run

- Private Xvfb `:93` in a private D-Bus session (`start-display.sh`). No window manager. The Windows desktop and WSLg's `:0` were never used.
- WSL has no Xvfb package installed and no root. Xvfb, `x11-utils` and ImageMagick were unpacked from their `.deb` files into the scratch root (`dpkg -x`). Node 24.22.1 came from nodejs.org (SHA-256 checked).
- WSLg mounts `/tmp/.X11-unix` read-only. Xvfb then listens on the abstract socket only, which X clients use without trouble.
- Input: XTEST through `xctl` into Xvfb. It works there (key events reached `on_pre_key_event`).
- Fixture server `harness/server.py` on 127.0.0.1:8793. CDP daemon `harness/cdp.mjs` on the remote-debugging port 9293.
- Tab 0 is `http://127.0.0.1:8793/index.html`. Tab 1 is `http://localhost:8793/page2.html`. The spy extension has host permission for `http://127.0.0.1/*` only. So scripting into tab 1 succeeds only with an `activeTab` grant.
- Scripts: `p1-popup.sh` (§2, §3, §5), `p2-action.sh` (§4, §6). `p1-popup.sh` ran twice with the same results.
- Trap: Chromium writes DNR ruleset indexes into an unpacked extension's directory (`_metadata/generated_indexed_rulesets/`). The first run loaded the fixtures straight from the checkout and left those files in `tests/fixtures/extensions/{dnr,probe}`. They were deleted. `run.sh` now copies the fixtures into the env directory first.

Path fixes in this run: every script sources `env.sh`. Paths come from `VSESVIT_SCRATCH`, `VSESVIT_REPO`, `CARGO_TARGET_DIR` and `SPIKE_DISPLAY`. Extension ids come from `ids.py` (manifest key or path), not from the deleted `keys/ids.json`. The manifest `key`s of `spy` and `ports3` are public keys, so no private key was needed.

Build: `CEF_PATH=$VSESVIT_SCRATCH/cef cargo build`. The cef crate downloads CEF 154.0.34 into `CEF_PATH` when it is missing. WSL has no `-dev` packages, so `libxkbcommon-x11.so` and `libxcb-xkb.so` link names were symlinked into a scratch directory passed as `RUSTFLAGS="-L native=..."`.

---

## 2. Tab and window model (VERIFIED)

`sw "report()"` in the spy worker, two shell tabs:

```
all:          [{id:…117, windowId:…116, index:0, active:true, url:127.0.0.1/index.html},
               {id:…119, windowId:…118, index:0, active:true, url:localhost/page2.html}]
activeCurrent:[{id:…119, windowId:…118, url:localhost/page2.html}]
active:       both tabs
normal:       []
windows:      [{id:…116, type:"popup", focused:true, tabs:1}, {id:…118, type:"popup", focused:true, tabs:1}]
current:      {id:…116, type:"popup"}      lastFocused: {id:…118, type:"popup"}
```

- Each Chrome-style child is a one-tab `"popup"` window. Extensions that filter `windowType: "normal"` see no tabs.
- Every tab is `active: true` and every window `focused: true`, because each is the only tab of its window.
- `currentWindow` and `lastFocusedWindow` follow the GPUI tab that last had focus (tab 1 here). `windows.getCurrent()` from the worker returned the first window instead.

---

## 3. Popup page in a shell-hosted child browser (VERIFIED)

`ctl popuphost alloy|chrome chrome-extension://<spy>/popup.html` creates a 24-bit host window as a child of the GPUI toplevel, raises it over the page, and creates the CEF browser inside it. This run added auto-resize: `on_after_created` calls `set_auto_resize_enabled(1, 25x25, 800x600)`, and `on_auto_resize` resizes the host and the CEF child with the right edge kept in place.

```
gpui: popup host 0x1600001 at XRect { x: 912, y: 80, w: 360, h: 300 }: create_browser style=alloy ... -> 1
cef: on_auto_resize id=3 498x571
gpui: popup host resized to XRect { x: 774, y: 80, w: 498, h: 571 } (content 498x571 DIP)
...
gpui: popup host 0x1600001 ...: create_browser style=chrome ... -> 1
cef: on_auto_resize id=4 58x90
cef: on_auto_resize id=4 498x571
gpui: popup host resized to XRect { x: 774, y: 80, w: 498, h: 571 } (content 498x571 DIP)
```

What the popup sees (`popup.js`), with tab 1 active:

| | Alloy child | Chrome child |
|---|---|---|
| `tabs.query({active: true, currentWindow: true})` | tab 1 (`localhost/page2.html`) | the popup itself |
| `windows.getCurrent({populate: true})` | window …116 with `127.0.0.1/index.html` (the first window, not the active one) | its own `"popup"` window holding only `popup.html` |
| `tabs.getCurrent()` | id …120, `windowId: -1` | id …122 in window …121 |
| `extension.getViews({type: "popup"})` | 0 | 0 |
| `runtime.getContexts` | (not dumped) | `TAB` context for `popup.html` |
| scripting into tab 1 | `err Cannot access contents of url "http://localhost:8793/page2.html". Extension manifest must request permission to access this host.` | same error |
| `window.close()` | closes it (`popup-host browser id=3 closed`) | closes it (`id=4 closed`) |

Screenshot `extapi-env/shots/popup-chrome.png`: the popup sits at the top right of the page, sized to its content, over the localhost tab.

- The Chrome-style popup becomes an extra window and tab in the extension's model. The Alloy popup has a tab id but no window. It sees the real tab as current, so popups that do `tabs.query({active: true, currentWindow: true})` work only in Alloy style.
- Neither style gets `activeTab`. Opening the popup is not an action invocation to Chromium.
- After a context-menu grant (§5) the Chrome-style popup could script into tab 1 (`ok http://localhost:8793/page2.html`). The grant belongs to the extension and the tab, not to the surface.
- Auto-resize reports the preferred size within the given bounds. The Chrome child first reported 58x90 (before layout) and then 498x571.

---

## 4. Opening or invoking the action (NOT VERIFIED)

`p2-action.sh`, spy worker, tab 1 focused:

```
openPopup() (p1, first try):        rejected Could not find an active browser window.
openPopup({windowId: last focused}): rejected Cannot show popup for an inactive window. To show the popup for this window, first call `chrome.windows.update` with `focused` set to true.
windows.update(…687, {focused: true}): {"id":…687,"focused":true}
openPopup({windowId: …687}):        rejected Browser window has no toolbar.
openPopup():                        rejected Browser window has no toolbar.
```

The first two errors are about activation, which Xvfb without a window manager does not settle. A second `p1-popup.sh` run got "Browser window has no toolbar." on the first plain `openPopup()` already. That is the real limit: the child's Browser has no toolbar, so Chromium refuses.

`_execute_action` (Alt+Shift+U through XTEST, fresh localhost tab, no earlier grant):

```
cef: on_pre_key_event id=2 type=0 vk=0x12 native=64 mods=0x408
cef: on_pre_key_event id=2 type=0 vk=0x10 native=50 mods=0x40a
cef: on_pre_key_event id=2 type=0 vk=0x55 native=30 mods=0xa
cef: on_key_event (unhandled by page) id=2 vk=0x55 mods=0xa
rootwatch: CreateNotify 0x400003 200x200+0+0 override=false     (never mapped)
popup targets: []
spy scripting into the tab afterwards: {"ok":false,"error":"Cannot access contents of url ..."}
```

No popup opens and no grant is made. Chrome creates one 200x200 X window that it never maps.

---

## 5. Commands and context menus grant activeTab (VERIFIED)

Alt+Shift+Y (`spy-cmd`) with tab 1 focused:

```
cmds: [{"name":"spy-cmd","tab":{"id":1420896119,"windowId":1420896118,"url":"http://localhost:8793/page2.html"},
        "inject":{"ok":true,"result":"http://localhost:8793/page2.html"}}]
```

Right click into tab 1 (`xctl click … 3`, `SPIKE_MENU=shell`). The `CefMenuModel` holds the extension items:

```
[10] id=49000 IDC_EXTENSIONS_CONTEXT_CUSTOM_FIRST type=submenu label="Spy parent"
  [0] id=49001 type=command label="Spy child page"
  [2] id=49002 type=radio label="Spy radio 1" checked=1 group=2
  [3] id=49003 type=radio label="Spy radio 2" checked=0 group=2
cef: run_context_menu id=2 shell=true
```

`ctl menupick "Spy child page"` calls `callback.cont(49001)`:

```
clicks: [{"info":{"frameId":0,"frameUrl":"http://localhost:8793/page2.html","menuItemId":"spy-child2",
          "pageUrl":"http://localhost:8793/page2.html","parentMenuItemId":"spy-parent"},
          "tab":{"id":1420896119,...},"inject":{"ok":true,"result":"http://localhost:8793/page2.html"}}]
```

So a GPUI-drawn context menu built from `CefMenuModel` keeps Chrome's dispatch and its `activeTab` grant.

---

## 6. CDP `Extensions.triggerAction` (NOT VERIFIED: crash)

```
Target.getTargets {"filter":[{"type":"tab"}]}:
  [{"type":"tab","url":"http://localhost:8793/page2.html","id":"62B27B41"},{"type":"tab","url":"http://127.0.0.1:8793/index.html","id":"50387B54"}]
Extensions.triggerAction {"id":"<spy>","targetId":"62B27B41…"}
  -> fetch failed: SocketError: other side closed
browser alive: 0
cdp.log: daemon: browser connection closed
```

The browser process died with no log line. Without a tab filter `Target.getTargets` lists no `tab` targets, and the call only returns "cannot retrieve host for the provided id" (p1). Chromium 154's `ExtensionsHandler::TriggerAction` calls `ExtensionsContainer::From(*browser)->GetActionForId(...)` with no null check (`chrome/browser/devtools/protocol/extensions_handler.cc:165-200`). A toolbar-less Browser has no `ExtensionsContainer`. That null dereference is the likely cause (INFERRED).

---

## 7. What the real crate must do

1. Host action popups itself: a shell-created child browser over the page, anchored under the GPUI action button, auto-resize on (25x25 to 800x600), resized from `on_auto_resize`, closed on `on_before_close` (`window.close()`), dismissed on outside click and Escape.
2. Create popup browsers in Alloy style. In Chrome style the popup is a tab in its own window and sees itself as the current tab.
3. Intercept the `_execute_action` shortcut in the shell and open the shell popup. Chromium's own handling does nothing in a child.
4. Accept and document: no `activeTab` grant from an action click or popup, `chrome.action.openPopup()` fails, no `action.onClicked` path through Chromium. A CEF patch exposing `ExtensionActionRunner` or the action model is the only fix.
5. Never call CDP `Extensions.triggerAction`. It kills the browser.
6. Build the GPUI context menu from `CefMenuModel` in `run_context_menu`, including the extension submenus, radios and checkboxes, and answer with `callback.cont(command_id)`. That keeps `contextMenus.onClicked` and its `activeTab` grant.
7. Let Chromium handle extension command shortcuts inside the page. They fire `onCommand` with the tab and grant `activeTab`. Do not swallow their key combos in `on_pre_key_event`.
8. Expect extensions to see one `"popup"` window per tab, every tab `active`, and nothing for `windowType: "normal"`.
9. Hide Chrome-created windows synchronously in `on_after_created` and adopt them as tabs (earlier run).
10. Start with `--noerrdialogs` (earlier run).
