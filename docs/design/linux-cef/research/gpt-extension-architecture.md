# GPUI + CEF on Linux: extension architecture research

Research date: **2026-10-16**. Read-only investigation for the `linux-cef-gpui` worktree. No browser was built or executed; “VERIFIED” means the cited source or discussion was read, **not** that Vsesvit was tested. “INFERRED” marks architectural conclusions and expected behavior requiring a spike. An issue author's reproduction is identified as a report rather than treated as our own test.

## Executive summary and recommendation

**INFERRED — No stock-CEF architecture simultaneously delivers all browser chrome in GPUI, ordinary multi-tab Chrome extension semantics, native Wayland embedding, and total Chrome extension compatibility.** The owner must relax at least one requirement. A JavaScript bridge does not remove this constraint.

**Recommendation, ordered by extension compatibility:**

1. **INFERRED — Maximum compatibility without a fork: C, with real Chrome-style windows and native extension UI.** Let CEF own actual browser windows, actions/popups, extension menus, side panels and permission UI; use GPUI for application surfaces outside that native browser UI. For ordinary multi-tab semantics, prefer a normal Chrome browser window over independently created `CefBrowserView`s. This sacrifices “GPUI for all browser chrome,” but preserves the browser objects extensions actually operate on. A CEF Views window containing one Chrome BrowserView remains a popup-style, generally one-tab browser; Views alone does not fix tab grouping.
2. **INFERRED — Closest integrated GPUI compromise: B on X11/XWayland, preferably retaining a native extension-toolbar island.** Use Chrome-style native child browsers, shared profile/request context, and GPUI navigation/tab UI. Keep enough Chrome UI for real action activation and prompts. Each independently created content browser has its own Chrome Browser/window identity. Extensions see this topology, not Vsesvit's GPUI tab/window grouping. Hiding all Chrome UI loses much of the benefit for actions and prompts.
3. **INFERRED — If all-GPUI chrome and native Wayland composition are non-negotiable: A, advertised as partial compatibility.** OSR is a reasonable rendering architecture, but the extension bridge becomes another substantial compatibility runtime. Native MV3, content scripts and network APIs are valuable improvements over WebKit. They do not imply full browser-level extension support. Keep source extensions untouched wherever possible; do not make install-time rewriting the default strategy.
4. **INFERRED — D is the route to tighter integration, not a bounded finishing task.** A maintained fork could expose native action/permission operations and connect GPUI tabs to Chromium browser models, or add Chrome-style OSR. That buys real capabilities but entails Chromium/CEF release engineering and security updates. It still cannot promise every Chrome extension, deprecated API, proprietary service or third-party integration.

**VERIFIED — The version suffix matters.** The local Rust crate is `cef 154.5.0+154.0.34`; the CEF build index maps CEF **154.0.34+g14c5a08** to Chromium **154.0.8037.98**, not Chromium 154.0.34. Matching CEF branch: **8037**, inspected at `14c5a089e8452874cb1a556dc3fde26ed2d87723`. Master inspected at `a875f407e36bdf4fe6c31622e150ce73f77629c8`, whose compatibility file targets Chromium **156.0.8078.0**. Chromium citations below use the exact .98 version. [S1–S3]

## Per-API compatibility matrix

Legend: **N** = works natively at the engine/model level, subject to normal permissions and version support; **B** = needs an embedder bridge or UI integration; **X** = exact behavior unavailable through the stated stock architecture. **V** = VERIFIED source path/limitation; **I** = INFERRED integration outcome. **N\*** is conditional, unsupported or requires a spike; it is not a compatibility guarantee. A “B/X” cell means useful emulation is feasible but complete native semantics are not.

Architectures: **A** = Alloy OSR + GPUI + extension shims; **B0** = separate Chrome-style X11 children, all native toolbar UI hidden; **C** = Chrome/Views-owned windows retaining native extension UI, GPUI companion surfaces; **D** = custom CEF/Chromium integration patches. C distinguishes a Views BrowserView from a normal multi-tab Chrome window where necessary. D describes possibilities, not an existing implementation.

| API area / behavior | A: Alloy OSR + bridge | B0: Chrome children, toolbar hidden | C: native Chrome/Views UI hybrid | D: patched integration |
|---|---|---|---|---|
| MV3 service workers, runtime, alarms, local/session storage, manifest content scripts | N\* V: Chrome profile runtime, unsupported Alloy combinations | N I | N I | N I |
| `storage.sync` across Vsesvit devices | B I: Chromium local API is not Vsesvit sync | B I | B I | B I |
| DNR and profile/network interception | N\* V: explicitly identified as working category; test tab-scoped conditions | N I | N I | N I |
| `webRequest` blocking and MV2 | X I where Chromium 154 removes/restricts them; bridge cannot restore removed engine support | Same X I | Same X I | B/X I: requires additional maintained changes |
| `tabs.query`, windows enumeration, active/current window | B V/I: Alloy absent from Chrome Browser enumeration | N V for actual Chrome objects; X I for GPUI grouping without virtualization | N V for actual objects; Views-per-view still fragmented | B I: integrate native models |
| `tabs.create/update/move/remove`, pin/highlight/discard and lifecycle events | B/X V/I: WebContents operations sometimes native; tab-strip operations absent | N\* I for native topology; B/X I to preserve GPUI topology/lifecycle | N\* I; normal Chrome multi-tab window is strongest | B I |
| `scripting.executeScript`, registered scripts, CSS; explicit real tab ID + existing host access | N\* V: Alloy lookup + TabHelper executor exist | N V | N V | N I |
| `tabs.sendMessage/connect`, content-script messaging | N\* V: real tab ID; Alloy success reported | N I | N I | N I |
| `activeTab` from a GPUI action/menu/shortcut | X V/I for native grant via ordinary GPUI click; B I only by emulating privileged consumers too | B/X I with all native activation paths hidden; native invocation needed | N V/I through actual Chrome action/menu/command invocation | B I: expose real grant/action hooks |
| `debugger.attach({tabId})` | N\* V: WebContents lookup exists; authorization/warning UI still needs validation | N\* I: native warning/consent surfaces must remain usable | N\* I | N/B I |
| `tabCapture.capture()` implicit active tab | X V: searches a Chrome Browser and native invocation grant | N\* I with native grant and correct real active browser | N\* I | B I |
| `tabCapture.getMediaStreamId({targetTabId})` | N\*/X V: lookup can work, but native per-tab capture grant is still required | N\* I with grant; GPUI click alone insufficient | N\* I | B I |
| `action` icon/title/badge/enabled state | B V/I: native state has no OSR toolbar; no general public observer API | B V/I: state exists, toolbar hidden | N V with visible Chrome toolbar | B I: expose native state/events |
| Action click, popup, `getViews`, popup lifetime/current tab semantics | B/X V/I: opening popup URL is only an ordinary extension page | B/X V/I: hidden toolbar removes normal interaction; CDP action path conditional | N\* V/I with actual toolbar; popup-only windows need testing | B I |
| Page `contextMenus` items and real dispatch | B V/I: Alloy menu manager does not populate Chrome extension items | N V/I if native Chrome menu model/callback execution retained | N V/I | B I for GPUI presentation |
| Action-button `contextMenus` items | B/X I: shim menu and listeners, no native action UI | X/B I: native button absent; bridge or restore native UI | N\* I with real action button | B I |
| `commands`, `_execute_action`, global shortcuts | B/X I: emulate focus, dispatch, native grant and worker wakeup | N\*/B I: CEF child focus/native handling; GPUI focus requires integration | N\* I; compositor/global shortcut limits apply | B I |
| `tabGroups`, group operations/events | B/X V/I: no native group model for GPUI tabs | N\*/X I: real groups only; separate one-tab Browsers cannot represent one GPUI group's tabs | N\*/X I for Views-per-view; N\* I for normal Chrome multi-tab windows | B I |
| `sidePanel` options/open/close, action behavior and lifecycle | B/X V/I: panel page can be shown, native coordination absent | N\*/B I inside Chrome child; cannot simply move native side panel into GPUI | N\* I where native side-panel UI exists; verify popup Browser support | B I |
| Core CRX download/verification and custom install consent | B V/I: keep core, explicitly load unpacked | B V/I | B V/I or use native install flows if supported | B I |
| `permissions.request`, withheld hosts, permission/installation prompts | B/X I: native grant state must match Rust decisions | N\*/B I: native UI works only when attached/visible/usable; GPUI replacement lacks complete hooks | N\* I via native UI; spike required for popup-style views | B I: grant/consent API |
| Native messaging / devtools extensions | N\*/B I: profile/platform dependent, devtools UI especially constrained | N\*/B I: native host discovery/devtools integration must be checked | N\*/B I; normal Chrome devtools strongest | B I where additional support needed |
| Single surface composed by GPUI on native Wayland | N I for OSR composition | X V: stock native child path is X11 | X V/I for embedding; separate CEF top-levels are a different design | B I: Wayland embedding/rendering work |
| Unmodified Web Store extensions + all-GPUI UI + ordinary Chrome semantics | X I | X I | X I if all chrome must be GPUI | B/X I: substantial fork; never universal Chrome parity |

**INFERRED — “N” does not mean “all APIs in that namespace.”** Exact permission checks, API availability, incognito mode, frame/document IDs, worker lifetime, user gestures, enterprise policy and platform support still apply. None of these options automatically connects extension bookmarks/history/preferences to Vsesvit's SQLite core. Native Chrome profile data and Vsesvit data need an explicit ownership/synchronization decision.

## Verified architectural boundaries

### Runtime bootstrap is not browser style

**VERIFIED.** `cef_runtime_style_t` is now in `include/internal/cef_types_runtime.h`, rather than defined directly in `cef_types.h`. Its documentation says: “Windowless rendering will always use Alloy style.” It also allows Chrome-style native-parent windows and states that a Chrome-style Views window hosts at most one Chrome BrowserView, although it may host multiple Alloy BrowserViews. Linux `CefWindowInfo` repeats the OSR restriction. The Rust crate's `set_as_windowless` helper sets `RuntimeStyle::ALLOY`. Setting CHROME afterwards cannot turn stock OSR into Chrome style. [S3–S5]

**VERIFIED.** #3681 introduced mixed styles under Chrome bootstrap; #3685 removed the old Alloy bootstrap at M128. The migration issue and May 2024 cef-announce post explicitly distinguish the bootstraps/styles and extension support. Upgrading to “Chrome runtime” therefore does not supply Chrome window objects for Alloy OSR. [S6–S8]

**VERIFIED.** In #3859 the maintainer says RequestContext-level extension functionality such as network interception should work with Alloy, while Chrome UI-dependent functionality will not be supported. In October 2024's forum thread he says supported extensions require Chrome-style windows showing some Chrome UI, and Alloy cases are the embedder's responsibility to test/support. In May 2025 he identifies a Views-hosted Chrome BrowserView/Window showing the toolbar as the way to access extension icons. [S9–S11]

### Why enumerating tabs differs from targeting a known tab

**VERIFIED.** Chromium's `BrowserExtensionWindowController` registers itself in `WindowControllerList`, stores the actual Browser session ID, and obtains its `TabListInterface`. `TabsQueryFunction::BuildTabList` enumerates Chrome BrowserWindowInterfaces. `ExtensionTabUtil::CreateTabObject` derives activation, selection, pinning and group membership from real tab interfaces. An Alloy WebContents does not acquire these structures merely because it has a `CefBrowser`. [S12–S14]

**VERIFIED.** Stock CEF already patches `ExtensionTabUtil::GetTabById` to call `cef::GetAlloyTabById`. That fallback scans live RenderProcessHosts/frames, matches the profile and Chromium SessionTabHelper ID, and returns an Alloy WebContents. The patch explicitly says the output window/index do not exist for Alloy. Alloy also calls `TabHelpers::AttachTabHelpers`. This is why `tabs.get(realId)` and APIs operating on that WebContents can work while `tabs.query` cannot discover it. [S15–S16]

**VERIFIED — reports, not local reproductions.** Infiniterm #310 reports empty `tabs.query`/`windows.getAll`, external native windows for creation, and undrawn action state on CEF 152 OSR. CEF #4255 reports a real `sender.tab.id`, successful `tabs.get`, `tabs.sendMessage`, `scripting.executeScript`, zoom and reload, but failed `tabs.update`, on CEF 147. The maintainer closed #4255 as not planned, saying Alloy only supports extension APIs needed by embedded components such as PDF Viewer. Do not generalize its version-specific update failure to every newer build. [S17–S18]

**VERIFIED.** #4256 reported `autoDiscardable` crashing Alloy. A September 2026 maintainer comment confirms the null native lifecycle object. **The exact CEF 154.0.34 source inspected already guards the missing lifecycle object and missing opener tab**, returning extension errors. Master also has these guards. This particular crash is not evidence that the installed .34 build still crashes. The missing tab-strip/lifecycle semantics remain. [S15, S19]

## A: Alloy OSR plus an embedder bridge

### What can stay native

**VERIFIED.** `scripting::CanAccessTarget` calls `ExtensionsBrowserClient::IsValidTabId`, then asks for the WebContents' script executor. Chrome's browser-client implementation delegates the validity lookup to `ExtensionTabUtil::GetTabById` and gets the executor from extension `TabHelper`. Together with CEF's Alloy fallback/helpers, these are concrete native paths for script injection into Alloy tabs. Permission checks still run per target frame. [S16, S20]

**INFERRED.** A bridge should preserve Chromium's **real extension tab IDs**. Map GPUI tab IDs to those IDs, rather than handing native APIs arbitrary Rust counters. A small content script reporting `sender.tab` is one possible discovery mechanism; unrestricted extensions may not be able to inject such a script on every page, so browser-side CDP/target correlation needs a separate proof. Never assume `CefBrowser::GetIdentifier`, a CDP target ID and an extension tab ID are interchangeable. Native messaging and scripting can then remain native when permissions already suffice.

**VERIFIED.** `debugger`'s `InitAgentHost` resolves an explicit tab ID with `ExtensionTabUtil::GetTabById` and constructs a WebContents DevTools agent host. Consequently there is no universal “Alloy tab IDs cannot work for debugger” rule. [S21]

**INFERRED.** Debugger attach success, warnings, session ownership, restricted URLs, frames and detach behavior need a Linux OSR test. Native debugger access is plausible; complete compatibility is not verified merely from the lookup. Reimplementing debugging through the embedder's CDP connection is a separate privileged broker and must preserve extension permission/consent checks.

### What a UI bridge would have to implement

**INFERRED.** A usable bridge needs a coherent virtual windows/tabs model, events and ordering; action state/defaults/per-tab overrides; action popup hosting and closure; context menu persistence/matching/click dispatch; command resolution; tab groups; side panel coordination; install/optional permission consent; incognito visibility; and worker restart recovery. It must handle callbacks, Promises, `runtime.lastError`, ports, extension updates and multiple JS contexts. These are browser behaviors, not simply a set of Rust RPC endpoints.

**VERIFIED.** Alloy uses CEF's `CefMenuManager`, whose menu construction contains standard page items and renderer custom items. Chrome's `RenderViewContextMenu` separately uses `ContextMenuMatcher` and populates extension items. Merely drawing the Alloy menu in GPUI does not introduce Chrome extension context menus. Chrome menus, by comparison, can retain native dispatch even if an embedder customizes presentation. [S22]

**INFERRED.** Shimming `contextMenus.create/update/remove` can build a GPUI menu, but must also replace corresponding listeners and persistence, resolve clicked frames/documents and invoke the worker. It cannot call a public CEF “execute extension menu item” method that does not exist. Similar issues apply to firing native `commands.onCommand` or `action.onClicked`: a JavaScript event facade is not Chromium's internal event routing/wakeup system.

### The hard boundary: activeTab and capture grants

**VERIFIED.** Chromium's `ActiveTabPermissionGranter::GrantIfRequested` installs actual per-tab API/host permissions, including the special `kTabCaptureForTab` grant. Script injection checks native permissions; a JavaScript `active: true` field or synthetic event does not change these grants. Public CEF headers inspected do not expose a general extension active-tab grant API. [S23]

**INFERRED.** GPUI clicking an icon then calling a shim listener does **not** grant native `activeTab`. Opening an extension popup's URL also does not reproduce native action invocation. A shim can fake permissions and execute code through the embedder, but then every dependent native API must be intercepted too: scripting, sensitive tab information, capture, DNR feedback, host access and permission changes. Broadening the extension manifest to `<all_urls>` changes the permission model; it is not exact `activeTab` compatibility.

**VERIFIED.** `tabCapture.capture()` selects the last active Chrome Browser and its active native tab. `getMediaStreamId` can instead resolve an explicit `targetTabId` via `ExtensionTabUtil`, **but both check `kTabCaptureForTab` or the extension allowlist**. Thus explicit IDs are not categorically impossible on Alloy; normal native invocation grants are the missing link. An allowlist bypass is not a general replacement for per-user consent. [S24]

**INFERRED.** A screenshot API or CDP screencast is not an equivalent tabCapture MediaStream with Chrome's stream IDs, audio routing, consumer-origin/process constraints and permission revocation. Emulating that stack without native hooks is not a credible route to total compatibility.

### Install-time shims, content verification and MV3 CSP

**VERIFIED.** Vsesvit's install code verifies CRX3 then rewrites manifest `key`; the directory and cached manifest are treated as immutable. This was confirmed in `crates/vsesvit-core/src/extensions/{crx,install,mod}.rs`, including `install.rs::set_key`. A `key` preserves extension identity; it is not a new store signature for rewritten code.

**VERIFIED.** Chromium distinguishes **CRX package authenticity** from **post-install content verification**. `ChromeContentVerifierDelegate::GetVerifyInfo` ordinarily selects no content verification for a non-auto-updateable manifest location. `Manifest::IsAutoUpdateableLocation` includes internal/external installs, not unpacked/command-line developer installs. Thus a Web Store-downloaded CRX, verified by Vsesvit and subsequently loaded as unpacked, is not automatically a Web Store-installed extension for verifier purposes. Repair policy is an earlier override and must be considered. [S25]

**VERIFIED.** `ContentVerifier::VerifiedFileTypeHelper` explicitly excludes manifest.json because the browser/store can modify it, but includes service worker/background script files. For an installation under store content-verification enforcement, rewriting verified scripts invalidates their hashes; preserving the ID or editing `computed_hashes.json` does not produce the store's signed expected hashes. [S26]

**INFERRED.** Therefore “all modifications are inevitably disabled by the verifier” is too broad, and “CRX verification makes modified code still store-authentic” is also false. An unpacked transformed copy may load without a CEF fork, but the shipped executed program has been changed by Vsesvit. Keep the verified original immutable and, only if explicitly choosing this architecture, create a separately versioned derived runtime copy with transformation provenance. Do not mutate the core installation in place or turn off verification globally.

**VERIFIED.** MV3 extension pages have a minimum CSP restricting executable code sources; adding `unsafe-eval` or arbitrary remote script sources is rejected. A packaged shim loaded from the extension's own origin can comply; remotely fetching and evaluating a shim cannot be assumed to comply. [S27]

**INFERRED.** Worker rewriting must handle classic and module service workers, initialization order, imports, cached registrations and updates. A packaged wrapper is technically feasible. Extension `connect-src` restrictions may still block a loopback WebSocket/custom-scheme transport; CSP `script-src` and network transport authorization are different questions. Page/frame hooks do not solve worker bootstrapping. Any transport must authenticate extension identity, enforce host/incognito permissions, avoid web-page access and survive worker suspension. Native messaging requires the extension's permission and an allowed native host; it is not a transparent transport for arbitrary unmodified extensions.

### CDP and renderer hooks do not substitute for native browser objects

**VERIFIED.** `CefRenderProcessHandler::OnContextCreated` takes a CefBrowser/CefFrame and documents a frame context callback. The public handler has no corresponding service-worker-start/context callback. It can assist with browser-owned frames; it is not a general before-first-script extension-worker hook. [S28]

**INFERRED.** CDP Target auto-attach plus Runtime evaluation can instrument some extension targets without changing installed bytes. Reliable operation requires attaching before extension code executes on every restart, binding all relevant targets/worlds, avoiding competing debugger sessions and reconnecting after suspension. No evidence here establishes that as a complete production API override mechanism. A debugger can evaluate JS despite ordinary script-loading restrictions; this does not relax native extension authorization checks or create WindowControllers.

**VERIFIED.** Chromium 154 has an experimental CDP `Extensions` domain with unpacked-extension management and an action trigger. Its `TriggerAction` implementation requires a **tab target**, finds the associated **BrowserWindowInterface**, obtains its **ExtensionsContainer**, then calls the native action model's `ExecuteUserAction`. Its handler enables management/action operations for a browser target session. This is not a general Alloy action hook: the Alloy WebContents lacks the Browser required by this code. [S29]

**INFERRED.** CDP is worth spiking for B/C action invocation without extension rewriting, but the existence of the method is not proof it works with absent/hidden toolbar containers or every popup-style Browser. It exposes neither a general GPUI-ready stream of action icons/badges nor complete extension UI state. Do not execute this path on Alloy assuming it safely degrades; the inspected native Browser/container dereferences are not a compatibility fallback.

## B: Chrome-style native children of a GPUI X11 window

**VERIFIED.** CEF's `chrome_child_window.cc` implements native-parent Chrome browsers using an internally created BrowserView and CefWindow; its parent widget comes from `CefWindowInfo.parent_window`. Linux's native parent path uses X11 handles. This is a stock path, not a need for a modified CEF build. #3685 also lists Chrome-style external native parent operation on Windows/Linux. [S7, S30]

**VERIFIED.** `ChromeBrowserHostImpl::Create` creates a Browser, adds its initial real tab and gets that tab's CefBrowser host. The native-child route creates a BrowserView. For Views-hosted creation, `CreateBrowser` changes normal Browser type to **TYPE_POPUP**, marked trusted to suppress title/address controls. The source comments say Views-hosted Browsers contain a single host; ordinary non-Views Chrome windows may contain a host per tab. [S31]

**INFERRED.** If GPUI has one visual window with ten independently created Chrome child browsers, extensions ordinarily see ten Chrome windows, each with one tab. `tabs.query({active:true})` can yield an active tab in each real Browser. A query filtered by current window need not mean “the selected tab in this GPUI window”; focus/last-active bookkeeping follows the native Browser. Popup window type can also make extensions filtering `windowType: 'normal'` miss these views. Hiding nine child windows does not merge their WindowControllers.

**INFERRED.** Native scripting, messaging and much of `tabs/windows` work better than A because the real objects exist. But extension `windows.remove/update`, `tabs.move`, new windows, grouping, focus and close must be reflected in GPUI. Independently virtualizing `tabs.query` is insufficient: `sender.tab`, native API results, actions, side panels and events also describe the real Chrome topology. Bridging only enumeration creates conflicting identities rather than Chrome compatibility.

**VERIFIED.** In Views hosting, `GetChromeToolbarType` defaults to NONE; `ChromeBrowserView::OverrideCreateToolbar` creates/exposes the toolbar for NORMAL/LOCATION modes. `CefBrowserView::GetChromeToolbar` provides the native view; `CefCommandHandler` can hide selected built-in buttons/page-action icons. These are controls over native UI, not a replacement extension-action C API. [S32]

**INFERRED.** A visible native toolbar preserves real button activation, popup anchoring, action menus and native grants. Hiding it cannot preserve those interactions just by loading `default_popup` into another browser. Page context menus have a better story: preserve Chrome's native model and callback dispatch, optionally render that supplied model in GPUI. Native action context menus still require a real action control or additional integration.

**Recommendation within B — INFERRED.** Prefer a deliberately visible Chrome extension-control area, minimizing other Chrome controls through supported hooks, over an invisible toolbar plus a speculative synthetic action bridge. The automatic native-child delegate does not expose every custom BrowserView choice; confirm the stock embedding/control arrangement in a small spike before promising an extension-only island. The island is a compromise with “all chrome in GPUI.”

**VERIFIED.** Native Wayland child embedding is not established by this X11 path. CEF #2804 tracks Ozone/Wayland; PR #4233 is **open**, proposing shared `wl_display`, parent `wl_surface`, popup parent API and a Chromium patch. The proposal explicitly explains that surfaces are scoped to connections. It is not present in the inspected stock branch/master. [S33]

**INFERRED.** B therefore requires both GPUI and CEF on X11, commonly through XWayland on the owner's desktop. A Wayland GPUI `wl_surface` cannot be passed as an XID. XWayland gives an integration route, not native Wayland subsurface composition. Native children also introduce separate GPU surfaces/stacking, focus, IME, scaling and popup placement concerns; a GPUI overlay cannot be assumed to paint over them like an OSR texture. No GPUI backend-forcing API was verified in this investigation.

## C: Chrome-style Views/content windows plus GPUI elsewhere

**INFERRED.** This has two substantially different variants:

- **C1: one Chrome BrowserView per content window**, native toolbar retained. This improves UI/lifecycle integration over B0 and can use separate CEF-owned top-level windows, but still has popup-style one-view Browser semantics. One BrowserView in one CefWindow is not the same as multiple Chrome tabs managed by GPUI. The documented limit of one Chrome BrowserView per Chrome Views window rules out placing a GPUI-like grid of Chrome BrowserViews there.
- **C2: an ordinary Chrome-style native browser window with its native tab strip/extensions UI**, plus GPUI companion windows/panels/settings. This preserves the closest available Chrome window/tab model. It compromises the GPUI browser-chrome requirement most visibly and is the strongest baseline for extension compatibility.

**INFERRED.** Keeping a hidden Chrome window solely for `chrome://extensions`, prompts or background work can help manage a shared profile; it does **not** turn separate OSR WebContents into tabs in that hidden window. Extension enumeration may see the helper window instead. A shared RequestContext shares extension installation, storage and network policies, not tab-strip membership. The May 2025 forum reports the on-demand management-window pattern, without claiming full OSR compatibility. [S11]

**INFERRED.** Separate native top-level CEF windows avoid the *embedding* obstacle on Wayland, but Wayland does not generally let a GPUI companion position itself as a perfectly synchronized part of another process/toolkit's window. Treat that as multi-window UX, not seamless composition. Actual CEF distribution/Ozone/backend support must be tested. An X11 reparent/crop of an ordinary whole Chrome window is another possible experiment, but there is no verified supported stock CEF contract here for hiding all of its native UI while retaining full behavior.

## D: what a maintained fork would buy and cost

**INFERRED.** There are two main engineering directions:

1. Connect GPUI's tabs/windows to real Chrome Browser/TabStrip/TabInterface objects, preserving those objects' observers and lifecycle, while exposing layout/action/menu/permission hooks through CEF's C API and Rust bindings.
2. Keep Alloy WebContents and add a virtual extension window/tab integration in Chromium: WindowControllers, enumeration, activation, native grant operations, tab lifecycle, groups, menus, command routing and side panel support. Existing Alloy tab lookup is only a small part of this work.

**INFERRED.** Both also need rendering/hosting work: Chrome-style OSR/compositor support if one GPUI surface is mandatory, or native Wayland embedding if that is the desired host model. #4233 demonstrates the latter's scope and explicitly acknowledges milestone rebasing costs; it is not itself an extension compatibility patch. [S33]

**INFERRED.** Benefits include unmodified extension workers, real `activeTab`/capture grants, native event/permission routing, action-state observation, permission dialogs implemented in GPUI with native decisions, and coherent tab/window identities. The cost includes Chromium checkout/build infrastructure, distribution per architecture, sandbox/GPU/Ozone testing, CEF API/ABI and Rust-binding changes, automated extension regression suites and prompt security-release rebasing. There is no defensible precise person-week estimate from this read-only review; an “add a few tab APIs” estimate would omit most dependencies.

**VERIFIED / INFERRED.** The Alloy special cases, recently hardened lifecycle dereferences and changing Chromium tab interfaces are visible maintenance examples (VERIFIED). Extending that surface materially increases long-term integration work (INFERRED). Prefer contributing narrowly scoped upstream APIs where feasible; upstream acceptance cannot be scheduled or assumed.

## Remaining gaps even in the recommended native option

**VERIFIED — report.** CEF #4309, filed 2026-10-12 and still open when read, reports extension worker cross-origin fetch acquiring `Origin: null`, breaking 1Password account setup. It reproduces on CEF 148, including windowed operation. It is evidence that Chrome style does not eliminate all CEF-specific extension defects; it is **not** a verified CEF 154/Linux reproduction. The report's blanket claim that the app cannot intercept worker requests must also be read in light of the newer CEF 154 service-worker-handler fallback fix. [S34]

**INFERRED.** Stock Chromium version/platform restrictions remain for MV2, `webRequestBlocking`, Chrome-only/private APIs, enterprise-policy APIs, Google identity/sync services, external native host integration and proprietary components. Vsesvit must test claimed support rather than inheriting a “total” label. Preserving a store ID does not guarantee native host discovery, store installation provenance or third-party attestation requirements. Native install management must coexist deliberately with the core's immutable unpacked installation/update pipeline.

**INFERRED — decision gates for implementation, not work performed here:**

1. Establish unmodified MV3 baseline on exact CEF .34 using native Chrome windows: worker/content script messaging, scripting, DNR and extension UI.
2. Compare Alloy OSR with native X11 children using real `sender.tab` IDs, enumeration/currentWindow, group/move/remove and event ordering.
3. Use an `activeTab`-only extension and a tabCapture extension. Compare a real native action click with opening its popup URL from GPUI; verify native permission state and revocation.
4. Exercise action `setIcon`/badges/per-tab overrides, popup API calls, page/action context menus, commands and sidePanel in popup-style BrowserViews. Separately validate CDP action triggering with hidden and visible native toolbars.
5. Test optional permissions, withheld host permissions, incognito and extension restart/update, plus external native messaging, DevTools extensions and 1Password worker fetch behavior.
6. Test XWayland focus, text input, scaling, popup placement, overlays and CEF/GPUI message-loop integration. Select an architecture only after these results make the accepted incompatibilities explicit.

## Sources read

Source links are public URLs; CEF source links are pinned to the exact matching branch head except where master is explicitly discussed. Source files were downloaded under `/tmp/vsesvit-research-gpt/`; no checkout was built or run. Web search used Brave and CEF's own forum search; Google/Bing attempts were unhelpful. No conclusion relies solely on a search snippet.

- **S1:** [CEF binary build index](https://cef-builds.spotifycdn.com/index.json), mapping `154.0.34+g14c5a08+chromium-154.0.8037.98`; local crate `/home/dy/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cef-154.5.0+154.0.34`.
- **S2:** [CEF 8037 Chromium compatibility](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/CHROMIUM_BUILD_COMPATIBILITY.txt); [master compatibility at inspected commit](https://github.com/chromiumembedded/cef/blob/a875f407e36bdf4fe6c31622e150ce73f77629c8/CHROMIUM_BUILD_COMPATIBILITY.txt).
- **S3:** [CEF 154 runtime styles](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/include/internal/cef_types_runtime.h); [master runtime styles](https://github.com/chromiumembedded/cef/blob/a875f407e36bdf4fe6c31622e150ce73f77629c8/include/internal/cef_types_runtime.h).
- **S4:** [Linux cef_window_info_t](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/include/internal/cef_types_linux.h); [C++ Linux WindowInfo helper](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/include/internal/cef_linux.h). Also inspected `include/internal/cef_types.h`.
- **S5:** [BrowserView delegate](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/include/views/cef_browser_view_delegate.h). The requested `include/cef_browser_view_delegate.h` lives under `include/views/`.
- **S6:** [#3681: lightweight Alloy windows under Chrome bootstrap](https://github.com/chromiumembedded/cef/issues/3681).
- **S7:** [#3685: remove Alloy bootstrap, migration and supported modes](https://github.com/chromiumembedded/cef/issues/3685).
- **S8:** [cef-announce: Alloy style / bootstrap migration](https://groups.google.com/g/cef-announce/c/s1WaovAopFo/m/LV5eiNX1BgAJ); [architecture documentation](https://chromiumembedded.github.io/cef/architecture). Historical wording in the architecture page is subordinate to current headers/migration issue.
- **S9:** [#3859: OSR chrome://extensions request and maintainer response](https://github.com/chromiumembedded/cef/issues/3859).
- **S10:** [CEF forum: Queries on chrome extensions, October 2024](https://magpcss.org/ceforum/viewtopic.php?f=6&t=20003).
- **S11:** [CEF forum: Interaction with Chrome extensions, May 2025](https://magpcss.org/ceforum/viewtopic.php?f=6&t=20278); [older Chrome-vs-Alloy support statement](https://magpcss.org/ceforum/viewtopic.php?f=6&t=19510).
- **S12:** [Chromium WindowController](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/window_controller.cc); [BrowserExtensionWindowController](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/browser_extension_window_controller.cc).
- **S13:** [Chromium ExtensionTabUtil](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/extension_tab_util.cc).
- **S14:** [Chromium tabs API, especially BuildTabList / Update](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/api/tabs/tabs_api.cc).
- **S15:** [CEF 154 extension patches](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/patch/patches/chrome_browser_extensions.patch); [master extension patches](https://github.com/chromiumembedded/cef/blob/a875f407e36bdf4fe6c31622e150ce73f77629c8/patch/patches/chrome_browser_extensions.patch).
- **S16:** [CEF GetAlloyTabById](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/chrome/extensions/chrome_extension_util.cc); [Alloy AttachHelpers](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/alloy/browser_platform_delegate_alloy.cc).
- **S17:** [Infiniterm #310](https://github.com/ekinertac/infiniterm/issues/310).
- **S18:** [CEF #4255: get/sendMessage/scripting work, query/update gaps](https://github.com/chromiumembedded/cef/issues/4255).
- **S19:** [CEF #4256: missing lifecycle object and crash hardening](https://github.com/chromiumembedded/cef/issues/4256).
- **S20:** [Chromium scripting API](https://github.com/chromium/chromium/blob/154.0.8037.98/extensions/browser/api/scripting/scripting_api.cc); [scripting utils](https://github.com/chromium/chromium/blob/154.0.8037.98/extensions/browser/scripting_utils.cc); [ChromeExtensionsBrowserClient](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/chrome_extensions_browser_client.cc).
- **S21:** [Chromium debugger API, InitAgentHost](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/api/debugger/debugger_api.cc).
- **S22:** [Alloy menu manager](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/menu_manager.cc); [Chrome context menu](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/renderer_context_menu/render_view_context_menu.cc); [CEF Chrome context-menu delegation](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/chrome/chrome_web_contents_view_delegate_cef.cc).
- **S23:** [ActiveTabPermissionGranter](https://github.com/chromium/chromium/blob/154.0.8037.98/extensions/browser/permissions/active_tab_permission_granter.cc).
- **S24:** [Chromium tabCapture API](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/api/tab_capture/tab_capture_api.cc).
- **S25:** [Chrome content-verifier delegate](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/chrome_content_verifier_delegate.cc); [Manifest auto-updateable locations](https://github.com/chromium/chromium/blob/154.0.8037.98/extensions/common/manifest.h).
- **S26:** [ContentVerifier, especially VerifiedFileTypeHelper](https://github.com/chromium/chromium/blob/154.0.8037.98/extensions/browser/content_verifier/content_verifier.cc).
- **S27:** [Chrome extension CSP reference](https://developer.chrome.com/docs/extensions/reference/manifest/content-security-policy).
- **S28:** [CefRenderProcessHandler frame callbacks](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/include/cef_render_process_handler.h).
- **S29:** [Chromium 154 CDP ExtensionsHandler](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/devtools/protocol/extensions_handler.cc); [browser-target handler gating](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/devtools/chrome_devtools_session.cc); [current public protocol schema](https://github.com/ChromeDevTools/devtools-protocol/blob/master/json/browser_protocol.json). Current schema is supplemental; the .98 implementation establishes the action path.
- **S30:** [CEF Chrome native child window](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/chrome/views/chrome_child_window.cc); [native Linux delegate](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/native/browser_platform_delegate_native_linux.cc).
- **S31:** [ChromeBrowserHostImpl, Create/CreateBrowser](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/chrome/chrome_browser_host_impl.cc).
- **S32:** [CEF ChromeBrowserView toolbar creation](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/libcef/browser/chrome/views/chrome_browser_view.cc); [GetChromeToolbar](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/include/views/cef_browser_view.h); [CefCommandHandler](https://github.com/chromiumembedded/cef/blob/14c5a089e8452874cb1a556dc3fde26ed2d87723/include/cef_command_handler.h).
- **S33:** [#2804 Ozone/Wayland](https://github.com/chromiumembedded/cef/issues/2804); [open PR #4233: Wayland native embedding](https://github.com/chromiumembedded/cef/pull/4233).
- **S34:** [#4309: extension worker Origin:null / 1Password](https://github.com/chromiumembedded/cef/issues/4309); [CEF 8037 commit history, including service-worker request-handler fix](https://github.com/chromiumembedded/cef/commits/8037/).
- **S35:** [Chromium sidePanel API](https://github.com/chromium/chromium/blob/154.0.8037.98/chrome/browser/extensions/api/side_panel/side_panel_api.cc), read for native gesture and panel-service routing; UI behavior on CEF popup-style windows remains untested.
