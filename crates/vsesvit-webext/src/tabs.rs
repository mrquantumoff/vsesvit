//! Tabs as the runtime sees them. The shell owns the real tabs and answers through
//! [`TabHost`]; these types are what crosses that boundary and what `chrome.tabs` shows
//! to extensions.

use serde::Serialize;
use serde_json::Value;
use vsesvit_core::private::Browsing;

use crate::windows::{WINDOW_TYPE, WindowId, WindowScope};
#[cfg(target_os = "linux")]
use crate::windows::{NewWindow, WindowInfo, WindowUpdate};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TabId(pub u32);

impl TabId {
    pub fn from_json(v: &Value) -> Option<TabId> {
        v.as_u64().and_then(|n| u32::try_from(n).ok()).map(TabId)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabInfo {
    pub id: TabId,
    pub window_id: WindowId,
    pub index: u32,
    pub url: String,
    pub title: String,
    pub active: bool,
    /// The kind of its window: `incognito` in `chrome.tabs`.
    pub browsing: Browsing,
}

impl TabInfo {
    /// `chrome.tabs.Tab`. Without `sees_content` (no `tabs` permission and no host
    /// access to the tab's URL) `url` and `title` are left out, as Chrome does.
    pub fn to_json_for(&self, sees_content: bool) -> Value {
        let mut tab = serde_json::json!({
            "id": self.id.0,
            "windowId": self.window_id,
            "index": self.index,
            "active": self.active,
            "highlighted": self.active,
            "selected": self.active,
            "pinned": false,
            "incognito": self.browsing == Browsing::Private,
            "status": "complete",
        });
        if sees_content {
            tab["url"] = Value::String(self.url.clone());
            tab["title"] = Value::String(self.title.clone());
        }
        tab
    }

    /// `chrome.tabs.query(queryInfo)`. Unknown keys are ignored; `url` accepts a match
    /// pattern or a list of them, matched with [`crate::patterns::url_matches`]. A tab
    /// whose contents the caller may not see never matches a `url` or `title` filter.
    /// `currentWindow`, `lastFocusedWindow` and `WINDOW_ID_CURRENT` are the caller's
    /// `scope`.
    pub fn matches_query(&self, query: &Value, sees_content: bool, scope: &WindowScope) -> bool {
        let Some(q) = query.as_object() else { return true };
        if !sees_content && (q.contains_key("url") || q.contains_key("title")) {
            return false;
        }
        let bool_key = |k: &str| q.get(k).and_then(Value::as_bool);
        if let Some(active) = bool_key("active")
            && active != self.active
        {
            return false;
        }
        if let Some(index) = q.get("index").and_then(Value::as_u64)
            && index != u64::from(self.index)
        {
            return false;
        }
        if let Some(window) = q.get("windowId").and_then(Value::as_i64)
            && scope.resolve(window) != Some(self.window_id)
        {
            return false;
        }
        let in_window = |key: &str, window: Option<WindowId>| bool_key(key).is_none_or(|wanted| wanted == (window == Some(self.window_id)));
        if !in_window("currentWindow", scope.current) || !in_window("lastFocusedWindow", scope.last_focused) {
            return false;
        }
        if q.get("windowType").and_then(Value::as_str).is_some_and(|t| t != WINDOW_TYPE) {
            return false;
        }
        if let Some(title) = q.get("title").and_then(Value::as_str)
            && !crate::patterns::glob(title, &self.title)
        {
            return false;
        }
        if let Some(url) = q.get("url") {
            let patterns: Vec<&str> = match url {
                Value::String(s) => vec![s.as_str()],
                Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            if !patterns.iter().any(|p| crate::patterns::url_matches(p, &self.url)) {
                return false;
            }
        }
        true
    }
}

/// `tabs.create`, once the runtime has resolved the URL and the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTab {
    pub url: String,
    pub active: bool,
    /// `None` when there is no window to put it in: the shell opens one.
    pub window: Option<WindowId>,
    /// `None` for the end of the window's tabs; past the end means the end too.
    pub index: Option<u32>,
}

/// What the shell provides. Every method is called on the UI thread, never while the
/// runtime holds internal borrows, so an implementation may call back into
/// [`crate::Runtime`] (for example `create_tab` calling `user_content_manager`, or any
/// change reporting itself through `tab_attached` or `windows_changed`).
#[cfg(target_os = "linux")]
pub trait TabHost {
    /// Most recently focused first.
    fn windows(&self) -> Vec<WindowInfo>;
    /// In window order, each window's tabs in order.
    fn tabs(&self) -> Vec<TabInfo>;
    fn create_tab(&self, tab: &NewTab) -> Option<TabId>;
    fn update_tab(&self, tab: TabId, url: Option<&str>, active: Option<bool>) -> bool;
    /// To `index` of `window` (the end for `None` or past it), within its window or out of it.
    fn move_tab(&self, tab: TabId, window: WindowId, index: Option<u32>) -> bool;
    fn remove_tab(&self, tab: TabId) -> bool;
    fn web_view(&self, tab: TabId) -> Option<webkit::WebView>;
    /// `window.urls` are absolute and `window.tab` exists.
    fn create_window(&self, window: &NewWindow) -> Option<WindowId>;
    fn update_window(&self, window: WindowId, update: &WindowUpdate) -> bool;
    fn remove_window(&self, window: WindowId) -> bool;
    /// Whether a cookie under `domain` (a leading dot for a domain cookie) belongs to a site the
    /// user set to Block, which extensions may not set cookies for.
    fn cookies_blocked(&self, domain: &str) -> bool;
    /// The private windows' network session, from their first tab until the last of them
    /// closes.
    fn private_session(&self) -> Option<webkit::NetworkSession>;
    /// Asks the user over `window` whether to grant what an extension's `permissions.request`
    /// adds, as Chrome's prompt does, and calls `answer` with the choice (at most once; never
    /// is a no). `window` is one the extension may know: a private one only where it runs.
    fn ask_permissions(&self, window: WindowId, prompt: crate::permissions::Prompt, answer: Box<dyn FnOnce(bool)>);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tab() -> TabInfo {
        TabInfo { id: TabId(7), window_id: WindowId(1), index: 2, url: "http://127.0.0.1:8080/index.html".into(), title: "Vsesvit fixture".into(), active: true, browsing: Browsing::Normal }
    }

    /// The caller's window is 1, the last focused 3.
    fn scope() -> WindowScope {
        WindowScope { current: Some(WindowId(1)), last_focused: Some(WindowId(3)) }
    }

    #[test]
    fn query_filters() {
        let (t, s) = (tab(), scope());
        assert!(t.matches_query(&json!({}), true, &s));
        assert!(t.matches_query(&Value::Null, true, &s));
        assert!(t.matches_query(&json!({"active": true, "currentWindow": true}), true, &s));
        assert!(!t.matches_query(&json!({"active": false}), true, &s));
        assert!(t.matches_query(&json!({"url": "http://127.0.0.1/*"}), true, &s));
        assert!(t.matches_query(&json!({"url": ["https://x/*", "*://*/index.html"]}), true, &s));
        assert!(!t.matches_query(&json!({"url": "https://*/*"}), true, &s));
        assert!(t.matches_query(&json!({"title": "Vsesvit*"}), true, &s));
        assert!(!t.matches_query(&json!({"title": "Other"}), true, &s));
        assert!(t.matches_query(&json!({"index": 2, "windowId": 1}), true, &s));
        assert!(!t.matches_query(&json!({"windowId": 2}), true, &s));
        assert!(t.matches_query(&json!({"windowId": -2}), true, &s));
        assert!(t.matches_query(&json!({"windowType": "normal"}), true, &s));
        assert!(!t.matches_query(&json!({"windowType": "popup"}), true, &s));
    }

    #[test]
    fn queries_follow_the_callers_windows() {
        let (t, s) = (tab(), scope());
        assert!(!t.matches_query(&json!({"currentWindow": false}), true, &s));
        assert!(!t.matches_query(&json!({"lastFocusedWindow": true}), true, &s));
        assert!(t.matches_query(&json!({"lastFocusedWindow": false}), true, &s));
        let elsewhere = WindowScope { current: Some(WindowId(2)), last_focused: Some(WindowId(1)) };
        assert!(!t.matches_query(&json!({"currentWindow": true}), true, &elsewhere));
        assert!(!t.matches_query(&json!({"windowId": -2}), true, &elsewhere));
        assert!(t.matches_query(&json!({"lastFocusedWindow": true}), true, &elsewhere));
        assert!(!t.matches_query(&json!({"currentWindow": true}), true, &WindowScope::default()));
        assert!(!t.matches_query(&json!({"windowId": -1}), true, &s));
    }

    #[test]
    fn tab_json_and_ids() {
        let v = tab().to_json_for(true);
        assert_eq!(v["id"], 7);
        assert_eq!(v["active"], true);
        assert_eq!(v["url"], "http://127.0.0.1:8080/index.html");
        assert_eq!(v["title"], "Vsesvit fixture");
        assert_eq!(v["incognito"], false);
        assert_eq!(TabInfo { browsing: Browsing::Private, ..tab() }.to_json_for(true)["incognito"], true);
        assert_eq!(TabId::from_json(&json!(7)), Some(TabId(7)));
        assert_eq!(TabId::from_json(&json!("7")), None);
        assert_eq!(TabId::from_json(&json!(-1)), None);
    }

    /// Without `tabs` or host access an extension gets the tab minus its contents, and
    /// cannot probe them through a query either.
    #[test]
    fn tabs_hide_their_contents_from_extensions_without_access() {
        let t = tab();
        let v = t.to_json_for(false);
        assert_eq!(v["id"], 7);
        assert_eq!(v["active"], true);
        assert!(v.get("url").is_none() && v.get("title").is_none(), "{v}");
        let s = scope();
        assert!(t.matches_query(&json!({"active": true}), false, &s));
        assert!(!t.matches_query(&json!({"url": "http://127.0.0.1/*"}), false, &s));
        assert!(!t.matches_query(&json!({"title": "Vsesvit*"}), false, &s));
    }
}
