# Chrome extension support in CEF 154: what a GPUI + CEF Linux shell can and cannot get

Research date: 2026-10-16. Target: CEF `154.0.34+g14c5a08+chromium-154.0.8037.98`, which is the version the `cef` crate 154.5.0+154.0.34 binds (`CEF_VERSION` in `cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs`). The CEF branch is `8037`, and its head commit `14c5a089e8` ("Update to Chromium version 154.0.8037.98", 2026-10-03) is the build that commit names.

How to read the tags:
- **VERIFIED**: I read the source, header, commit, issue or forum post myself. For code, the source is CEF branch 8037 or the Chromium tag `154.0.8037.98` unless the claim says otherwise.
- **EMPIRICAL**: a third party measured it on a real CEF build. The version is given with each claim.
- **INFERRED**: my reasoning from code I read. Nobody has run it.

Where the code came from:
- CEF branch 8037 and CEF master were shallow-cloned to `/tmp/vsesvit-research/cef/` (master was at `a875f407e3` on 2026-10-10, already on Chromium 156.0.8078).
- Chromium files were fetched from `chromium.googlesource.com` at tag `154.0.8037.98`, into `/tmp/vsesvit-research/chromium/`.
- Nothing was built or run.

---

## 1. Executive summary

**The architecture.** Since M128 CEF has one bootstrap, the Chrome bootstrap. Each browser then gets one of two *styles*:
- **Chrome style** is a real Chrome `Browser` object, with its `TabStripModel` and optionally Chrome's Views UI.
- **Alloy style** is a bare `WebContents` with no `Browser`.
- **Windowless (OSR) browsers are always Alloy style.** Chrome-style OSR will never be built: issue #3293 was closed in favour of "OSR = Chrome bootstrap + Alloy style" (VERIFIED: `include/internal/cef_types_runtime.h`, `CefWindowInfo::SetAsWindowless` in `include/internal/cef_linux.h`, issues #3293 and #3681).

**Where extensions run.** Chrome's full extension system (`ExtensionService`, MV3 service workers, content scripts, storage, declarativeNetRequest, webRequest) runs *per profile*, so it runs for both styles. What differs is the Browser, tab-strip and Views layer, which most user-facing extension APIs depend on.

**Upstream's position, verbatim (VERIFIED):**
- Marshall Greenblatt, CEF forum, 2024-10-15: *"Extensions are only supported with Chrome style windows that show some portion of the Chrome UI (like the Chrome toolbar)."*
- Same thread, 2024-10-16: *"Extensions that don't require Chrome UI dependencies (toolbar windows, icons, etc) may work with Alloy style windows and OSR, but that would be up to you to test and support on a case-by-case basis."*
- Issue #4011, 2025-10-06: *"Alloy style only supports Browser/Tab extension APIs to the extent necessary to load the PDF viewer. There are no plans for general-purpose Tab API support with Alloy style."*
- Issue #4255, 2026-08-26: *"Alloy style only supports extension APIs that are required by the embedded components like the PDF Viewer."*

**There is no CEF extension API in 154.**
- `CefExtension`, `CefExtensionHandler` and `CefRequestContext::LoadExtension` were deleted with the Alloy bootstrap in commit `a461a89728` (2024-07-01), so M128 was the first release without them. Nothing replaced them.
- Issue #3450, "chrome: Add support for programmatic extension management", has been open since 2023. Issue #3877, an API to load an extension at runtime, is also open.
- That leaves only Chromium's own mechanisms. All are VERIFIED in the Chromium 154 source:
  - `--load-extension`, which still works in CEF.
  - CDP `Extensions.loadUnpacked` on a browser-target session (no flags needed since Chromium main@1615780, but the extension lasts one session only).
  - External-extension JSON files, which install silently on Linux.
  - Enterprise policy, through `CefSettings.chrome_policy_id`.
  - Chrome Web Store installs.
  - `chrome://extensions`, which only loads in Chrome-style browsers.
- **MV2 is unconditionally disabled in Chromium 154**, so "total compatibility" means MV3 at Chrome 154 parity.

**Alloy style, including every windowless browser, measured on CEF 147 and CEF 152 (EMPIRICAL):**

| Works | Fails |
|---|---|
| MV3 service workers | `tabs.query({})` and `windows.getAll()` return `[]` |
| Content scripts | Tab events never fire |
| `runtime` messaging | `tabs.update({url})` fails with "No tab with id" |
| `storage` | `tabs.create` and `windows.create` either fail ("No current window") or open a **real native Chrome window outside the app** |
| `scripting.executeScript({tabId})` | `chrome.action` icon, badge and popup state is stored, but nothing draws it and no API exposes it |
| `tabs.get`, `sendMessage`, `reload`, `getZoom`/`setZoom` (with `windowId` -1 and `index` -1) | A popup page loaded as an ordinary page cannot find "its" tab |
| `webNavigation` | No extension items in the context menu, which is CEF's own menu (VERIFIED in code) |
| declarativeNetRequest | `chrome://extensions` is blocked (VERIFIED allowlist) |
| `cookies`, `bookmarks`, `history`, `management`, `downloads` | No keyboard-command registry (INFERRED) |

**Chrome style** (VERIFIED in code unless marked):
- Every `CefBrowser` is its own Chrome `Browser` with exactly one tab.
- Hosted in a CEF Views `CefBrowserView` or in a native parent window, that Browser is created as **`TYPE_POPUP`** (`chrome_browser_host_impl.cc`). With no parent and no view, CEF opens a full Chrome-UI `TYPE_NORMAL` window.
- What works:
  - All page-level APIs.
  - `tabs` and `windows` enumerate these browsers, each one as a popup-type window with one tab.
  - Extension `contextMenus` items **are** in the `CefMenuModel` handed to `CefContextMenuHandler::OnBeforeContextMenu`, so a GPUI menu can render and execute them.
  - Extension keyboard shortcuts are registered per Browser.
- What breaks:
  - **The extension toolbar (pinned actions, badges, puzzle menu, popups) exists only when CEF draws Chrome's own Views toolbar** (`CEF_CTT_NORMAL`). That is possible only in a CEF Views window and **never in native-parent (child window) mode**, whose internal `ChildBrowserViewDelegate` gets the default `CEF_CTT_NONE`.
  - `tabs.create`, `windows.create` and `runtime.openOptionsPage` need a tabbed (`TYPE_NORMAL`) Browser. Chrome therefore opens or creates a **default Chrome-UI window**, outside any embedder UI (INFERRED from code; the same thing was observed in OBS, CEF 127 and 128).
  - `tabGroups` is unavailable.

**What this means for Vsesvit.** With stock CEF 154, "GPUI draws all browser chrome" and "extensions behave as in Chrome" contradict each other. They collide on four surfaces that real extensions depend on:
1. The **action toolbar and popups**. Popups must run in the context of the active tab and window.
2. **`activeTab` grants.** Chrome grants them when the user clicks an action, picks a context-menu item or presses a command shortcut.
3. The **tab and window model**: one GPUI window with N tabs should be one extension window with N tabs; `tabs.create` should land in GPUI; `tabGroups` should work.
4. **Extension management UI and prompts**, which are Chrome Views dialogs CEF does not let you intercept.

**Options, from most to least extension-compatible:**

| | Option | Extension compatibility | Fits "GPUI draws all chrome"? |
|---|---|---|---|
| A | **Own CEF build with a patch set** on a **Chrome-style** base (see §7) | Close to Chrome, if the patches are done | Yes |
| B | Stock CEF, Chrome style, browsers as **native X11 child windows** inside GPUI windows | All page-level APIs plus context menus and commands. **No action toolbar, no badges, no popups with tab context, no `activeTab` from a click.** Each tab is its own popup-type window, and new tabs from extensions open Chrome-UI windows | Yes, but compatibility is not "total" |
| C | Stock CEF, Chrome style in **CEF Views windows with Chrome's own toolbar** (`CEF_CTT_NORMAL`) | Highest stock compatibility | **No**: Chrome draws the toolbar and location bar |
| D | Stock CEF, **Alloy windowless (OSR)** composited by GPUI | Worst. Upstream will not fix it | Yes |

**Two precedents in 2026** show what option A costs:
- **infiniterm** (Rust, cef-rs, CEF 152, Alloy OSR) measured these gaps and decided to build its own CEF. Its patches P1 (tab and window model) and P2 (route `tabs.create` and `windows.create` to the app) are drafted against branch 7977. Their design has P3 (action-state observer, footer bar) and P4 (commands) after them, plus a codec rebuild.
- **CrabNebula** (the company behind Tauri and cef-rs) opened PR #4187, "feat: add new Extensions API for Alloy and Chrome runtimes", on 2026-06-02. It added the following, and is a usable API design reference:
  - `CefExtension`, with `GetActionIcon(browser)`.
  - `CefExtensionHandler::OnExtensionActionUpdated`.
  - `CefRequestContext::InstallExtension(crx)`, `InstallUnpackedExtension`, `LoadUnpackedExtension`, `SetExtensionEnabled`, `UninstallExtension`.
  - `ShowExtensionPopup(id, source_browser, anchor_rect)`.
  - An Alloy popup bubble that needs no `Browser`.

  The author closed the PR the next day without review. CrabNebula's site says their CEF fork "carr[ies] an Extensions API for browser runtimes that isn't in upstream CEF C++". The fork repository now returns 404.

**Recommendation.** If "total extension compatibility" is a hard requirement, plan for **option A on a Chrome-style base** and budget for maintaining a CEF and Chromium build. CEF moves to a new Chromium milestone about every four weeks, and each build is hours of work on about 100-150 GB of disk (per infiniterm's estimate). Option B is the best stock-CEF fallback. Its known gaps should be written into `docs/PLAN.md`, as the WebView2 gaps already are (§214 of PLAN). A Chrome-style base is preferred because it already supplies real `Browser` objects, Chrome's context-menu model with extension items, the keybinding registry, Chrome's dialogs, Chrome Web Store installs and `chrome://extensions`. The patches would then mostly *expose* existing Chrome models to the embedder rather than *re-create* them for bare `WebContents`, which is what infiniterm's Alloy patches have to do.

---

## 2. Q1: what each style supports

### 2.1 Background facts (VERIFIED)

The runtime-style rules, from `include/internal/cef_types_runtime.h` (branch 8037), which the cef-rs bindings copy:
- "Windowless rendering will always use Alloy style."
- "Windowed rendering with a default window or client-provided parent window can configure the style via CefWindowInfo.runtime_style."
- "Alloy style Windows with the Views framework can host only Alloy style BrowserViews but Chrome style Windows can host both style BrowserViews."
- "a Chrome style Window can host at most one Chrome style BrowserView but potentially multiple Alloy style BrowserViews."

`CefWindowInfo::SetAsWindowless()` on Linux sets `runtime_style = CEF_RUNTIME_STYLE_ALLOY` (`include/internal/cef_linux.h:106-110`).

`CefBrowserPlatformDelegate::Create` (`libcef/browser/browser_platform_delegate_create.cc`) has three Chrome-style delegates:
- `CefBrowserPlatformDelegateChromeChildWindow` when a parent handle is set.
- `CefBrowserPlatformDelegateChromeViews` when there is a `CefBrowserView` or a Views-hosted popup opener.
- `CefBrowserPlatformDelegateChrome` otherwise: a default, fully styled Chrome window.

For Alloy style it uses the Views delegate, the native delegate, or the OSR delegate.

`ChromeBrowserHostImpl::Create` (`libcef/browser/chrome/chrome_browser_host_impl.cc:27-61, 403-470`):
- Calls `CreateBrowserWindow(...)`, which creates a **new Chrome `Browser` for every Chrome-style CefBrowser**.
- Then calls `chrome::AddTabAt(...)`, which gives it one tab.
- When `params.browser_view` is set, which also covers native-parent mode because CEF builds a `CefBrowserView` internally, a `TYPE_NORMAL` Browser is forced to **`TYPE_POPUP`** with `is_trusted_source = true`. The code comment reads: "Don't show most controls … Don't show title bar or address".

**Alloy-style browsers get Chrome's tab helpers.** `CefBrowserPlatformDelegateAlloy::AttachHelpers` calls `TabHelpers::AttachTabHelpers(web_contents)`; CEF's patch adds `friend class CefBrowserPlatformDelegateAlloy` to `chrome/browser/ui/tab_helpers.h`. As a result, Alloy contents have a `SessionTabHelper` (an extension tab id), the extension `WebContents` observers and `webNavigation` observers. They are just never part of any `TabStripModel`. Source: `libcef/browser/alloy/browser_platform_delegate_alloy.cc:179-219`.

### 2.2 (a) Chrome style in CEF Views windows

The *profile-level* extension system works fully.

The *UI-level* extension features work **only if the client returns `CEF_CTT_NORMAL` from `CefBrowserViewDelegate::GetChromeToolbarType`** (the default is `CEF_CTT_NONE`, per `include/views/cef_browser_view_delegate.h:111-120`) and places the toolbar view from `CefBrowserView::GetChromeToolbar()`:
- `ChromeBrowserView::OverrideCreateToolbar` maps `CEF_CTT_NORMAL` to `ToolbarView::DisplayMode::kNormal` and `CEF_CTT_LOCATION` to `kLocation` (`libcef/browser/chrome/views/chrome_browser_view.cc:85-110`).
- Chromium's `ToolbarView::Init` creates `ExtensionsToolbarDesktop`, the extensions container, only in `kNormal` mode. It skips it for guest profiles and when `features::IsWebUIExtensionsContainerEnabled()` is on (`chrome/browser/ui/views/toolbar/toolbar_view.cc:349-412` at 154).
- So with `CEF_CTT_LOCATION` or `CEF_CTT_NONE` there are no action buttons and no puzzle menu. (VERIFIED code; INFERRED behaviour.)
- CEF already carries a patch for the "toolbar hidden" case. `chrome_browser_dialogs_misc.patch` makes `extension_dialog_utils_views.cc` tolerate a null `toolbar_button_provider`. Its `patch.cfg` comment says it is there to "Avoid crash showing MV2 disabled dialog with hidden toolbar" (VERIFIED).

The tab and window model in this mode:
- Each CefBrowser is a `TYPE_POPUP` Browser with one tab. Extensions see each tab as a window of type `"popup"`. Popup windows are visible to the tabs API: `BrowserExtensionWindowController::IsVisibleToTabsAPIForExtension` hides only devtools windows (VERIFIED, `browser_extension_window_controller.cc:175-188`).
- `tabs.create` without a `windowId` targets the "current" window. Chromium 154's `TabsCreateFunction::Run` calls `OpenTabInBrowser(*browser, …)` (`tabs_api.cc:2121-2295`). A popup Browser cannot hold extra tabs, so `chrome::Navigate` then finds or creates a `TYPE_NORMAL` Browser. CEF turns a Browser it did not create itself into a default, fully styled Chrome window, with the client from `CefBrowserProcessHandler::GetDefaultClient()` (`chrome_browser_delegate.cc:500-560, 628-653`; `include/cef_browser_process_handler.h:141-150`). (INFERRED from code.)
- The OBS developer saw exactly this on CEF 127 and 128: Volume Master's install flow "opens two tabs in a new Chrome UI window". `OnBeforePopup` and `OnOpenURLFromTab` were never called, and the only remedy was to close the window from `OnAfterCreated` (EMPIRICAL, [CEF forum t=19898](https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=19898)).
- Chrome style also accepts the Web Store, `chrome://extensions`, and Chrome's install and permission dialogs.

### 2.3 (b) Chrome style in a native parent window (`CefWindowInfo.parent_window` plus `runtime_style = CHROME`)

- Linux uses an X11 XID as `cef_window_handle_t` (`unsigned long`). macOS does not support this mode (`GetParentWidget` returns null on Mac, see #3294).
- `chrome_child_window::MaybeCreateChildBrowser` creates a `CefBrowserView` with an **internal** `ChildBrowserViewDelegate`, then a `CefWindow` parented to the XID (`libcef/browser/chrome/views/chrome_child_window.cc:150-260`). That delegate does not override `GetChromeToolbarType`, so the toolbar is always `CEF_CTT_NONE` and there is **no extension toolbar of any kind** (VERIFIED).
- The public API has no way to create a `CefWindow` under a foreign native window. `CefWindowDelegate::GetParentWindow` returns a `CefWindow`, not a native handle (VERIFIED, `include/views/cef_window_delegate.h:100-113`).
- Otherwise this is identical to 2.2 without a toolbar. It is a real Browser (`TYPE_POPUP`, one tab), and the context-menu, commands, dialogs and tabs-API behaviour is Chrome's.
- This is the mode GPUI would use to embed Chrome-style content on X11 or XWayland. Wayland-native embedding is issue #2804, still open; a community patch at `leandromqrs/cef` `feat/wayland-embed-support`, posted 2026-08-03, has not been merged.
- Magreenblatt's rule ("Chrome style windows that show some portion of the Chrome UI (like the Chrome toolbar)") means extension UI in this mode is officially *unsupported*.

### 2.4 (c) Alloy style (windowed, Views or native parent)

- CEF patches `ExtensionTabUtil::GetTabById` to fall back to `cef::GetAlloyTabById`. That function scans all render-process frames for a `WebContents` whose `SessionTabHelper::IdForTab` matches and whose CEF host is Alloy style (`libcef/browser/chrome/extensions/chrome_extension_util.cc`, `patch/patches/chrome_browser_extensions.patch`).
- That patch was added in commit `ed079792b6` (2024-04-27), whose message is "This change adds minimal tabs API support for Alloy style browsers. Clicking links in PDF documents now navigate as expected." It was **aimed at the PDF viewer**.
- `TabsQueryFunction`, the `windows.*` functions and `TabsEventRouter` are **not** patched. They walk `Browser` objects only, so Alloy tabs never appear in `tabs.query` or `windows.getAll` and never fire tab events (VERIFIED code; EMPIRICAL CEF 152).
- `TabsUpdateFunction` was patched to work without a window. In 154 the `autoDiscardable` and `openerTabId` paths return errors instead of crashing (#4256, fixed in commit `7ef0d1ef7d` on branch 8037, 2026-09-10).
- The `url` path still fails with "No tab with id", because `tabs_internal::GetTabById` has no Alloy fallback. That was reported on CEF 147 (#4255) and closed as unsupported. The 154 patch does not change it (VERIFIED patch; INFERRED that 154 behaves like 147).
- **WebUI allowlist.** `AlloyBrowserHostImpl::MaybeAllowNavigation` blocks any `chrome://` host outside `kAllowedWebUIHosts`, logging "Navigation to … is blocked in Alloy-style browser." The allowlist is accessibility, blob-internals, chrome-urls, credits, gpu, histograms, indexeddb-internals, license, media-internals, net-export, net-internals, network-error(s), print, process-internals, resources, sandbox, serviceworker-internals, system, terms, theme, tracing, version and webrtc-internals. **`chrome://extensions` is not on it** (VERIFIED `alloy_browser_host_impl.cc:63-102, 543-573`; #3859).
- `chrome-extension://` URLs are *not* blocked. An Alloy browser can load `chrome-extension://<id>/popup.html` and the page reaches its service worker (EMPIRICAL, infiniterm, CEF 152).
- **Context menus.** Alloy uses `CefMenuManager`, which builds CEF's own default model (`libcef/browser/menu_manager.cc:130-165, 334ff`). It does not use Chrome's `RenderViewContextMenu`, so extension `contextMenus` items never appear (VERIFIED code).

### 2.5 (d) Windowless (OSR)

- Always Alloy, so everything in 2.4 applies.
- The OSR-specific measurements are infiniterm's table on CEF 152 (§6.1) and the #4255/#4256 reports on CEF 147.
- `ShowDevTools` on an OSR browser opens a Chrome-style native window ("Windowless rendering is not supported for this DevTools window", `chrome_browser_delegate.cc:139`). That affects debugging extension pages.
- Chrome's dialogs need a parent window. CEF patches some of them for OSR, for example `certificate_selector.cc`. Extension install and permission prompts are browser-modal Views dialogs parented to `show_params->GetParentWindow()` (VERIFIED `extension_install_dialog_view.cc:83-108`). With OSR that parent is the `CefWindowInfo.parent_window` given to `SetAsWindowless`, or none at all (INFERRED).

### 2.6 History of the extension API

| Milestone or date | Event | Tag |
|---|---|---|
| 2017 (CEF 3112 era) | `CefRequestContext::LoadExtension`, `CefExtension` and `CefExtensionHandler` added for the Alloy runtime (#1947, commit `9cff99dc4e`) | VERIFIED |
| 2020-11 | Maintainer: no more Chrome extension APIs for Alloy "beyond what is required by the PDF viewer extension"; full support only with the Chrome runtime (#1947 comments, #3048) | VERIFIED |
| 2022-03 | #3285 "chrome: Decide what to do with LoadExtension": use `--load-extension`; "We won't implement extension-related API at this time." | VERIFIED |
| M125 (2024-04) | Alloy split into style and bootstrap (#3681). `ed079792b6` adds minimal tabs support for Alloy, for PDF | VERIFIED |
| M128 (2024-07-01) | Alloy bootstrap deleted (#3685, commit `a461a89728`). Removed `include/cef_extension.h`, `cef_extension_handler.h`, their C API counterparts and the whole `libcef/browser/extensions/` Alloy implementation. #3685: "The Alloy extension API is not supported (has been removed in M128). The Chrome extension API is supported with Chrome style browsers/windows only." **Nothing replaced it.** | VERIFIED |
| 2024-12 | #3859 (Alloy, `chrome://extensions`): "Extension functionality that works at the RequestContext level (like network intercepts, etc) should already work with Alloy style browsers. Extension functionality that requires Chrome UI (like toolbars, etc) will not be supported with Alloy style browsers." | VERIFIED |
| 2025-10 / 2026-08 | #4011, #4255: no general tabs API for Alloy. #4256: graceful errors instead of a crash (fixed in 154) | VERIFIED |
| 2026-10 | CEF master (Chromium 156.0.8078) has the same extension surface as 8037. `include/` has no extension API, and `chrome_browser_extensions.patch` differs only in offsets | VERIFIED |

---

## 3. Q2: loading and installing extensions in CEF 154

### 3.1 `--load-extension`: honoured (VERIFIED)

- Chromium commit `290ed80466` (2025-04-05, main@1443013, in M137), "Remove `--load-extension` switch on Chrome builds", removed it **only from Google Chrome-branded builds**.
- Commit `04f6233ce5` (2025-09-25, main@1520577) then deleted the kill-switch feature `kDisableLoadExtensionCommandLineSwitch` and hard-coded the decision by build flag. **`--disable-features=DisableLoadExtensionCommandLineSwitch` no longer exists and is not needed.**
- In Chromium 154, `ExtensionService::LoadExtensionsFromCommandLineFlag` (`chrome/browser/extensions/extension_service.cc:412-470`) works like this:
  - Under `#if BUILDFLAG(GOOGLE_CHROME_BRANDING) && !BUILDFLAG(IS_CHROMEOS)` it logs "--load-extension is not allowed in Google Chrome" and returns.
  - Otherwise, which is the CEF case because CEF builds are Chromium-branded, it loads the extension **unless** one of these holds:
    - the profile has Enhanced Safe Browsing on (`safe_browsing::IsEnhancedProtectionEnabled`), or
    - the `ExtensionInstallTypeBlocklist` policy contains `"command_line"`.
  - It also needs `extension_registrar_->extensions_enabled()`, so `--disable-extensions` turns it off.
- Embedders confirm it works: #4255 on CEF 147 (Windows) and #4309 on CEF 148 (macOS, `cefclient --load-extension=<dir>`), both EMPIRICAL.
- Properties (VERIFIED code):
  - Location is `kCommandLine` (`unpacked_installer.cc:171`).
  - **Not persisted**: `InstalledLoader::LoadAllExtensions` skips `kCommandLine` records at startup (`installed_loader.cc:364-406`). The prefs record is not deleted, so `Local Extension Settings/<id>` should survive (INFERRED).
  - Exempt from the developer-mode requirement, which applies only to `kUnpacked` (`ExtensionManagement::IsAllowedByUnpackedDeveloperModePolicy`, `extension_management.cc:374-395`; `kExtensionDisableUnsupportedDeveloper` is enabled by default).
  - The extension ID comes from the manifest `key` if present, otherwise from the path.
  - The switch value is comma-separated.
  - CEF does not filter the switch in 8037. A `grep` for `load-extension` and `LoadExtension` in libcef finds nothing.
  - The app can append it in `CefApp::OnBeforeCommandLineProcessing`.
- A known gotcha (EMPIRICAL, infiniterm, CEF 152): after `bg.js` and the manifest version changed in an extension reloaded under the same ID and path, "the old service worker kept running until the extension id changed". Plan for this if Vsesvit's own updater swaps unpacked directories.

### 3.2 CDP `Extensions.*` domain (VERIFIED, Chromium 154)

- `chrome/browser/devtools/chrome_devtools_session.cc:102-107` creates `ExtensionsHandler` for trusted clients, with `allow_loading_extensions = (agent_host type == kTypeBrowser)`.
- Commit `dddf05b0e6` (2026-04-16, main@1615780, in M149) **removed the earlier requirement for `--remote-debugging-pipe` and `--enable-unsafe-extension-debugging`**: "This makes it possible to install extensions via WebSockets where WebSockets are allowed."
- Commit `0333892185` plus `62a851c413` (April 2026) made CDP installs **session-only**: the `Extension::INSTALLED_VIA_CDP` flag, and `ExtensionPrefs::CleanUpCdpInstalledExtensions()` at `extension_prefs.cc:2648-2656` in 154.
- Methods in 154 (`chrome/browser/devtools/protocol/extensions_handler.cc`):
  - `loadUnpacked(path, enableInIncognito?)` → id. Location `kUnpacked`, exempt from developer mode.
  - `uninstall(id)`, for unpacked extensions only.
  - `getExtensions()`, which lists **only unpacked-location** extensions.
  - `getStorageItems`, `setStorageItems`, `removeStorageItems`, `clearStorageItems`.
  - `triggerAction(id, targetId)`. It looks up `ExtensionsContainer::From(*browser)` and calls `GetActionForId` **without a null check**. A Browser without the Chrome toolbar has no `ExtensionsContainer`, so with any setup except `CEF_CTT_NORMAL` this would dereference null (INFERRED from code).
  - `loadUnpacked` and `getExtensions` act on `DevToolsBrowserContextManager::GetDefaultBrowserContext()`, the default profile only.
- **Reachability from CEF:**
  - `CefBrowserHost::SendDevToolsMessage` and `ExecuteDevToolsMethod` attach to the browser's *page* agent host (`libcef/browser/devtools/devtools_controller.cc:133-135`). Page sessions get `TargetHandler::AccessMode::kRegular` or `kAutoAttachOnly`, and `Target.attachToBrowserTarget` requires `kBrowser` (`target_handler.cc:1213-1216`; `render_frame_devtools_agent_host.cc:456-460`). **So `Extensions.loadUnpacked` cannot be reached through CEF's DevTools message API** (VERIFIED code).
  - It needs `CefSettings.remote_debugging_port`, which CEF maps to `--remote-debugging-port` (`chrome_main_delegate_cef.cc:329-333`), or `--remote-debugging-pipe` (file descriptors 3 and 4). Chrome's `RemoteDebuggingServer::GetInstance` handles both, and the default-user-data-dir restriction applies only to Google Chrome branding (`remote_debugging_server.cc:164-180, 353-395`).
  - CEF using Chrome's pipe handler is INFERRED. The port is EMPIRICAL (infiniterm and #4309 use it).
- **Security cost:** a TCP debugging port lets any local process drive every tab and install extensions.

### 3.3 External-extension JSON (persistent, silent on Linux; VERIFIED)

`ExternalProviderImpl::CreateExternalProviders` registers three Linux sources (`external_provider_impl.cc:857-895`):

| Path | Contents |
|---|---|
| `/usr/share/chromium/extensions/<id>.json` | `chrome::DIR_STANDALONE_EXTERNAL_EXTENSIONS`, Chromium branding. **Shared with any system Chromium** |
| `<directory of libcef.so>/extensions/external_extensions.json` | `chrome::DIR_EXTERNAL_EXTENSIONS` = `DIR_MODULE/extensions` |
| `<root_cache_path>/External Extensions/<id>.json` | `chrome::DIR_USER_EXTERNAL_EXTENSIONS`, Linux with `CHROMIUM_BRANDING`. CEF overrides `chrome::DIR_USER_DATA` with `CefSettings.root_cache_path` (`libcef/common/resource_util.cc:126-131`) |

- `FeatureSwitch::prompt_for_external_extensions` defaults to on only for Windows and Mac (`extensions/common/feature_switch.cc:22-27`), so **on Linux external installs are enabled without a prompt**.
- The JSON is `{"external_crx": "/abs/path.crx", "external_version": "x.y"}` or `{"external_update_url": "https://clients2.google.com/service/update2/crx"}`. The second downloads from the Chrome Web Store and auto-updates (format INFERRED from Chromium docs).
- This is the one stock way to get a **persistent, packed (CRX3, signature-checked)** install without user interaction. It fits Vsesvit's existing core pipeline, which downloads and verifies the CRX and could then hand Chromium the CRX path.

### 3.4 Enterprise policy (VERIFIED)

- `CefSettings.chrome_policy_id` enables "Chrome policy management via Platform and OS-user policies … On Linux, this is an absolute directory path like `/etc/opt/chrome/policies`". It is documented as "Only supported with Chrome style" (`include/internal/cef_types.h:474-487`; patch `chrome_browser_policy.patch`, issue #3581). CEF calls `ChromeBrowserPolicyConnector::EnablePlatformPolicySupport(id)` (`chrome_main_delegate_cef.cc:489-492`).
- `ExtensionInstallForcelist` and `ExtensionSettings` give persistent installs that the user cannot remove.
- `webRequestBlocking` for MV3 is available **only** with `"location": "policy"` (`extensions/common/api/_permission_features.json:783-792`).
- Off-store force-installs are blocked in "low trust environments" only on Windows and Mac (`extension_management.cc:90-99, 441-457`), so **Linux allows any update URL**.
- Side effect: Chrome WebUI pages say "managed by your organization". A GPUI shell would hide most of that.

### 3.5 Chrome Web Store and CRX

- `webStorePrivate` is exposed to `"contexts": ["web_page"], "matches": ["https://chromewebstore.google.com/*"]` with no branding gate (`extensions/common/api/_api_features.json:965-976`). Chromium-branded builds can therefore install from the Web Store (VERIFIED feature definition).
- The install prompt is Chrome's `ExtensionInstallDialogView`, a browser-modal Views dialog (VERIFIED). Expected to work in Chrome style. In Alloy style the `webstorePrivate` binding should be present, since the renderer client is Chrome's, but the prompt's parenting is untested (INFERRED, needs a spike).
- Extensions from the Web Store auto-update through `ExtensionUpdater` (INFERRED; the Chromium default update URL).
- **CRX from the web or a file:** a website install of a CRX needs the `ExtensionInstallSources` policy (`ExtensionManagement::IsOffstoreInstallAllowed`, `extension_management.cc:309-321`). Dragging a CRX onto `chrome://extensions` in developer mode works in Chrome style (INFERRED).
- **No CEF C API exists** for any of this.

### 3.6 `chrome://extensions`

- Available in Chrome-style browsers only (2.4). It is the only complete management UI without patches: enable/disable, details, site access, "Allow in Incognito", file URLs, errors, shortcuts and "Load unpacked" (which needs developer mode).
- It can be shown inside a GPUI-hosted Chrome-style content area, so it is WebUI content rather than browser chrome.

### 3.7 Preferences on disk

| What | Where |
|---|---|
| Profile directory | `CefSettings.cache_path` or `CefRequestContextSettings.cache_path`, which must be under `root_cache_path` (VERIFIED `cef_types.h:284-296, 298-324`). An empty `cache_path` means an incognito profile, where extensions run only if allowed in incognito (INFERRED) |
| Extension state | Chrome's standard layout (INFERRED, standard Chrome profile code): `Preferences` / `Secure Preferences` key `extensions.settings`, `Extensions/<id>/<version>_0/` for packed installs, `Local Extension Settings/<id>` (`storage.local`), `Sync Extension Settings`, `Managed Extension Settings`, `Extension Rules`, `Extension Scripts`, `Extension State`, plus the profile's `Service Worker/` |
| `Local State` (global) | `root_cache_path` |

Editing `extensions.settings`:
- #3877 (open): writing `extensions.settings` with `CefRequestContext::SetPreference` takes effect only after a restart.
- **On Linux, tracked-preference enforcement is off.** `GetSettingsEnforcementGroup()` returns `GROUP_NO_ENFORCEMENT` everywhere except Windows and Mac (`chrome/browser/prefs/chrome_pref_service_factory.cc:213-233`). Pre-seeding `extensions.settings` before `CefInitialize` is therefore not reset on Linux.
- It is still fragile: an internal, unversioned format.

Native messaging host lookup (VERIFIED `chrome_paths.cc:452-485`):
- `/etc/chromium/native-messaging-hosts` system-wide.
- `<root_cache_path>/NativeMessagingHosts` per user.
- **Not `~/.config/google-chrome` or `~/.config/chromium`.** Password managers (Bitwarden, 1Password, KeePassXC, Proton Pass) install their host manifests for Chrome and Chromium, so Vsesvit must copy or symlink them, or ask users to.

---

## 4. Q3: API by API

Columns:
- **Chrome+TB**: Chrome style in a CEF Views window with `CEF_CTT_NORMAL`.
- **Chrome-noTB**: Chrome style in a Views window without a toolbar, or in a native parent window.
- **Alloy**: windowed or OSR.

| API | Chrome+TB | Chrome-noTB | Alloy (windowed / OSR) | Basis |
|---|---|---|---|---|
| `runtime` (messaging, ports, `onInstalled`, `getURL`, `connectNative`) | Works | Works | Works. Content script → service worker `sendMessage` works; `sender.tab = {id, windowId: -1, index: -1}` | EMPIRICAL #4255 (CEF 147 OSR), infiniterm (152) |
| `runtime.openOptionsPage` | Opens in a tab of a tabbed Browser, or a new default Chrome window | Same: a Chrome-UI window outside the app | Fails; the Alloy path has no Browser | INFERRED; infiniterm notes |
| `storage` (local/sync/session/managed) | Works | Works | Works | EMPIRICAL (Dark Reader in infiniterm), profile-level |
| `tabs.query` / `windows.getAll` | Every CefBrowser is a `"popup"` window with 1 tab; the default Chrome window is `"normal"` | Same | **`[]`**; `windows.getLastFocused` → "No last-focused window" | VERIFIED code; EMPIRICAL infiniterm |
| `tabs.get`, `sendMessage`, `reload`, `get/setZoom` | Works | Works | Works (`windowId` -1, `active` always false) | EMPIRICAL #4255, infiniterm |
| `tabs.update` | Works (pinned/active within its 1-tab window) | Works | `url` → "No tab with id"; `autoDiscardable` → error (crashed the browser before the 154 fix) | EMPIRICAL #4255/#4256, VERIFIED fix in 8037 |
| `tabs.create`, `windows.create` | Opens a default Chrome-UI `TYPE_NORMAL` window (outside the embedder) | Same | "No current window", or a real native Chrome window | INFERRED code + EMPIRICAL OBS (127/128), #4255, infiniterm |
| `tabs.captureVisibleTab` | Works | Works | "No current window" | EMPIRICAL #4255 |
| Tab and window events (`onCreated`, `onUpdated`, `onActivated`, `onRemoved`, `windows.onFocusChanged`) | Works for CEF Browsers (the 1-tab windows) | Works | **Never fire** | EMPIRICAL infiniterm; VERIFIED (TabsEventRouter tracks TabStripModels) |
| `scripting.executeScript({target: {tabId}})`, `insertCSS`, `registerContentScripts` | Works | Works | Works | EMPIRICAL #4255, infiniterm |
| `action` / `browserAction`: icon, badge, title, enable | **Drawn by Chrome's Views toolbar**; popups are Views bubbles | Stored, **not drawn** | Stored, **not drawn** | VERIFIED code (toolbar_view.cc), EMPIRICAL infiniterm |
| `action.openPopup`, `_execute_action` shortcut | Works (needs the toolbar anchor) | No container; fails (CDP `triggerAction` would dereference null) | Fails | INFERRED code |
| Action state exposed to the embedder | **No API in any style** | No | No | VERIFIED (absent from `include/`) |
| `activeTab` granted by an action click | Yes | No toolbar, so no click | No | INFERRED (Chrome grants it from action, context menu and command invocations) |
| `contextMenus` | Items in Chrome's menu; **also in the `CefMenuModel`** passed to `OnBeforeContextMenu` and runnable through `RunContextMenu` + `Continue(command_id)`. **Item icons not exposed** (#4205 open) | Same (menu present without a toolbar) | **Absent**: CEF's own `CefMenuManager` model. API calls succeed | VERIFIED: `chrome_browser_context_menus.patch` runs CEF's observer `InitMenu` *after* `RenderViewContextMenu::InitMenu` appends extension items (`render_view_context_menu.cc:1495-1509`); `menu_manager.cc` |
| `commands` (keyboard shortcuts) | `ExtensionKeybindingRegistryViews` per Browser with a focus manager | Same while the CEF view has focus (the Chrome accelerator table stays live in child views, per Tauri's CEF docs) | No registry; `onCommand` never fires; `getAll` works | VERIFIED `browser_window_features.cc:832-838`; INFERRED Alloy |
| declarativeNetRequest | Works | Works | Works | EMPIRICAL infiniterm; profile-level. `displayActionCountAsBadgeText` needs the toolbar |
| `webRequest` (observe, `extraHeaders`) | Works. Interception is chained with CEF's `ProxyURLLoaderFactory` (#3444 fixed in M111); magreenblatt has "not verified that chrome.webRequest extension APIs work as expected"; `TrustedURLLoaderHeaderClient` proxying is still a TODO | Same | Same ("RequestContext level … should already work", #3859) | VERIFIED issue text |
| `webRequestBlocking` | MV3: policy-installed only; MV2: dead | Same | Same | VERIFIED `_permission_features.json` |
| `cookies` | Works | Works | Works | EMPIRICAL infiniterm |
| `webNavigation` | Works | Works | Works | EMPIRICAL infiniterm |
| `notifications` | Profile-level; Linux D-Bus notification bridge | Same | Same | INFERRED (infiniterm's test was invalid) |
| `sidePanel` | `SidePanelCoordinator` exists whenever a BrowserView exists; whether CEF's `ChromeBrowserView` lays the panel out is untested; the side-panel toolbar button is `CEF_CTBT_SIDE_PANEL_DEPRECATED` | Unknown, likely no UI | No Browser, so `open()` fails | VERIFIED `browser_window_features.cc:842-845, 911-914`; INFERRED behaviour |
| `offscreen` | Profile-level `OffscreenDocumentHost` | Same | Same | INFERRED |
| `identity` | `getAuthToken` needs Chrome sign-in plus Google API keys (CEF has neither by default); `launchWebAuthFlow` opens a browser window | Same; the window is a Chrome-UI window | `launchWebAuthFlow` would open a native Chrome window | INFERRED (infiniterm: "identity works" for basic calls) |
| `downloads` | Works (profile `DownloadManager`) | Works | `downloads.search` etc. work | EMPIRICAL infiniterm |
| `management` | Works | Works | Works | EMPIRICAL infiniterm |
| `tabGroups`, `tabs.group` | Only in default Chrome `TYPE_NORMAL` windows | No (popup Browsers) | No | INFERRED (`ExtensionTabUtil::SupportsTabGroups` → `TabStripModel::SupportsTabGroups`) |
| `bookmarks`, `history`, `topSites`, `sessions` | Works | Works | Works | EMPIRICAL infiniterm |
| Install, update and permission prompts (`permissions.request`, Web Store install, host-permission prompts) | Chrome Views dialogs parented to the window | Views dialogs parented to the child or top-level window | Views dialog parented to the native top-level window (windowed), or the OSR `parent_window` / unparented | VERIFIED `extension_install_dialog_view.cc:83-108`; **no CEF callback**: `CefPermissionHandler::OnShowPermissionPrompt` covers only web permission types (`cef_types.h:3901-3937`) |
| `omnibox` (keyword) | Chrome's location bar | No location bar | No | INFERRED. **Vsesvit's GPUI omnibox would have to implement this itself** |
| `chrome_url_overrides.newtab` | `chrome://newtab` rewritten | Works if the embedder navigates to `chrome://newtab` | Same, if `chrome://newtab` loads (it is not on the Alloy allowlist, so likely blocked) | INFERRED |
| `chrome_settings_overrides` (search, homepage) | Chrome's `TemplateURLService` | Ignored by a GPUI omnibox unless bridged | Same | INFERRED |

**Tab IDs.** `CefBrowser::GetIdentifier()` is documented as "also used as the tabId for extension APIs" (`include/cef_browser.h:126-130`). In 154 that is **stale**:
- `GetIdentifier()` returns CEF's own counter, `++next_browser_id_` (`browser_info_manager.cc:76-101`; `browser_host_base.cc:1205-1207`).
- Extension APIs use `sessions::SessionTabHelper::IdForTab` (`chrome_extension_util.cc`).
- #4255 shows a tab ID of `703482399` for an OSR browser.
- So mapping CefBrowser to a tab ID needs a content-script handshake or a patch (VERIFIED code; the conclusion is INFERRED).

---

## 5. Q4: MV3 service workers and content scripts

- **MV3 service workers run in every style.** They are profile-level extension hosts, not tied to a browser (EMPIRICAL: #4255 on CEF 147 OSR, #4309 on CEF 148 windowed and OSR, infiniterm on CEF 152 OSR).
- **Open bug in all styles, #4309 (2026-10-12):**
  - A cross-origin CORS `fetch()` from an extension service worker is sent with `Origin: null`.
  - Cause: `ChromeContentBrowserClientCef::WillCreateURLLoaderFactory` puts CEF's `ProxyURLLoaderFactory` in front of every factory. `InterceptedRequest::Restart` then derives `Origin` through `Referrer::SanitizeOriginForRequest`, and `chrome-extension` is not a referrer scheme.
  - This **breaks 1Password**, whose server returns HTTP 500.
  - The embedder cannot fix it through CEF handlers, because extension service-worker requests have no browser and so `CefResourceRequestHandler` is never consulted. The reporter's workaround is a DevTools `Fetch` interceptor that drops the header.
  - Related: #3593, service-worker requests not reaching `OnBeforeResourceLoad`, closed 2026-09.
  - For "total compatibility" this needs a CEF patch or an upstream fix.
- **Content scripts run in Alloy and windowless browsers** (EMPIRICAL: #4255 step 3, a content script on `<all_urls>` in an OSR browser; infiniterm's Dark Reader filters pages).
  - Why: the renderer is Chrome's (`ChromeContentRendererClient` with the extensions renderer client, through the Chrome bootstrap), and the browser side attaches Chrome's tab helpers (VERIFIED `AttachTabHelpers`).
  - The 2024 crash report #3654 ("load-extension with content_scripts" crashing in `ExtensionInjectionHost::CanExecuteOnFrame`) belongs to the deleted Alloy *bootstrap*.
- **Stale worker after an update** (EMPIRICAL, infiniterm, CEF 152): when an unpacked extension is reloaded with the same ID, the old worker can keep running.

---

## 6. Q5: infiniterm and other embedder reports; upstream or fork changes in 2025-2026

### 6.1 infiniterm issue #310, PRs #311, #332, #333 (EMPIRICAL / design)

Issue #310 (2026-10-08, open), measured on CEF 152 Alloy windowless: "`chrome.tabs.query` and `chrome.windows.getAll` return nothing, so a popup cannot find its page. `chrome.windows.create` and `chrome.tabs.create` open a real native window outside the canvas. `chrome.action` state (icon, badge, title) is stored but nothing draws it. No toolbar." It also reports no H.264, H.265 or AAC and no DevTools in a card.

The spec in PR #311 (`docs/superpowers/specs/2026-10-09-browser-extensions-cef-fork-design.md`) has a probe table on CEF 152.0.6 / Chromium 152.0.7977.83:

| Check | Result |
|---|---|
| Dark Reader works | Yes |
| Popup loaded as an ordinary page | Renders |
| `tabs.query` / `windows.getAll` | `[]` |
| `windows.getLastFocused` | Error |
| `tabs.get`, `scripting.executeScript({tabId})`, `webNavigation` | Work (`active` false, `windowId` -1) |
| Tab events | Never fire |
| `windows.create` / `tabs.create` | Open a real native window |
| `action` | Values stored; nothing drawn |
| `bookmarks`, `history`, `topSites`, `sessions`, `cookies`, `management`, DNR, `commands.getAll`, `identity`, `downloads.search` | Work |
| Proprietary codecs | Not supported |

The spec's open unknowns: `contextMenus`, `notifications` and `commands`.

infiniterm's conclusion: "CEF upstream closed the related issues as not planned, so the plan is our own CEF build". They rejected a JS shim, because it "cannot reach every context", and render-process injection, because "CEF 152 has no hook for service workers, so backgrounds stay broken".

The planned patches:

| Patch | What it does |
|---|---|
| P1 | `CefBrowserHost::SetExtensionTabInfo(window_id, active)` plus `SetExtensionFocusedWindow`. Adds Alloy contents to `TabsQueryFunction::BuildTabList`, an `AlloyWindowController` registered in `WindowControllerList`, and tab and window events |
| P2 | `CefLifeSpanHandler::OnExtensionCreateTab(...)`, hooked into `TabsCreateFunction` and `WindowsCreateFunction`; the default is to refuse |
| P3 | Action-state observer (icons as bitmaps, badge, title, popup path) and `ClickAction` |
| P4 | Dispatch `commands` |
| P5 | `proprietary_codecs` / `ffmpeg_branding=Chrome` |

- PR #333 holds P1 and P2 as patch files that pass `git apply --check` against branch 7977 but were **never compiled**. Its "Not done" list includes `tabs.update({active})`, `tabs.move`, `tabs.remove`, `windows.update`, `onBoundsChanged`, `runtime.openOptionsPage`, attach/detach/move events, mixed-profile windows, and generated C API wrappers.
- PR #332 (merged) gives a DevTools card through the remote debugging port, after "`ShowDevTools` with a windowless client: does not work".

### 6.2 Other embedder reports

| Source | Report | Answer |
|---|---|---|
| [CEF forum t=20003](https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=20003) (jcam, 2024-10, Linux OSR Alloy) | Opening `default_popup` through `CreateBrowser` shows the popup, but it "does not 'see' the tab information" (Dark Reader) | The two maintainer quotes in §1 |
| [CEF forum t=20002](https://magpcss.org/ceforum/viewtopic.php?t=20002) | — | Same answer |
| [CEF forum t=19898](https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=19898) (OBS, 2024-08) | Extension tabs "break out" into Chrome UI windows even with Alloy style; `--disable-extensions` is the advice for apps that don't want extensions; extensions are on by default | — |
| #3472 (2023, Metamask), #3654 (2024) | — | "Extensions are only supported with the Chrome runtime" |
| #4011 (2025, OSR tabs), #4255 / #4256 (2026, OSR tabs) | Alloy tabs API gaps | See §2.6; #4256's crash was fixed for 154 |
| #4309 (2026-10, service-worker `Origin: null`) | Breaks 1Password | Open |

### 6.3 Changes that add extension support or expose actions

- **Upstream CEF: none.** The only Alloy work since 2024 is `ed079792b6`, minimal tabs for PDF (M125/126), plus the crash-to-error hardening for #4256 (`7ef0d1ef7d` on 8037, `6e5a72b1af` on master, September 2026). CEF master on Chromium 156 has no extension API and the same patch. #3450 and #3877 remain open and unassigned. Magreenblatt's only suggestion on #3877 was "listening for the pref change inside CEF".
- **CrabNebula PR #4187** (2026-06-02, closed by its author 2026-06-03, no review):
  - New `include/cef_extension.h`: `GetIdentifier`, `GetPath`, `GetName`, `GetVersion`, `GetManifest`, `IsEnabled`, `ShouldShowInExtensionsUI`, `GetActionIcon(browser)`, `IsValid`, `IsSame`, `GetRequestContext`, `SetEnabled`, `Uninstall`.
  - New `include/cef_extension_handler.h`: `OnExtensionLoaded`, `OnExtensionUnloaded`, `OnExtensionLoadFailed`, `OnExtensionActionUpdated(extension, browser)`.
  - `CefRequestContext`: `LoadUnpackedExtension` (`--load-extension` semantics), `InstallUnpackedExtension` ("Load unpacked" semantics, persistent), `InstallExtension(crx_path)`, `SetExtensionsHandler`, `HasExtension`, `GetExtensions`, `GetExtension`, `SetExtensionEnabled`, `UninstallExtension`, `ShowExtensionPopup(id, source_browser, anchor_screen_rect)`, `CloseExtensionPopup`.
  - Implementation: `CefAlloyExtensionPopup`, a `views::BubbleDialogDelegate` hosting Chrome's `ExtensionViewHost` with no `Browser`, plus `cef::MaybeAppendAlloyPopupQueryTab` so `tabs.query` run inside a popup returns the source tab.
  - [crabnebula.dev/opensource](https://crabnebula.dev/opensource/) lists their CEF fork as carrying "an Extensions API for browser runtimes that isn't in upstream CEF C++". `github.com/crabnebula-dev/cef` now returns 404, and Tauri's CEF runtime docs ([v3.tauri.app/develop/cef](https://v3.tauri.app/develop/cef/)) do not mention extensions.
  - This is the most useful public API design reference, though its popups are Chrome Views bubbles, not embedder-drawn.
- **No branch** adds Chrome-style OSR, or exposes Chrome's `ExtensionsContainer`, `ToolbarActionsModel` or `ExtensionActionRunner` to embedders.

---

## 7. Design notes for Vsesvit (INFERRED unless marked)

1. **Chrome-style base versus Alloy base for a patch set.** Chrome style already provides:
   - Browser objects, so tabs, windows and events run on Chrome's own code.
   - Chrome's context menu with extension items, already in `CefMenuModel` (VERIFIED).
   - The keybinding registry.
   - Chrome's install and permission dialogs.
   - Web Store installs.
   - `chrome://extensions`.

   The missing pieces are (a) an `ExtensionsContainer` implementation that forwards to the embedder, (b) popup hosting that the embedder can place, and (c) a tab-strip model that matches GPUI's. Chrome's `ExtensionsContainer` is an interface stored as `UnownedUserData` (`extensions_container.cc`, VERIFIED), and `ToolbarActionsModel` / `ExtensionActionViewModel` are UI-agnostic. An Alloy base must re-create the Browser-dependent pieces by hand: infiniterm's P1/P2 (tab and window model, create routing), the context-menu items (CEF's `CefMenuManager` has none), commands, and the WebUI allowlist for `chrome://extensions`.
2. **Getting Chrome's tab model to match GPUI's** is the hard part of a Chrome-style patch set. CEF already supports tabs moving between Browsers: the `ChromeBrowserDelegate` header says "the Browser object will change when the tab is dragged between windows". A patch could create Views- or child-hosted Browsers as `TYPE_NORMAL` with a hidden tab strip, so that one GPUI window is one Chrome `Browser` with N tabs, and expose `TabStripModel` activate/move/close plus an "add tab to this Browser" hook for `tabs.create`. That makes `tabs.*`, `windows.*`, `tabGroups` and `activeTab` behave exactly as in Chrome. The cost is giving up "one CefBrowserView per tab" in CEF's model.
3. **Stock fallback (option B) checklist:**
   - Chrome-style child windows.
   - A GPUI context menu built from `CefMenuModel` through `RunContextMenu`.
   - `chrome://extensions` in a content tab as the management UI.
   - External-extension JSON (`<root_cache_path>/External Extensions/<id>.json` with `external_crx` from Vsesvit's verified CRX store) for persistent installs, or `--load-extension` with directories Vsesvit unpacked.
   - Native messaging manifests copied into `<root_cache_path>/NativeMessagingHosts`.
   - `GetDefaultClient()` plus `OnAfterCreated` to catch Chrome-UI windows that extensions spawn, and redirect their URL into a GPUI tab. This is the OBS workaround: the window flashes, and the extension gets back a tab object that is then closed.
   - Accept or document: no action toolbar, no popups with tab context, no `activeTab` from a click.
4. **Do not rely on** `CefBrowser::GetIdentifier()` being the extension tab ID (§4), or on CDP `triggerAction` without a toolbar (§3.2).
5. **What PLAN.md's install pipeline means under CEF.** Vsesvit's current design gives core the extension install, update, prompt and sync. Under CEF, Chromium's `ExtensionService` owns installation and updates. Either core keeps its pipeline and feeds Chromium (external-extension JSON pointing at verified CRX files, or a patch API like CrabNebula's `InstallExtension`), or Chromium owns installs and core only mirrors state for sync. That mirroring needs a patch or `chrome.management` through a bundled helper extension with native messaging. `storage.sync` sync goes through Chrome Sync, which CEF does not configure (INFERRED), so a helper extension or a patch would be needed to bridge it into Vsesvit's sync records.

---

## 8. Risks

- No embedder API for extension actions, popups, the tab and window model or extension management in any CEF style. Upstream declines it for Alloy and has left #3450 open for over three years.
- Chrome-style embedding inside GPUI needs X11 or XWayland child windows. Wayland embedding (#2804) is unmerged.
- Any CEF fork means building Chromium yourself every milestone (about every four weeks), keeping security patches current, and regenerating the cef-rs bindings. cef-rs supports `CEF_PATH`; check whether its `check_archive_json` version check accepts suffixed builds.
- #4309 (`Origin: null` on extension service-worker fetches) breaks some real extensions (1Password) in every style.
- #3444: CEF request interception plus webRequest has not been fully verified upstream; proxying `TrustedURLLoaderHeaderClient` is still a TODO.
- MV2 is gone in 154, so any MV2-only extension cannot be supported.
- Extension-spawned Chrome-UI windows (`tabs.create`, `windows.create`, `openOptionsPage`, `launchWebAuthFlow`, Web Store install flows) escape the GPUI UI unless patched.
- Chrome install and permission prompts are Views dialogs with no CEF hook. In GPUI they appear as separate Chrome-styled windows.
- The external-extension directory `/usr/share/chromium/extensions` is shared with the system Chromium package, which can inject extensions into Vsesvit.
- A debugging port, if used for CDP `loadUnpacked`, exposes every tab to local processes.
- CEF ships no extension tests (the `tests/ceftests` list in 8037 has none), so extension regressions between CEF versions will not be caught upstream.

---

## 9. Sources

CEF source (branch 8037 = CEF 154.0.34; master at `a875f407e3`):
- https://github.com/chromiumembedded/cef/tree/8037
- https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types_runtime.h
- https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_linux.h
- https://github.com/chromiumembedded/cef/blob/8037/include/internal/cef_types.h (`chrome_policy_id`, `cache_path`, `root_cache_path`, permission types)
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_browser.h (`GetIdentifier`, `ExecuteChromeCommand`)
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_request_context.h
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_browser_process_handler.h
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_command_handler.h
- https://github.com/chromiumembedded/cef/blob/8037/include/cef_permission_handler.h
- https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_browser_view.h
- https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_browser_view_delegate.h
- https://github.com/chromiumembedded/cef/blob/8037/include/views/cef_window_delegate.h
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/extensions/chrome_extension_util.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/alloy/alloy_browser_host_impl.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/alloy/browser_platform_delegate_alloy.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_platform_delegate_create.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/chrome_browser_host_impl.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/chrome_browser_delegate.cc and `.h`
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/browser_delegate.h
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/views/chrome_child_window.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/views/chrome_browser_view.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/chrome/chrome_context_menu_handler.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/menu_manager.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/browser_info_manager.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/browser/devtools/devtools_controller.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/common/resource_util.cc
- https://github.com/chromiumembedded/cef/blob/8037/libcef/common/chrome/chrome_main_delegate_cef.cc
- https://github.com/chromiumembedded/cef/blob/8037/patch/patch.cfg
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_browser_extensions.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_browser_context_menus.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_runtime_views.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_browser_browser.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_browser_dialogs_misc.patch
- https://github.com/chromiumembedded/cef/blob/8037/patch/patches/chrome_browser_policy.patch
- https://github.com/chromiumembedded/cef/blob/8037/tools/gn_args.py (`enable_background_mode: False`)
- https://github.com/chromiumembedded/cef/blob/master/docs/architecture.md (the docs moved from the Bitbucket wiki; https://chromiumembedded.github.io/cef/architecture)
- https://github.com/chromiumembedded/cef/blob/master/docs/chromium_update.md (cefclient and cefsimple style and mode flags)

CEF commits:
- https://github.com/chromiumembedded/cef/commit/ed079792b6e02624bc5c85df959c41118890bd6d (minimal Alloy tabs support, 2024-04-27)
- https://github.com/chromiumembedded/cef/commit/a461a89728054d40eb8ad4c609eaa391cf4ea52e (Alloy bootstrap and `CefExtension` deleted, 2024-07-01)
- https://github.com/chromiumembedded/cef/commit/7ef0d1ef7df8be751a32d6eb38b66e005b9e607b (#4256 fix on 8037)
- https://github.com/chromiumembedded/cef/commit/6e5a72b1afcc60abaa899988cbdbaa4d0c7b7cb6 (#4256 fix on master)

CEF issues and PRs:
- https://github.com/chromiumembedded/cef/issues/3681 , /3685 , /3859 , /3763 , /3285 , /3450 , /3877 , /4011 , /4255 , /4256 , /4309 , /3444 , /3472 , /3654 , /3293 , /3294 , /2804 , /4205 , /3593 , /4106
- https://github.com/chromiumembedded/cef/issues/1947 (maintainer comments 2020-10-08 and 2020-11-16)
- https://github.com/chromiumembedded/cef/pull/4187 (CrabNebula Extensions API)

CEF forum and announcements:
- https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=20003 (maintainer quotes, raw HTML checked)
- https://magpcss.org/ceforum/viewtopic.php?t=20002
- https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=19898 (OBS: extension tabs open Chrome UI windows)
- https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=20156 and t=20295 (Chrome style with `SetAsChild`, popups)
- https://groups.google.com/g/cef-announce/c/s1WaovAopFo/m/LV5eiNX1BgAJ (Alloy bootstrap removal announcement, linked from #3685)

Chromium source, tag 154.0.8037.98 (`https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/<path>`):
- `chrome/browser/extensions/extension_service.cc` (`LoadExtensionsFromCommandLineFlag`)
- `extensions/common/extension_features.cc`
- `chrome/browser/devtools/chrome_devtools_session.cc`, `chrome/browser/devtools/protocol/extensions_handler.cc`, `chrome/browser/devtools/remote_debugging_server.cc`
- `content/public/browser/devtools_agent_host_client.cc`, `content/browser/devtools/protocol/target_handler.cc`, `content/browser/devtools/render_frame_devtools_agent_host.cc`
- `extensions/browser/unpacked_installer.cc`, `chrome/browser/extensions/installed_loader.cc`, `extensions/browser/extension_prefs.cc`, `chrome/browser/extensions/extension_management.cc`
- `extensions/browser/manifest_v2_handler.cc`, `extensions/browser/manifest_v2_util.cc`
- `extensions/common/api/_permission_features.json`, `extensions/common/api/_api_features.json`
- `chrome/browser/extensions/api/tabs/tabs_api.cc`, `chrome/browser/extensions/extension_tab_util.cc`, `chrome/browser/extensions/browser_extension_window_controller.cc`, `chrome/browser/extensions/window_controller.cc`
- `chrome/browser/ui/views/toolbar/toolbar_view.cc`, `chrome/browser/ui/extensions/extensions_container.cc`, `chrome/browser/ui/views/extensions/extension_install_dialog_view.cc`, `chrome/browser/ui/browser_window/internal/browser_window_features.cc`
- `chrome/browser/renderer_context_menu/render_view_context_menu.cc`
- `chrome/common/chrome_paths.cc`, `chrome/browser/extensions/external_provider_impl.cc`, `extensions/common/feature_switch.cc`, `chrome/browser/prefs/chrome_pref_service_factory.cc`
- `chrome/browser/resources/webstore_app/manifest.json`

Chromium commits (GitHub mirror):
- https://github.com/chromium/chromium/commit/290ed8046692651ce76088914750cb659b65fb17 (remove `--load-extension` on Chrome-branded builds, main@1443013)
- https://github.com/chromium/chromium/commit/04f6233ce5be7e5e420418b5286f3b0f87ffc28f (remove `kDisableLoadExtensionCommandLineSwitch`, main@1520577)
- https://github.com/chromium/chromium/commit/dddf05b0e69f88da95c3d6d4698521b775f20c2a (CDP extensions without pipe or unsafe flag, main@1615780)
- https://github.com/chromium/chromium/commit/03338921851f5a0773c4d6b68586133851e2ca4e (CDP extensions uninstalled at next startup, crrev.com/c/7736786)
- https://github.com/chromium/chromium/commit/62a851c41378d9330162ab04616f0c256df3bf1e (CDP cleanup moved to `extension_prefs`)

Embedder reports:
- https://github.com/ekinertac/infiniterm/issues/310
- https://github.com/ekinertac/infiniterm/pull/311 (spec and probe table)
- https://github.com/ekinertac/infiniterm/pull/333 (P1 and P2 patch notes)
- https://github.com/ekinertac/infiniterm/pull/332 (DevTools through the debugging port)
- https://crabnebula.dev/opensource/ and https://v3.tauri.app/develop/cef/

Crate source on disk:
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs` (`CEF_VERSION`, `cef_runtime_style_t`, `cef_chrome_toolbar_type_t`, no extension types)
- `.../cef-dll-sys-154.5.0+154.0.34/build.rs` (`CEF_PATH` support)
