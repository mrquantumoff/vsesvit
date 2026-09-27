//! Tabs as the runtime sees them. The shell owns the real tabs and answers through
//! [`TabHost`]; these types are what crosses that boundary and what `chrome.tabs` shows
//! to extensions.

use serde::Serialize;
use serde_json::Value;

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
    pub window_id: u32,
    pub index: u32,
    pub url: String,
    pub title: String,
    pub active: bool,
}

impl TabInfo {
    /// `chrome.tabs.Tab`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "id": self.id.0,
            "windowId": self.window_id,
            "index": self.index,
            "url": self.url,
            "title": self.title,
            "active": self.active,
            "highlighted": self.active,
            "selected": self.active,
            "pinned": false,
            "incognito": false,
            "status": "complete",
        })
    }

    /// `chrome.tabs.query(queryInfo)`. Unknown keys are ignored; `url` accepts a match
    /// pattern or a list of them, matched with [`crate::patterns::url_matches`].
    pub fn matches_query(&self, query: &Value) -> bool {
        let Some(q) = query.as_object() else { return true };
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
            && window >= 0
            && window != i64::from(self.window_id)
        {
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

/// What the shell provides. Every method is called on the UI thread, never while the
/// runtime holds internal borrows, so an implementation may call back into
/// [`crate::Runtime`] (for example `create_tab` calling `user_content_manager`).
#[cfg(target_os = "linux")]
pub trait TabHost {
    fn tabs(&self) -> Vec<TabInfo>;
    fn create_tab(&self, url: &str, active: bool) -> Option<TabId>;
    fn update_tab(&self, tab: TabId, url: Option<&str>, active: Option<bool>) -> bool;
    fn remove_tab(&self, tab: TabId) -> bool;
    fn web_view(&self, tab: TabId) -> Option<webkit::WebView>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tab() -> TabInfo {
        TabInfo { id: TabId(7), window_id: 1, index: 2, url: "http://127.0.0.1:8080/index.html".into(), title: "Vsesvit fixture".into(), active: true }
    }

    #[test]
    fn query_filters() {
        let t = tab();
        assert!(t.matches_query(&json!({})));
        assert!(t.matches_query(&Value::Null));
        assert!(t.matches_query(&json!({"active": true, "currentWindow": true})));
        assert!(!t.matches_query(&json!({"active": false})));
        assert!(t.matches_query(&json!({"url": "http://127.0.0.1/*"})));
        assert!(t.matches_query(&json!({"url": ["https://x/*", "*://*/index.html"]})));
        assert!(!t.matches_query(&json!({"url": "https://*/*"})));
        assert!(t.matches_query(&json!({"title": "Vsesvit*"})));
        assert!(!t.matches_query(&json!({"title": "Other"})));
        assert!(t.matches_query(&json!({"index": 2, "windowId": 1})));
        assert!(!t.matches_query(&json!({"windowId": 2})));
        assert!(t.matches_query(&json!({"windowId": -2})));
    }

    #[test]
    fn tab_json_and_ids() {
        let v = tab().to_json();
        assert_eq!(v["id"], 7);
        assert_eq!(v["active"], true);
        assert_eq!(TabId::from_json(&json!(7)), Some(TabId(7)));
        assert_eq!(TabId::from_json(&json!("7")), None);
        assert_eq!(TabId::from_json(&json!(-1)), None);
    }
}
