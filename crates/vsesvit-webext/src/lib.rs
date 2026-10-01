//! WebExtensions runtime for WebKitGTK. WebKitGTK has no browser-extension runtime, so
//! this crate implements the subset Vsesvit supports on Linux, the way GNOME Web does:
//! content scripts as user scripts in a per-extension isolated world, a hidden background
//! web view, `chrome.*` bridged over `postMessage` replies, `chrome-extension://` served
//! from a custom URI scheme, and declarativeNetRequest translated to content blockers.
//!
//! Platform-neutral pieces compile and test everywhere: [`dnr`] (the translator),
//! [`protocol`] (the JS/Rust wire format), [`patterns`], [`mime`], [`i18n`], the tab
//! types in [`tabs`] and [`support`] (which of a manifest's requests this runtime lacks).
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
//! // Every tab WebView is built with the runtime's UserContentManager for that tab:
//! let view = webkit::WebView::builder()
//!     .network_session(&network_session)
//!     .user_content_manager(&runtime.user_content_manager(tab_id))
//!     .build();
//!
//! // Its navigation policy refuses a NavigationAction or NewWindowAction to a target that
//! // `may_navigate` refuses from the view's URL (the opener's for a new window's first
//! // load), so a web page cannot open an extension page that is not web-accessible.
//! if !runtime.may_navigate(&source, &target) { decision.ignore(); }
//!
//! // Lifecycle: load/unload installed extensions (content scripts apply to loads that
//! // start afterwards, as in Chrome). `load` returns after the synchronous part; DNR
//! // rulesets compile in the background, `pending_filters()` counts them and
//! // `on_filters_ready` runs once every pending one is attached, which the self-test
//! // uses before its `dnr_blocked` navigation. `Runtime::web_context()` is the context
//! // the scheme is registered on. `load` is for startup and the load after an install;
//! // when the user re-enables an extension, `load_with(.., LoadReason::Enable)` keeps
//! // `runtime.onStartup` from firing.
//! runtime.load(&installed_extension)?;                 // Err(LoadError) for unreadable files/rulesets
//! runtime.load_with(&installed_extension, LoadReason::Enable)?;
//! runtime.on_filters_ready(|| { /* navigate */ });
//! runtime.unload(&id);
//! let ids: Vec<ExtensionId> = runtime.loaded();
//!
//! // The shell reports tab events; the runtime turns them into chrome.tabs events and
//! // forgets closed tabs.
//! runtime.tab_updated(tab_id);      // after a committed navigation or a title change
//! runtime.tab_activated(tab_id);    // on tab switch
//! runtime.tab_closed(tab_id);
//!
//! // Toolbar actions. `activate_action` returns the popup WebView to put in a popover
//! // (the shell owns it; drop it to close), or None after firing action.onClicked.
//! let actions: Vec<ActionInfo> = runtime.actions();
//! runtime.connect_actions_changed(move || rebuild_toolbar());
//! if let Some(popup) = runtime.activate_action(&id, Some(tab_id)) { popover.set_child(Some(&popup)); }
//!
//! // Remote storage.sync changes (from a future sync engine's ApplyReport):
//! runtime.storage_sync_changed(&ext_id, &changes);
//! ```
//!
//! What extensions get:
//!
//! - Files served from `chrome-extension://<id>/<path>` (secure, CORS-enabled scheme).
//!   Documents outside the extension origin may load only `web_accessible_resources`,
//!   and navigate only to those (with the shell's navigation policy, see above).
//!   Extension pages keep the manifest's CSP wherever they are shown; an MV3 policy that
//!   Chrome would refuse (remote or `eval`'d script) gets Chrome's default instead.
//! - A background context: `background.scripts`, `background.page`, and MV3
//!   `background.service_worker` (run as a generated page; `type: module` honoured; a
//!   classic worker's `importScripts` of string literals is loaded by the page ahead of it).
//! - Content scripts with `matches`, `exclude_matches`, `run_at`, `all_frames` and `css`,
//!   each extension in its own isolated world (named by its id); `"world": "MAIN"` ones in
//!   the page's world, without the extension API, as in Chrome.
//! - `chrome.*` and `browser.*` (promise and callback styles, `chrome.runtime.lastError`)
//!   in content scripts: `runtime.sendMessage/onMessage/getURL/id/getManifest`,
//!   `storage.local/sync` with `storage.onChanged`, `i18n`. Extension pages (background,
//!   popup, and any of the extension's documents shown in a tab: the options page,
//!   `tabs.create(getURL(..))`, links) additionally get
//!   `tabs.query/get/getCurrent/create/update/remove/reload/sendMessage` with
//!   `onUpdated/onActivated/onRemoved`, `scripting.executeScript/insertCSS`,
//!   `action`/`browserAction` (`setBadgeText`, `setTitle`, `setIcon`, `setPopup`,
//!   `onClicked`), `alarms` (at most 500, every 30 seconds at the soonest, as in Chrome),
//!   `permissions.contains/getAll`, `extension.getURL`,
//!   `runtime.openOptionsPage`, and `runtime.onInstalled` on the first load of an install
//!   or version (`runtime.onStartup` on later startups).
//! - Chrome's permission model for those APIs: `scripting.*` needs the `scripting`
//!   permission and host access to the target tab (a host permission, or `activeTab`
//!   after the user invoked the action on that tab); tab URLs and titles are visible
//!   only with the `tabs` permission or host access to the tab's URL. No host permission,
//!   `<all_urls>` included, reaches `file:` pages: Chrome needs the user's "Allow access to
//!   file URLs" grant for that, which Vsesvit does not offer. `tabs.create/update` resolve
//!   relative URLs against the calling page and refuse `javascript:` and `file:`.
//! - declarativeNetRequest static rulesets as one WebKit content blocker per extension,
//!   attached to every tab. Rules WebKit cannot express are logged and skipped. As in
//!   Chrome, rulesets need the `declarativeNetRequest` permission, and redirect and
//!   modifyHeaders rules (every rule, with `declarativeNetRequestWithHostAccess`) act only
//!   on requests to hosts the extension has host permissions for.
//!
//! Known limits: events reach a tab's top frame only (`tabs.sendMessage`, `storage.onChanged`
//! in subframes); no `runtime.connect` ports; no `webRequest`; one runtime per process;
//! `about:blank` frames inside extension pages get no API; in a background or popup view,
//! an `http(s)` iframe loads only for an extension without host permissions (WebKitGTK
//! applies the view's CORS allowlist to every frame); WebKitGTK does not say which frame
//! requests a file, so a request without a `Referer` is judged by its view's top document:
//! an extension frame inside a web page loads only web-accessible files (extension
//! documents send no `Referer`), and a frame whose referrer policy sends none passes for
//! its top document. Runtime state (compiled filters, install markers) lives in
//! `<profile>/webext/`.

pub mod dnr;
pub mod i18n;
pub mod lifecycle;
pub mod mime;
pub mod patterns;
pub mod protocol;
pub mod support;
pub mod tabs;

pub use lifecycle::LoadReason;
pub use support::{Unsupported, unsupported_features};
pub use tabs::{TabId, TabInfo};

#[cfg(target_os = "linux")]
mod bridge;
#[cfg(target_os = "linux")]
mod content;
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

/// The JavaScript shim every context receives: one function expression that
/// [`protocol::bootstrap`] applies to the context's configuration.
pub const API_JS: &str = include_str!("js/api.js");
