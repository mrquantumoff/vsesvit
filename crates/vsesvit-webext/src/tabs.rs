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
            "incognito": false,
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
    pub fn matches_query(&self, query: &Value, sees_content: bool) -> bool {
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
        assert!(t.matches_query(&json!({}), true));
        assert!(t.matches_query(&Value::Null, true));
        assert!(t.matches_query(&json!({"active": true, "currentWindow": true}), true));
        assert!(!t.matches_query(&json!({"active": false}), true));
        assert!(t.matches_query(&json!({"url": "http://127.0.0.1/*"}), true));
        assert!(t.matches_query(&json!({"url": ["https://x/*", "*://*/index.html"]}), true));
        assert!(!t.matches_query(&json!({"url": "https://*/*"}), true));
        assert!(t.matches_query(&json!({"title": "Vsesvit*"}), true));
        assert!(!t.matches_query(&json!({"title": "Other"}), true));
        assert!(t.matches_query(&json!({"index": 2, "windowId": 1}), true));
        assert!(!t.matches_query(&json!({"windowId": 2}), true));
        assert!(t.matches_query(&json!({"windowId": -2}), true));
    }

    #[test]
    fn tab_json_and_ids() {
        let v = tab().to_json_for(true);
        assert_eq!(v["id"], 7);
        assert_eq!(v["active"], true);
        assert_eq!(v["url"], "http://127.0.0.1:8080/index.html");
        assert_eq!(v["title"], "Vsesvit fixture");
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
        assert!(t.matches_query(&json!({"active": true}), false));
        assert!(!t.matches_query(&json!({"url": "http://127.0.0.1/*"}), false));
        assert!(!t.matches_query(&json!({"title": "Vsesvit*"}), false));
    }
}
