//! WebExtensions runtime for WebKitGTK. WebKitGTK has no browser-extension runtime, so
//! this crate implements the subset Vsesvit supports on Linux, the way GNOME Web does:
//! content scripts as user scripts in a per-extension isolated world, a hidden background
//! web view, `chrome.*` bridged over `postMessage` replies, `chrome-extension://` served
//! from a custom URI scheme, and declarativeNetRequest translated to content blockers.
//!
//! Platform-neutral pieces compile and test everywhere: [`cookies`] (what `chrome.cookies`
//! reaches, stores and reports), [`dnr`] (the translator),
//! [`dnr_rules`] (the rules the declarativeNetRequest API changes), [`dynamic_scripts`]
//! (the content scripts `scripting.registerContentScripts` adds),
//! [`protocol`] (the JS/Rust wire format), [`messaging`] (port channels), [`menus`] (context
//! menu items), [`notifications`] (what an extension notification shows), [`patterns`],
//! [`mime`], [`i18n`], the tab
//! types in [`tabs`], the window types and events in [`windows`], a tab's frames and their
//! navigation events in [`web_navigation`] and [`support`] (which of a manifest's requests this runtime lacks).
//! The WebKit glue ([`Runtime`]) is Linux only.
//!
//! # API for the GTK shell (Linux)
//!
//! ```ignore
//! use vsesvit_webext::{Runtime, TabHost, TabId, TabInfo, ActionInfo};
//!
//! // Once per process, before any tab WebView exists. `host` answers tab questions; it may
//! // hold only a Weak reference back to the runtime. The runtime registers the
//! // `chrome-extension` scheme on `WebContext::default()`, so tab views must use that
//! // context (the builder default) and `session`.
//! let runtime = Runtime::new(profile.clone(), &network_session, host);
//!
//! // Every tab WebView is built with the runtime's UserContentManager for that tab, which
//! // knows whether the tab is private (a private tab's view has an ephemeral session):
//! let view = webkit::WebView::builder()
//!     .network_session(&network_session)
//!     .user_content_manager(&runtime.user_content_manager(tab_id, Browsing::Normal))
//!     .build();
//!
//! // Its navigation policy is a `gate::Gate` per view, which the runtime answers for (see
//! // there): it refuses a NavigationAction or NewWindowAction the gate refuses, `create`
//! // returns no view for such a target, and a window.open view starts from
//! // `Gate::opened_by` its opener's. The shell reports the loads it starts and every commit.
//! gate.browser_load(&url); view.load_uri(&url);
//! if !gate.decide(&runtime, &target, action.is_redirect(), new_window) { decision.ignore(); }
//! gate.committed(&runtime, &view.uri().unwrap());     // on LoadEvent::Committed
//!
//! // `create` goes through `connect_create`, which releases the reference to the new view
//! // that the binding hands WebKit and WebKit never releases.
//! vsesvit_webext::connect_create(&view, move |_, action| new_tab_view(action));
//!
//! // Lifecycle: load/unload installed extensions (content scripts apply to loads that
//! // start afterwards, as in Chrome). `load` returns after the synchronous part; DNR
//! // rules compile in the background (at load and whenever an extension changes them),
//! // `pending_filters()` counts the compiles and `on_filters_ready` runs once every pending
//! // one is attached, which the self-test uses before its `dnr_blocked` navigation. `Runtime::web_context()` is the context
//! // the scheme is registered on. `load` is for startup and the load after an install;
//! // when the user re-enables an extension, `load_with(.., LoadReason::Enable)` keeps
//! // `runtime.onStartup` from firing.
//! runtime.load(&installed_extension)?;                 // Err(LoadError) for unreadable files/rulesets
//! runtime.load_with(&installed_extension, LoadReason::Enable)?;
//! runtime.on_filters_ready(|| { /* navigate */ });
//! runtime.unload(&id);
//! let ids: Vec<ExtensionId> = runtime.loaded();
//!
//! // The shell reports tab and window events; the runtime turns them into chrome.tabs and
//! // chrome.windows events and forgets closed tabs. `TabHost` answers with the windows
//! // (most recently focused first) and their tabs, and opens, moves and closes them.
//! runtime.tab_attached(tab_id);     // a window took it: new (onCreated) or from another window
//! runtime.tab_moved(tab_id);        // reordered within its window
//! runtime.tab_updated(tab_id);      // after a committed navigation or a title change
//! runtime.tab_activated(tab_id);    // on tab switch
//! runtime.tab_closed(tab_id, window_closing);   // also closes the ports of its documents
//! runtime.windows_changed();        // a window opened, closed, took or lost focus, or resized
//!
//! // Every load of a tab's top frame, from WebKit's load events (an error page the shell
//! // shows instead of a failed load is none), and the tab a page opened, before the shell
//! // puts it in a window: `chrome.webNavigation`'s events come from these, and from a script
//! // the runtime puts in every frame.
//! runtime.tab_load(tab_id, Load::Started(&uri));    // also Redirected, Committed(uri, transition), Finished
//! runtime.tab_load(tab_id, Load::Failed(&uri, NetError::of(&error)));
//! runtime.tab_opened_by(new_tab_id, tab_id);
//!
//! // `chrome.cookies` works on `session`'s cookie manager, and on `TabHost::private_session`'s
//! // for an extension allowed in private windows; `TabHost::cookies_blocked` says which sites
//! // the user blocked, for which extensions may set no cookie.
//!
//! // Toolbar actions. `activate_action` hands the popup WebView to put in a popover to its
//! // callback, possibly later (the shell owns it; drop it to close, which closes its
//! // ports), or fires action.onClicked when the action has no popup.
//! let actions: Vec<ActionInfo> = runtime.actions();
//! runtime.connect_actions_changed(move || rebuild_toolbar());
//! runtime.activate_action(&id, Some(tab_id), move |popup| popover.set_child(Some(&popup)));
//!
//! // Extensions' context menu items (`menus`): the shell asks what to show when a menu opens,
//! // with what was clicked, and reports the item the user chose.
//! let target = menus::Target { page_url, frame_url, link_url, selection, ..Default::default() };
//! let items: Vec<(ExtensionId, menus::Entry)> = runtime.page_menu(&target);
//! let entries: Vec<menus::Entry> = runtime.action_menu(&id);
//! runtime.menu_clicked(&id, &item, Some(tab_id), Some(&target));   // None for the action's menu
//!
//! // Extensions' keyboard shortcuts (`commands`): the shell binds the chords core resolves
//! // (`Profile::extension_shortcuts`) and reports a named command's; an action command
//! // (`_execute_action`) is the shell's `activate_action`.
//! runtime.command(&id, "toggle-feature", Some(tab_id));
//!
//! // Extensions' notifications (`notifications`) go out as GNotifications of the default
//! // GApplication, whose clicks invoke `app.extension-notification` (`notifications::ACTION`,
//! // parameter `(sss)`: extension id, notification id, activation name), which the shell
//! // registers and reports; the Settings button is the shell's (its extension settings).
//! // The per-extension switch lives in core; the shell reports each change.
//! let (ext, notification, name) = parameter.get::<(String, String, String)>()?;
//! runtime.notification_activated(&ExtensionId::parse(&ext)?, &notification, Activation::parse(&name)?);
//! profile.extensions().set_notifications_allowed(&id, false)?;
//! runtime.notification_permission_changed(&id);
//! let shown: Option<notifications::Shown> = runtime.notification(&id, &notification);
//!
//! // Remote storage.sync changes (from a future sync engine's ApplyReport):
//! runtime.storage_sync_changed(&ext_id, &changes);
//! ```
//!
//! What extensions get:
//!
//! - Files served from `chrome-extension://<id>/<path>` (secure, CORS-enabled scheme).
//!   Documents outside the extension origin may load only `web_accessible_resources`,
//!   and navigate only to those (with the shell's navigation policy, see above and
//!   [`gate`], which also says what it cannot tell).
//!   Extension pages keep the manifest's CSP wherever they are shown; an MV3 policy that
//!   Chrome would refuse (remote or `eval`'d script) gets Chrome's default instead.
//! - A background context: `background.scripts`, `background.page`, and MV3
//!   `background.service_worker` (run as a generated page; `type: module` honoured; a
//!   classic worker's `importScripts` of string literals is loaded by the page ahead of it).
//! - Content scripts with `matches`, `exclude_matches`, `run_at`, `all_frames` and `css`,
//!   each extension in its own isolated world (named by its id); `"world": "MAIN"` ones in
//!   the page's world, without the extension API, as in Chrome. The same for those an
//!   extension registers (see [`dynamic_scripts`]), which run only where it has host
//!   permissions, from the next load on, as in Chrome; the ones that persist across sessions
//!   last until the extension is installed afresh or updated.
//! - `chrome.*` and `browser.*` (promise and callback styles, `chrome.runtime.lastError`)
//!   in content scripts: `runtime.sendMessage/onMessage/connect/onConnect/getURL/id/getManifest`,
//!   `storage.local/sync` with `storage.onChanged`, `i18n`. Extension pages (background,
//!   popup, and any of the extension's documents shown in a tab: the options page,
//!   `tabs.create(getURL(..))`, links) additionally get
//!   `storage.session`, `tabs.query/get/getCurrent/create/update/move/remove/reload/sendMessage`
//!   with `onCreated/onUpdated/onActivated/onMoved/onDetached/onAttached/onRemoved`,
//!   `windows.get/getCurrent/getLastFocused/getAll/create/update/remove` with
//!   `onCreated/onRemoved/onFocusChanged/onBoundsChanged` over the shell's windows (see
//!   [`windows`]; a page in a tab is in that tab's window, any other page in the last focused
//!   one), `webNavigation.getFrame/getAllFrames` and its events with URL filters for every
//!   frame of every tab (see [`web_navigation`]), `cookies.get/getAll/set/remove/getAllCookieStores`
//!   with `onChanged` over the session's cookie store (see [`cookies`]; store `"0"`, and no
//!   cookie set for a site the user blocked), `scripting.executeScript/insertCSS/removeCSS` and
//!   `registerContentScripts/getRegisteredContentScripts/updateContentScripts/unregisterContentScripts`,
//!   `action`/`browserAction` (`setBadgeText`, `setTitle`, `setIcon`, `setPopup`,
//!   `onClicked`), `alarms` (at most 500, every 30 seconds at the soonest, as in Chrome),
//!   `permissions.getAll/contains/request/remove` with `onAdded/onRemoved` (see below),
//!   `contextMenus` (also as Firefox's `menus`, see [`menus`]),
//!   `commands.getAll/onCommand` when the manifest declares `commands`, `notifications` (see
//!   [`notifications`]: `create/update/clear/getAll/getPermissionLevel` with
//!   `onClicked/onButtonClicked/onClosed/onPermissionLevelChanged`, shown as Chrome shows
//!   them through the Linux portal, and refused while the user has them turned off),
//!   `extension.getURL`,
//!   `runtime.openOptionsPage`, `runtime.reload` (the whole extension starts over, its
//!   pages in tabs reload), and `runtime.onInstalled` on the first load of an install
//!   or version (`runtime.onStartup` on later startups), `tabs.connect`, and
//!   `runtime.getBackgroundPage` in the background page and in the popups it opens (a page
//!   background opens each popup with `window.open`, WebKit's only way for one view to
//!   reach another's window).
//! - Ports as in Chrome ([`messaging`]): `runtime.connect` reaches the extension's pages,
//!   `tabs.connect` the content scripts in a tab, a port posts to every context that took
//!   the connection, and it closes when its document goes away, its tab or popup closes,
//!   or the extension unloads. Another extension's pages can `runtime.sendMessage` and
//!   `runtime.connect` to it (`onMessageExternal`, `onConnectExternal`) unless its
//!   `externally_connectable.ids` leave that extension out.
//! - Chrome's permission model for those APIs: `scripting.*` needs the `scripting`
//!   permission and host access to the target tab (a host permission, or `activeTab`
//!   after the user invoked the action or one of its shortcuts on that tab); tab URLs and
//!   titles are visible only with the `tabs` permission or host access to the tab's URL. No
//!   host permission, `<all_urls>` included, reaches `file:` pages: Chrome needs the user's
//!   "Allow access to file URLs" grant for that, which Vsesvit does not offer.
//!   `tabs.create/update` resolve relative URLs against the calling page and refuse
//!   `javascript:` and `file:`.
//! - Optional permissions as in Chrome: `permissions.request` during a user gesture, for
//!   permissions the manifest lists, answers at once for what the extension holds or what has
//!   no warning, and otherwise asks the user through [`TabHost::ask_permissions`] with
//!   Chrome's warnings ([`permissions::Prompt`]). Core keeps the grants
//!   (`Extensions::active_permissions`), which `permissions.remove` takes back, refusing
//!   required ones. Granted hosts widen at once where the extension's pages fetch from across
//!   origins, its registered content scripts and declarativeNetRequest rules reach and the
//!   tabs it sees; a grant's namespace is there from the start in every page.
//! - Private tabs only where the user allowed the extension in private windows (Chrome's
//!   "Allow in Incognito"; [`Runtime::allowed_in_private_changed`] applies a change): elsewhere
//!   a private tab gets none of its content scripts or rulesets, `chrome.tabs` neither lists it
//!   nor reports its events, and `chrome.windows` does the same with private windows, focus
//!   going to one reading as `WINDOW_ID_NONE`. `incognito` says which tabs and windows are
//!   private. As in Chrome, `windows.create({incognito: true})` opens a private window for any
//!   extension (not on its own pages where it does not run, and without telling it the window),
//!   and no tab moves between a normal and a private window.
//! - declarativeNetRequest: the enabled static rulesets with the dynamic and session rules
//!   (`updateDynamicRules`, `updateSessionRules`, `updateEnabledRulesets`, their getters,
//!   `isRegexSupported`, `getAvailableStaticRuleCount`; see [`dnr_rules`]) as one WebKit
//!   content blocker per extension, attached to every tab it runs in beside the shell's own blockers
//!   and rebuilt on a worker thread when they change; an update resolves once the tabs have
//!   it. Dynamic rules and the chosen rulesets outlive a restart. Rules WebKit cannot
//!   express are logged and skipped. As in Chrome, the API and the rulesets need the
//!   `declarativeNetRequest` permission, and redirect and modifyHeaders rules (every rule,
//!   with `declarativeNetRequestWithHostAccess`) act only on requests to hosts the
//!   extension has host permissions for.
//!
//! Known limits: events reach a tab's top frame only (`tabs.sendMessage`, `tabs.connect`,
//! `storage.onChanged` in subframes; a subframe's own ports work), and webNavigation's frame
//! ids name frames to nothing else (no `frameId` targets a subframe); WebKit reports no
//! subframe loads, so a subframe's `onBeforeNavigate` comes as its document arrives, one
//! whose load fails reports nothing, and `onCreatedNavigationTarget` names the opener's top
//! frame; `cookies.onChanged` comes from reading the store again whenever WebKit says it
//! changed, so a change undone before the reading is never reported, a cookie gone is never
//! `evicted` and setting an expired one reports `explicit` rather than `expired_overwrite`;
//! web pages cannot
//! message an extension (`externally_connectable.matches`: WebKitGTK does not say which
//! document posted a message, so the sender could not be told apart from a frame
//! claiming its URL); `getBackgroundPage` cannot reach the background from an extension
//! page in a tab; a context menu click in a subframe has no `frameId`; WebKit's menus show
//! a radio item with a check mark; an image notification shows no image, and
//! `requireInteraction` and `silent` change nothing; every window is a normal one at the
//! screen's origin (a `popup` or `panel` opens as a normal window, and Wayland neither tells
//! nor sets a window's position), and no window can be unfocused or made to draw attention; GNotification does not tell when the
//! user dismisses a notification, so it stays in `notifications.getAll` until cleared or
//! replaced, as with Chrome on the portal; WebKit reports no content-blocker matches, so
//! `setExtensionActionOptions` shows no count and there is no `getMatchedRules`; no
//! `webRequest`; content-script CSS is a user-level style sheet (WebKitGTK ignores
//! author-level ones on standards-mode pages), so a page's own rules beat it unless it is
//! `!important`; a registered script's `matchOriginAsFallback` changes nothing; a host
//! permission taken back with `permissions.remove` stays fetchable from the extension's pages
//! until their web process ends (WebKit never forgets a CORS exception it was given); one runtime
//! per process;
//! `about:blank` frames inside extension pages get no API; in a background or popup view,
//! an `http(s)` iframe loads only for an extension without host permissions (WebKitGTK
//! applies the view's CORS allowlist to every frame); WebKitGTK does not say which frame
//! requests a file, so a request without a `Referer` is judged by its view's top document:
//! an extension frame inside a web page loads only web-accessible files (extension
//! documents send no `Referer`), and a frame whose referrer policy sends none passes for
//! its top document. Runtime state (compiled filters, dynamic rules, registered content
//! scripts, install markers) lives in `<profile>/webext/`.

pub mod cookies;
pub mod dnr;
pub mod dnr_rules;
pub mod dynamic_scripts;
pub mod gate;
pub mod i18n;
pub mod lifecycle;
pub mod menus;
pub mod messaging;
pub mod mime;
pub mod notifications;
pub mod patterns;
pub mod permissions;
pub mod protocol;
pub mod support;
pub mod tabs;
pub mod web_navigation;
pub mod windows;

pub use lifecycle::LoadReason;
pub use support::{Unsupported, unsupported_features};
pub use tabs::{NewTab, TabId, TabInfo};
pub use windows::{NewWindow, WindowId, WindowInfo, WindowState, WindowUpdate};

#[cfg(target_os = "linux")]
mod bridge;
#[cfg(target_os = "linux")]
mod content;
#[cfg(target_os = "linux")]
mod cookie_jar;
#[cfg(target_os = "linux")]
mod extension;
#[cfg(target_os = "linux")]
mod filters;
#[cfg(target_os = "linux")]
mod runtime;
#[cfg(target_os = "linux")]
mod scheme;
#[cfg(target_os = "linux")]
mod views;

#[cfg(target_os = "linux")]
pub use runtime::{ActionInfo, LoadError, Runtime};
#[cfg(target_os = "linux")]
pub use tabs::TabHost;
#[cfg(target_os = "linux")]
pub use views::connect_create;
pub use gate::{Gate, Policy};

/// The JavaScript shim every context receives: one function expression that
/// [`protocol::bootstrap`] applies to the context's configuration.
pub const API_JS: &str = include_str!("js/api.js");
