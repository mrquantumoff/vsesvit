//! Everything the runtime keeps per loaded extension.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vsesvit_core::extensions::manifest::{Background, Manifest, ManifestVersion, MatchPattern, RelPath};
use vsesvit_core::extensions::permissions::{self, PermissionSet};
use vsesvit_core::extensions::{ExtensionId, InstalledExtension};
use vsesvit_core::html::escape as html_escape;
use vsesvit_core::private::Browsing;
use webkit::glib;

use crate::content::UserContent;
use crate::dnr::Grants;
use crate::dnr_rules::{Rules, Saved};
use crate::dynamic_scripts::Scripts;
use crate::filters::Compiles;
use crate::i18n;
use crate::menus::Menus;
use crate::notifications::Notifications;
use crate::patterns;
use crate::protocol;
use crate::runtime::LoadError;
use crate::tabs::{TabId, TabInfo};

pub(crate) const SCHEME: &str = "chrome-extension";
/// Chrome's name for the page that hosts `background.scripts` / the service worker.
pub(crate) const GENERATED_BACKGROUND: &str = "_generated_background_page.html";

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ViewId(pub u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewKind {
    Background,
    Popup,
}

/// Work held until the background has loaded.
pub(crate) type Waiting = Box<dyn FnOnce()>;

pub(crate) struct ExtView {
    pub id: ViewId,
    pub kind: ViewKind,
    pub view: glib::WeakRef<webkit::WebView>,
}

/// Mutable toolbar-action state, seeded from the manifest.
#[derive(Clone, Debug)]
pub(crate) struct ActionState {
    pub title: String,
    pub icon: Option<PathBuf>,
    /// Relative path inside the extension; empty = no popup (fire `onClicked`).
    pub popup: String,
    pub badge_text: String,
}

/// A popup the background page is opening with `window.open` (see `views::open_popup`).
pub(crate) struct OpeningPopup {
    pub url: String,
    pub show: Box<dyn FnOnce(webkit::WebView)>,
}

pub(crate) struct Alarm {
    pub scheduled_time_ms: f64,
    pub period_minutes: Option<f64>,
    pub source: Option<glib::SourceId>,
}

pub(crate) struct Extension {
    pub id: ExtensionId,
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub version: String,
    /// Host part of this extension's URLs. The id itself when it is URL-safe.
    pub host: String,
    /// `chrome-extension://<host>/`
    pub base_url: String,
    /// Isolated-world name for content scripts.
    pub world: String,
    /// `window.webkit.messageHandlers.<handler>` for content scripts, registered in
    /// `world`; unique per extension so the signal detail identifies it.
    pub handler: String,
    /// The handler extension pages use, registered in the default world. A different
    /// name, because both handlers live on a tab's manager and the signal detail is
    /// all that tells them apart.
    pub page_handler: String,
    /// Secret the page bootstrap carries and every page call must repeat (see
    /// `protocol`): the default-world handler is reachable by any document in the same
    /// view, the bootstrap only by this extension's documents.
    pub page_token: String,
    /// The manifest's content scripts.
    pub content: UserContent,
    /// The content scripts `scripting.registerContentScripts` added, and their user content.
    pub dynamic_scripts: RefCell<Scripts>,
    pub dynamic_content: RefCell<UserContent>,
    /// The page shim, injected into this extension's documents (and no others) in the
    /// default world of every view.
    pub page_script: webkit::UserScript,
    /// The content-script shim, for isolated-world code the manifest did not declare
    /// (`scripting.executeScript`, dynamic content scripts).
    pub content_bootstrap: String,
    pub csp: String,
    /// What it holds now: its required permissions and the optional ones the user granted
    /// (`Extensions::active_permissions` in core).
    pub permissions: RefCell<PermissionSet>,
    /// What the extension's declarativeNetRequest rules may do; `None` without the
    /// declarativeNetRequest permission.
    pub grants: RefCell<Option<Grants>>,
    pub dnr: RefCell<Rules>,
    pub compiles: Compiles,
    pub background: RefCell<Option<webkit::WebView>>,
    /// Work waiting for the background to finish loading; `None` once it has, or when there
    /// is none.
    pub background_waiting: RefCell<Option<Vec<Waiting>>>,
    pub views: RefCell<Vec<ExtView>>,
    pub opening_popup: RefCell<Option<OpeningPopup>>,
    /// The popup the background page opened last.
    pub adopted_popup: glib::WeakRef<webkit::WebView>,
    pub action: RefCell<Option<ActionState>>,
    pub filter: RefCell<Option<webkit::UserContentFilter>>,
    pub alarms: RefCell<BTreeMap<String, Alarm>>,
    /// Tabs the user invoked the action on, while the `activeTab` permission applies.
    pub active_tabs: RefCell<BTreeSet<TabId>>,
    pub menus: RefCell<Menus>,
    pub notifications: RefCell<Notifications>,
    /// `storage.session`, which lasts as long as this load of the extension.
    pub session_storage: RefCell<BTreeMap<String, Value>>,
    /// The user allowed it in private windows (`extensions::private` in core).
    pub in_private: Cell<bool>,
}

impl Extension {
    pub fn build(installed: &InstalledExtension, ui_locale: &str, active: PermissionSet) -> Result<Extension, LoadError> {
        let manifest = &installed.manifest;
        let host = url_host(&installed.id);
        let base_url = format!("{SCHEME}://{host}/");
        let world = installed.id.as_str().to_owned();
        let handler = format!("vsesvit_{host}");
        let page_handler = format!("vsesvit_{host}_page");
        let page_token = random_token();
        let catalog = i18n::load_catalog(&installed.dir, ui_locale, manifest.default_locale.as_deref());
        // Every namespace it may hold, so one the user grants later works in pages already open.
        let available = permissions::required(manifest).union(&permissions::optional(manifest)).apis;

        let config = |kind: &str, handler: &str, token: Option<&str>| {
            json!({
                "id": installed.id.as_str(),
                "host": host,
                "handler": handler,
                "token": token,
                "kind": kind,
                "manifest": manifest.raw,
                "i18n": { "locale": ui_locale, "messages": catalog },
                "permissions": available,
                "optionsPage": manifest.options_page.as_ref().map(|p| p.as_str()),
                "dnr": crate::dnr_rules::constants(),
            })
        };
        let content_bootstrap = protocol::bootstrap(&config("content", &handler, None));
        let page_bootstrap = protocol::bootstrap(&config("page", &page_handler, Some(&page_token)));

        let content = UserContent::build(&installed.dir, &manifest.content_scripts, &world, &content_bootstrap)?;
        let own_documents = format!("{base_url}*");
        let page_script = webkit::UserScript::new(
            &page_bootstrap,
            webkit::UserContentInjectedFrames::AllFrames,
            webkit::UserScriptInjectionTime::Start,
            &[own_documents.as_str()],
            &[],
        );

        let grants = dnr_grants(&active);
        if grants.is_none() && manifest.dnr_rulesets.iter().any(|r| r.enabled) {
            log::warn!("{}: declarativeNetRequest rulesets ignored without the declarativeNetRequest permission", manifest.name);
        }
        let action = manifest.action.as_ref().map(|a| ActionState {
            title: a.default_title.clone().unwrap_or_else(|| manifest.name.clone()),
            icon: largest_icon(&installed.dir, &a.default_icon).or_else(|| largest_icon(&installed.dir, &manifest.icons)),
            popup: a.default_popup.as_ref().map(|p| p.as_str().to_owned()).unwrap_or_default(),
            badge_text: String::new(),
        });

        Ok(Extension {
            id: installed.id.clone(),
            dir: installed.dir.clone(),
            manifest: manifest.clone(),
            version: installed.version.clone(),
            host,
            base_url,
            world,
            handler,
            page_handler,
            page_token,
            content,
            dynamic_scripts: RefCell::new(Scripts::default()),
            dynamic_content: RefCell::new(UserContent::default()),
            page_script,
            content_bootstrap,
            csp: content_security_policy(manifest),
            permissions: RefCell::new(active),
            grants: RefCell::new(grants),
            dnr: RefCell::new(Rules::new(&manifest.dnr_rulesets, Saved::default()).0),
            compiles: Compiles::default(),
            background: RefCell::new(None),
            background_waiting: RefCell::new(None),
            views: RefCell::new(Vec::new()),
            opening_popup: RefCell::new(None),
            adopted_popup: glib::WeakRef::new(),
            action: RefCell::new(action),
            filter: RefCell::new(None),
            alarms: RefCell::new(BTreeMap::new()),
            active_tabs: RefCell::new(BTreeSet::new()),
            menus: RefCell::new(Menus::default()),
            notifications: RefCell::new(Notifications::default()),
            session_storage: RefCell::new(BTreeMap::new()),
            in_private: Cell::new(false),
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path.trim_start_matches('/'))
    }

    /// Is `url` a document of this extension?
    pub fn owns_url(&self, url: &str) -> bool {
        patterns::under_base(&self.base_url, url)
    }

    /// A runtime API's file reference (`files`, `path`, `popup`) as a path inside the
    /// extension, accepting Chrome's spellings (leading `/`, the extension's own URL).
    pub fn resource(&self, reference: &str) -> Result<RelPath, String> {
        RelPath::parse(patterns::resource_path(&self.base_url, reference)).map_err(|e| e.to_string())
    }

    /// A file a `scripting` call names, if the extension has it.
    pub fn script_file(&self, reference: &str) -> Option<RelPath> {
        self.resource(reference).ok().filter(|path| path.resolve(&self.dir).is_file())
    }

    /// The user content of the dynamic content scripts. A file gone since they were
    /// registered leaves them all out, which is logged.
    pub fn build_dynamic_content(&self) -> UserContent {
        let entries = self.dynamic_scripts.borrow().content_scripts(&self.host_permissions());
        UserContent::build(&self.dir, &entries, &self.world, &self.content_bootstrap).unwrap_or_else(|e| {
            log::warn!("{}: dynamic content scripts left out: {e}", self.id.as_str());
            UserContent::default()
        })
    }

    pub fn has_permission(&self, name: &str) -> bool {
        self.permissions.borrow().apis.contains(name)
    }

    /// The host permissions it holds now, granted ones included.
    pub fn host_permissions(&self) -> Vec<MatchPattern> {
        self.permissions.borrow().origins.iter().cloned().collect()
    }

    /// The origins its own views may fetch from across origins.
    pub fn cors_allowlist(&self) -> Vec<String> {
        self.permissions.borrow().origins.iter().map(|p| p.as_str().to_owned()).collect()
    }

    /// May this extension act on a document at `url` (inject scripts, read the tab's
    /// URL and title)? Its own pages, its host permissions, and an `activeTab` grant on
    /// `tab` say yes. Never for a local file: Chrome needs the user's file-access grant
    /// for that, even with `activeTab`, and this runtime does not offer one.
    pub fn host_access(&self, url: &str, tab: Option<TabId>) -> bool {
        if self.owns_url(url) {
            return true;
        }
        let parsed = url::Url::parse(url);
        if parsed.as_ref().is_ok_and(|u| u.scheme() == "file") {
            return false;
        }
        tab.is_some_and(|t| self.active_tabs.borrow().contains(&t)) || parsed.is_ok_and(|u| self.permissions.borrow().origins.iter().any(|p| p.matches(&u)))
    }

    /// Whether it runs in tabs of `browsing`'s kind: in private ones only where the user allowed
    /// it, as Chrome's "Allow in Incognito". A private tab it does not run in does not exist for
    /// it: no content, no `chrome.tabs` entry, no tab events.
    pub fn runs_in(&self, browsing: Browsing) -> bool {
        browsing == Browsing::Normal || self.in_private.get()
    }

    /// May this extension see `tab`'s URL and title? The `tabs` permission or host
    /// access to the URL, as in Chrome.
    pub fn sees_tab(&self, tab: &TabInfo) -> bool {
        self.has_permission("tabs") || self.host_access(&tab.url, Some(tab.id))
    }

    /// `chrome.tabs.Tab` as this extension may see it.
    pub fn tab_json(&self, tab: &TabInfo) -> Value {
        tab.to_json_for(self.sees_tab(tab))
    }

    /// The user invoked the action on `tab`: with `activeTab`, that grants host access
    /// to the tab until it leaves its origin or closes.
    pub fn grant_active_tab(&self, tab: TabId) {
        if self.has_permission("activeTab") {
            self.active_tabs.borrow_mut().insert(tab);
        }
    }

    pub fn revoke_active_tab(&self, tab: TabId) {
        self.active_tabs.borrow_mut().remove(&tab);
    }

    pub fn background_url(&self) -> Option<String> {
        match &self.manifest.background {
            Some(Background::Page { page, .. }) => Some(self.url(page.as_str())),
            Some(Background::Scripts { .. }) | Some(Background::ServiceWorker { .. }) => Some(self.url(GENERATED_BACKGROUND)),
            None => None,
        }
    }

    /// Whether the background is a page (`background.page` or `background.scripts`), which
    /// `runtime.getBackgroundPage` returns, rather than a service worker.
    pub fn background_is_page(&self) -> bool {
        matches!(self.manifest.background, Some(Background::Page { .. } | Background::Scripts { .. }))
    }

    /// Whether the background is a service worker or an event page, whose context menu
    /// items Chrome keeps across restarts and which must name every item it creates.
    pub fn lazy_background(&self) -> bool {
        matches!(
            self.manifest.background,
            Some(Background::ServiceWorker { .. } | Background::Scripts { persistent: false, .. } | Background::Page { persistent: false, .. })
        )
    }

    /// The page that hosts `background.scripts` or the MV3 service worker.
    pub fn generated_background_page(&self) -> Option<String> {
        background_page(&self.manifest, &self.base_url, |path| RelPath::parse(path).ok().and_then(|rel| std::fs::read_to_string(rel.resolve(&self.dir)).ok()))
    }

    pub fn web_extension_mode(&self) -> webkit::WebExtensionMode {
        match self.manifest.manifest_version {
            ManifestVersion::V2 => webkit::WebExtensionMode::Manifestv2,
            ManifestVersion::V3 => webkit::WebExtensionMode::Manifestv3,
        }
    }

    /// Runs `f` once the background has loaded, since only then do its top-level listeners
    /// exist: now, unless it is still loading. Chrome holds messages for a starting background
    /// the same way.
    pub fn when_background_loaded(&self, f: impl FnOnce() + 'static) {
        if let Some(waiting) = self.background_waiting.borrow_mut().as_mut() {
            waiting.push(Box::new(f));
            return;
        }
        f();
    }

    /// Runs the work [`Extension::when_background_loaded`] held back.
    pub fn background_loaded(&self) {
        let waiting = self.background_waiting.borrow_mut().take();
        for f in waiting.into_iter().flatten() {
            f();
        }
    }

    /// Extension pages that are still alive, background first. Dead weak refs are pruned.
    pub fn live_views(&self) -> Vec<(ViewId, ViewKind, webkit::WebView)> {
        let mut views = self.views.borrow_mut();
        views.retain(|v| v.view.upgrade().is_some());
        let mut live: Vec<(ViewId, ViewKind, webkit::WebView)> =
            views.iter().filter_map(|v| v.view.upgrade().map(|w| (v.id, v.kind, w))).collect();
        live.sort_by_key(|(_, kind, _)| *kind != ViewKind::Background);
        live
    }

    /// The view a page call that arrived on `view`'s handler came from. A popup the
    /// background page opened shares that page's handler, so a call from a document other
    /// than the background's is the popup's.
    pub fn caller_view(&self, view: ViewId, url: Option<&str>) -> ViewId {
        let Some(popup) = self.adopted_popup.upgrade() else { return view };
        let views = self.views.borrow();
        let on_background = views.iter().any(|v| v.id == view && v.kind == ViewKind::Background);
        let document = url.map(|u| u.split(['?', '#']).next().unwrap_or(u).to_owned());
        if !on_background || document.is_none() || document == self.background_url() {
            return view;
        }
        views.iter().find(|v| v.view.upgrade().is_some_and(|w| w == popup)).map_or(view, |v| v.id)
    }

    pub fn owns_view(&self, view: &webkit::WebView) -> bool {
        self.views.borrow().iter().any(|v| v.view.upgrade().is_some_and(|w| &w == view))
    }

    pub fn web_accessible(&self, path: &str, page_url: &str) -> bool {
        patterns::web_accessible(&self.manifest.web_accessible_resources, path, page_url)
    }

    pub fn web_reachable(&self, path: &str) -> bool {
        patterns::web_reachable(&self.manifest.web_accessible_resources, path)
    }

    pub fn clear_alarms(&self) {
        for (_, alarm) in std::mem::take(&mut *self.alarms.borrow_mut()) {
            if let Some(source) = alarm.source {
                source.remove();
            }
        }
    }

    /// `chrome.alarms.Alarm` JSON.
    pub fn alarm_json(name: &str, alarm: &Alarm) -> Value {
        let mut v = json!({ "name": name, "scheduledTime": alarm.scheduled_time_ms });
        if let Some(p) = alarm.period_minutes {
            v["periodInMinutes"] = json!(p);
        }
        v
    }
}

/// [`Extension::generated_background_page`] for `manifest`, served from `base_url`, with
/// `read` giving the text of a file inside the extension. A classic service worker runs as
/// a page script here, where `importScripts` can load nothing (it is synchronous, and the
/// CSP rules out `eval`), so the page loads the scripts the worker imports by string
/// literal ahead of it, and the shim's `importScripts` only checks that it did. Code a
/// worker runs before its `importScripts` call therefore runs after the imports.
fn background_page(manifest: &Manifest, base_url: &str, read: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut html = format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>", html_escape(&manifest.name));
    let mut script = |kind: &str, src: &str| html.push_str(&format!("<script{kind} src=\"{}\"></script>", html_escape(src)));
    match &manifest.background {
        Some(Background::Scripts { scripts, .. }) => {
            for s in scripts {
                script("", &format!("{base_url}{}", s.as_str()));
            }
        }
        Some(Background::ServiceWorker { script: worker, module }) => {
            let worker = format!("{base_url}{}", worker.as_str());
            if *module {
                script(" type=\"module\"", &worker);
            } else {
                let mut imports = Vec::new();
                if let Ok(worker_url) = url::Url::parse(&worker) {
                    imported_scripts(base_url, &worker_url, &worker, &read, &mut vec![worker.clone()], &mut imports);
                }
                for src in imports.iter().chain([&worker]) {
                    script("", src);
                }
            }
        }
        _ => return None,
    }
    html.push_str("</head><body></body></html>");
    Some(html)
}

/// Appends to `out` the extension scripts that `script_url` imports by string literal,
/// each after its own imports, so in the order they run. A worker resolves every import
/// against its own URL (`worker`), whichever script makes it. `seen` stops cycles.
fn imported_scripts(base_url: &str, worker: &url::Url, script_url: &str, read: &impl Fn(&str) -> Option<String>, seen: &mut Vec<String>, out: &mut Vec<String>) {
    let Some(source) = crate::scheme::split_uri(script_url).and_then(|(_, path)| read(&path)) else { return };
    for argument in import_scripts_arguments(&source) {
        let Ok(url) = worker.join(&argument).map(String::from) else { continue };
        if !url.starts_with(base_url) || seen.contains(&url) {
            continue;
        }
        seen.push(url.clone());
        imported_scripts(base_url, worker, &url, read, seen, out);
        out.push(url);
    }
}

/// The arguments of every `importScripts(..)` call in `source` whose arguments are all
/// string literals, in order. A call after `//` on its line is taken as commented out.
fn import_scripts_arguments(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, name) in source.match_indices("importScripts") {
        let line = &source[source[..at].rfind('\n').map_or(0, |n| n + 1)..at];
        if line.contains("//") || line.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$') {
            continue;
        }
        let Some(mut rest) = source[at + name.len()..].trim_start().strip_prefix('(') else { continue };
        let mut arguments = Vec::new();
        let complete = loop {
            rest = rest.trim_start();
            if rest.starts_with(')') {
                break true;
            }
            let Some(quote) = rest.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) else { break false };
            let Some((literal, after)) = string_literal(&rest[1..], quote) else { break false };
            arguments.push(literal);
            rest = after.trim_start();
            match rest.strip_prefix(',') {
                Some(next) => rest = next,
                None if rest.starts_with(')') => {}
                None => break false,
            }
        };
        if complete {
            found.extend(arguments);
        }
    }
    found
}

/// The text of a JavaScript string literal that opened with `quote` just before `s`, and
/// what follows it. `None` for a template with substitutions or an unterminated literal.
fn string_literal(s: &str, quote: char) -> Option<(String, &str)> {
    let mut text = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            c if c == quote => return Some((text, &s[i + 1..])),
            '\\' => text.push(chars.next()?.1),
            '\n' if quote != '`' => return None,
            '$' if quote == '`' && s[i + 1..].starts_with('{') => return None,
            c => text.push(c),
        }
    }
    None
}

/// What declarativeNetRequest rules may do for an extension holding `permissions`.
pub(crate) fn dnr_grants(permissions: &PermissionSet) -> Option<Grants> {
    let apis: Vec<String> = permissions.apis.iter().cloned().collect();
    let hosts: Vec<String> = permissions.origins.iter().map(|p| p.as_str().to_owned()).collect();
    Grants::from_manifest(&apis, &hosts)
}

/// Chrome-style ids are URL hosts already. Gecko ids (`{uuid}`, `name@domain`) are not,
/// so they get a stable 32-hex-digit host derived from the id (Firefox likewise hides
/// the id behind a UUID host).
pub(crate) fn url_host(id: &ExtensionId) -> String {
    if id.is_chrome_style() {
        return id.as_str().to_owned();
    }
    let a = fnv1a(id.as_str().as_bytes(), 0xcbf2_9ce4_8422_2325);
    let b = fnv1a(id.as_str().as_bytes(), a ^ 0x9e37_79b9_7f4a_7c15);
    format!("{a:016x}{b:016x}")
}

fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// 128 bits from the OS random source, in hex.
fn random_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn largest_icon(dir: &Path, icons: &BTreeMap<u32, RelPath>) -> Option<PathBuf> {
    icons.iter().next_back().map(|(_, p)| p.resolve(dir))
}

/// `content_security_policy.extension_pages` (MV3) or the MV2 string, else Chrome's default.
/// Chrome refuses an MV3 policy that lets extension pages run code from elsewhere or from
/// strings; such a policy gets the default here.
fn content_security_policy(manifest: &Manifest) -> String {
    let raw = manifest.raw.get("content_security_policy");
    let from_manifest = match manifest.manifest_version {
        ManifestVersion::V3 => raw.and_then(|v| v.get("extension_pages")).and_then(Value::as_str).filter(|csp| {
            let secure = mv3_policy_is_secure(csp);
            if !secure {
                log::warn!("{}: insecure content_security_policy.extension_pages ignored: {csp}", manifest.name);
            }
            secure
        }),
        ManifestVersion::V2 => raw.and_then(Value::as_str),
    };
    from_manifest.map(str::to_owned).unwrap_or_else(|| "script-src 'self'; object-src 'self';".to_owned())
}

/// Chrome's rule for MV3 extension pages: `script-src` (or `default-src`) must exist, and
/// it, `object-src` and `worker-src` may name only `'self'`, `'none'`, `'wasm-unsafe-eval'`
/// and localhost.
fn mv3_policy_is_secure(csp: &str) -> bool {
    let directives: Vec<(String, Vec<String>)> = csp
        .split(';')
        .filter_map(|d| {
            let mut words = d.split_whitespace().map(str::to_ascii_lowercase);
            Some((words.next()?, words.collect()))
        })
        .collect();
    let sources = |name: &str| directives.iter().find(|(n, _)| n == name).or_else(|| directives.iter().find(|(n, _)| n == "default-src")).map(|(_, s)| s);
    let allowed = |source: &String| {
        matches!(source.as_str(), "'self'" | "'none'" | "'wasm-unsafe-eval'")
            || url::Url::parse(source.strip_suffix(":*").unwrap_or(source)).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && matches!(u.host_str(), Some("localhost" | "127.0.0.1")))
    };
    sources("script-src").is_some() && ["script-src", "object-src", "worker-src"].iter().all(|d| sources(d).is_none_or(|s| s.iter().all(allowed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Manifest {
        Manifest::parse(json, &|_| None).expect("test manifest parses")
    }

    #[test]
    fn a_page_token_is_32_hex_digits_and_fresh() {
        let token = random_token();
        assert!(token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()), "{token}");
        assert_ne!(token, random_token());
    }

    #[test]
    fn mv3_policies_that_run_code_from_elsewhere_fall_back_to_the_default() {
        let mv3 = |csp: &str| content_security_policy(&manifest(&json!({ "manifest_version": 3, "name": "t", "version": "1", "content_security_policy": { "extension_pages": csp } }).to_string()));
        let default = "script-src 'self'; object-src 'self';";
        for insecure in [
            "script-src 'self' 'unsafe-eval' https://cdn.example; object-src 'self'",
            "script-src 'self' 'unsafe-inline'",
            "default-src *",
            "script-src 'self'; worker-src https://cdn.example",
            "img-src 'self'",
        ] {
            assert_eq!(mv3(insecure), default, "{insecure}");
        }
        for secure in ["script-src 'self' 'wasm-unsafe-eval'; object-src 'self';", "default-src 'self'; img-src *", "SCRIPT-SRC 'SELF' http://localhost:8080 http://127.0.0.1:*"] {
            assert_eq!(mv3(secure), secure);
        }
        let mv2 = content_security_policy(&manifest(r#"{"manifest_version":2,"name":"t","version":"1","content_security_policy":"script-src 'self' 'unsafe-eval'; object-src 'self'"}"#));
        assert_eq!(mv2, "script-src 'self' 'unsafe-eval'; object-src 'self'");
        assert_eq!(content_security_policy(&manifest(r#"{"manifest_version":3,"name":"t","version":"1"}"#)), default);
    }

    #[test]
    fn a_classic_service_worker_page_loads_its_imports_first() {
        let base = "chrome-extension://abc/";
        let files = |path: &str| match path {
            "js/bg.js" => Some("importScripts('lib.js', \"../poly.js\");\n// importScripts('old.js');\nimportScripts(dynamic);\nchrome.runtime.onMessage.addListener(() => {});\n".to_owned()),
            "js/lib.js" => Some("self.importScripts(`deep/helper.js`, 'https://cdn.example/x.js');\n".to_owned()),
            "poly.js" => Some("importScripts('lib.js', 'extra.js');\n".to_owned()),
            _ => None,
        };
        let html = background_page(&manifest(r#"{"manifest_version":3,"name":"t","version":"1","background":{"service_worker":"js/bg.js"}}"#), base, files).unwrap();
        let sources: Vec<&str> = html.split("<script src=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
        // Every import resolves against the worker, as in a real one: poly.js's extra.js is js/extra.js.
        assert_eq!(sources, ["chrome-extension://abc/js/deep/helper.js", "chrome-extension://abc/js/lib.js", "chrome-extension://abc/js/extra.js", "chrome-extension://abc/poly.js", "chrome-extension://abc/js/bg.js"]);

        let module = background_page(&manifest(r#"{"manifest_version":3,"name":"t","version":"1","background":{"service_worker":"js/bg.js","type":"module"}}"#), base, files).unwrap();
        assert_eq!(module.matches("<script").count(), 1);
        assert!(module.contains("<script type=\"module\" src=\"chrome-extension://abc/js/bg.js\">"));
    }

    #[test]
    fn import_scripts_arguments_are_string_literals_only() {
        assert_eq!(import_scripts_arguments("importScripts ( 'a.js' , \"b\\\"c.js\" , `d.js`, );"), ["a.js", "b\"c.js", "d.js"]);
        assert!(import_scripts_arguments("importScripts('a.js', name); importScripts(`${x}.js`); myimportScripts('b.js'); importScripts('c.js' 'd.js')").is_empty());
        assert!(import_scripts_arguments("if (typeof importScripts === 'function') {}").is_empty());
    }
}
