//! The wire protocol between the JS shim (`src/js/api.js`) and Rust.
//!
//! Every `chrome.*` call that needs the browser posts one JSON object through
//! `window.webkit.messageHandlers.<handler>.postMessage(...)`, whose Promise resolves with
//! the reply:
//!
//! ```text
//! request:  {"m": "storage.get", "a": ["local", ["visits"]], "u": "<location.href>", "top": true}
//! reply:    {"v": <json>}      the call's result
//!           {}                 the call returned undefined
//!           rejected Promise   the call failed; the message is what chrome.runtime.lastError shows
//! ```
//!
//! Rust → JS goes the other way through `evaluate_javascript` (fire-and-forget events,
//! see [`emit_source`]) and `call_async_javascript_function` (message dispatch, which
//! awaits the page's answer as [`Dispatched`]).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Error text Chrome uses when nobody listens for a message.
pub const NO_RECEIVER: &str = "Could not establish connection. Receiving end does not exist.";

/// One call from the shim.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Call {
    #[serde(rename = "m")]
    pub method: Method,
    #[serde(rename = "a", default)]
    pub args: Vec<Value>,
    /// `location.href` of the calling document.
    #[serde(rename = "u", default)]
    pub url: Option<String>,
    #[serde(rename = "top", default)]
    pub top_frame: bool,
}

impl Call {
    pub fn from_json(text: &str) -> Result<Call, String> {
        serde_json::from_str(text).map_err(|e| format!("malformed call: {e}"))
    }

    pub fn arg(&self, i: usize) -> &Value {
        self.args.get(i).unwrap_or(&Value::Null)
    }
}

/// Every API the runtime implements on the Rust side. Anything else (`i18n`, `getURL`,
/// `permissions.contains`) is answered inside the shim from the embedded manifest.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Method {
    RuntimeSendMessage,
    RuntimeOpenOptionsPage,
    StorageGet,
    StorageSet,
    StorageRemove,
    StorageClear,
    StorageGetBytesInUse,
    TabsQuery,
    TabsGet,
    TabsGetCurrent,
    TabsCreate,
    TabsUpdate,
    TabsRemove,
    TabsReload,
    TabsSendMessage,
    ScriptingExecuteScript,
    ScriptingInsertCss,
    ActionSetBadgeText,
    ActionGetBadgeText,
    ActionSetTitle,
    ActionGetTitle,
    ActionSetIcon,
    ActionSetPopup,
    ActionGetPopup,
    ActionNoop,
    AlarmsCreate,
    AlarmsGet,
    AlarmsGetAll,
    AlarmsClear,
    AlarmsClearAll,
}

impl Method {
    pub const fn name(self) -> &'static str {
        match self {
            Method::RuntimeSendMessage => "runtime.sendMessage",
            Method::RuntimeOpenOptionsPage => "runtime.openOptionsPage",
            Method::StorageGet => "storage.get",
            Method::StorageSet => "storage.set",
            Method::StorageRemove => "storage.remove",
            Method::StorageClear => "storage.clear",
            Method::StorageGetBytesInUse => "storage.getBytesInUse",
            Method::TabsQuery => "tabs.query",
            Method::TabsGet => "tabs.get",
            Method::TabsGetCurrent => "tabs.getCurrent",
            Method::TabsCreate => "tabs.create",
            Method::TabsUpdate => "tabs.update",
            Method::TabsRemove => "tabs.remove",
            Method::TabsReload => "tabs.reload",
            Method::TabsSendMessage => "tabs.sendMessage",
            Method::ScriptingExecuteScript => "scripting.executeScript",
            Method::ScriptingInsertCss => "scripting.insertCSS",
            Method::ActionSetBadgeText => "action.setBadgeText",
            Method::ActionGetBadgeText => "action.getBadgeText",
            Method::ActionSetTitle => "action.setTitle",
            Method::ActionGetTitle => "action.getTitle",
            Method::ActionSetIcon => "action.setIcon",
            Method::ActionSetPopup => "action.setPopup",
            Method::ActionGetPopup => "action.getPopup",
            Method::ActionNoop => "action.noop",
            Method::AlarmsCreate => "alarms.create",
            Method::AlarmsGet => "alarms.get",
            Method::AlarmsGetAll => "alarms.getAll",
            Method::AlarmsClear => "alarms.clear",
            Method::AlarmsClearAll => "alarms.clearAll",
        }
    }

    const ALL: [Method; 30] = [
        Method::RuntimeSendMessage,
        Method::RuntimeOpenOptionsPage,
        Method::StorageGet,
        Method::StorageSet,
        Method::StorageRemove,
        Method::StorageClear,
        Method::StorageGetBytesInUse,
        Method::TabsQuery,
        Method::TabsGet,
        Method::TabsGetCurrent,
        Method::TabsCreate,
        Method::TabsUpdate,
        Method::TabsRemove,
        Method::TabsReload,
        Method::TabsSendMessage,
        Method::ScriptingExecuteScript,
        Method::ScriptingInsertCss,
        Method::ActionSetBadgeText,
        Method::ActionGetBadgeText,
        Method::ActionSetTitle,
        Method::ActionGetTitle,
        Method::ActionSetIcon,
        Method::ActionSetPopup,
        Method::ActionGetPopup,
        Method::ActionNoop,
        Method::AlarmsCreate,
        Method::AlarmsGet,
        Method::AlarmsGetAll,
        Method::AlarmsClear,
        Method::AlarmsClearAll,
    ];

    /// Content scripts get the subset Chrome gives them; everything else is for
    /// extension pages only.
    pub fn allowed_in_content_script(self) -> bool {
        matches!(
            self,
            Method::RuntimeSendMessage
                | Method::StorageGet
                | Method::StorageSet
                | Method::StorageRemove
                | Method::StorageClear
                | Method::StorageGetBytesInUse
        )
    }
}

impl FromStr for Method {
    type Err = UnknownMethod;
    fn from_str(s: &str) -> Result<Method, UnknownMethod> {
        Method::ALL.iter().copied().find(|m| m.name() == s).ok_or_else(|| UnknownMethod(s.to_owned()))
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0} is not supported by Vsesvit")]
pub struct UnknownMethod(pub String);

impl<'de> Deserialize<'de> for Method {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Method, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// `chrome.storage` area names as the shim sends them.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StorageArea {
    Local,
    Sync,
}

impl StorageArea {
    pub fn from_arg(v: &Value) -> Result<StorageArea, String> {
        match v.as_str() {
            Some("local") => Ok(StorageArea::Local),
            Some("sync") => Ok(StorageArea::Sync),
            Some(other) => Err(format!("storage area {other:?} is not supported")),
            None => Err("storage area must be a string".into()),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            StorageArea::Local => "local",
            StorageArea::Sync => "sync",
        }
    }
}

/// The reply payload for a successful call. `None` is JavaScript `undefined`.
pub fn reply_json(value: Option<Value>) -> String {
    match value {
        Some(v) => serde_json::json!({ "v": v }).to_string(),
        None => "{}".to_owned(),
    }
}

/// What `__vsesvit.dispatchMessage` resolves with inside a page.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Dispatched {
    /// No `runtime.onMessage` listener in that context.
    #[serde(default)]
    pub none: bool,
    /// The response, when a listener answered with a value.
    #[serde(rename = "v")]
    pub value: Option<Value>,
    /// A listener's Promise rejected.
    #[serde(rename = "e")]
    pub error: Option<String>,
}

impl Dispatched {
    pub fn parse(json: Option<&str>) -> Dispatched {
        json.and_then(|j| serde_json::from_str(j).ok()).unwrap_or(Dispatched { none: true, value: None, error: None })
    }
}

/// `chrome.runtime.MessageSender` for a message the runtime delivers.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Sender {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<Value>,
    #[serde(rename = "frameId", skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<u32>,
}

impl Sender {
    pub fn origin_of(url: &str) -> Option<String> {
        let u = url::Url::parse(url).ok()?;
        match u.origin() {
            url::Origin::Tuple(..) => Some(u.origin().ascii_serialization()),
            url::Origin::Opaque(_) if u.scheme() == "chrome-extension" => {
                Some(format!("chrome-extension://{}", u.host_str().unwrap_or_default()))
            }
            url::Origin::Opaque(_) => None,
        }
    }
}

/// JavaScript source that fires an event in a context that has the shim, and is a no-op
/// anywhere else (a tab the extension never injected into, a page loaded before the
/// extension).
pub fn emit_source(event: &str, args: &[Value]) -> String {
    let mut s = format!("globalThis.__vsesvit && globalThis.__vsesvit.emit({}", js_string(event));
    for a in args {
        s.push_str(", ");
        s.push_str(&js_literal(a));
    }
    s.push_str(");");
    s
}

/// Body for `call_async_javascript_function`: delivers `message` to the context's
/// `runtime.onMessage` listeners and resolves with a [`Dispatched`].
pub fn dispatch_source(message: &Value, sender: &Sender) -> String {
    let sender = serde_json::to_value(sender).unwrap_or(Value::Null);
    format!(
        "if (!globalThis.__vsesvit) return {{ none: true }}; return globalThis.__vsesvit.dispatchMessage({}, {});",
        js_literal(message),
        js_literal(&sender)
    )
}

/// A JSON value as a JavaScript expression. JSON is a JavaScript subset since ES2019
/// (U+2028/2029 allowed in string literals), so the serialized text is the literal.
pub fn js_literal(v: &Value) -> String {
    v.to_string()
}

pub fn js_string(s: &str) -> String {
    Value::String(s.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_call() {
        let c = Call::from_json(r#"{"m":"storage.get","a":["local",["visits"]],"u":"http://x/","top":true}"#).unwrap();
        assert_eq!(c.method, Method::StorageGet);
        assert_eq!(c.args.len(), 2);
        assert_eq!(StorageArea::from_arg(c.arg(0)).unwrap(), StorageArea::Local);
        assert_eq!(c.arg(1), &json!(["visits"]));
        assert_eq!(c.arg(5), &Value::Null);
        assert!(c.top_frame);
        assert_eq!(c.url.as_deref(), Some("http://x/"));
    }

    #[test]
    fn defaults_and_rejections() {
        let c = Call::from_json(r#"{"m":"alarms.getAll"}"#).unwrap();
        assert!(c.args.is_empty() && !c.top_frame && c.url.is_none());
        let err = Call::from_json(r#"{"m":"tabs.captureVisibleTab","a":[]}"#).unwrap_err();
        assert!(err.contains("tabs.captureVisibleTab is not supported"), "{err}");
        assert!(Call::from_json("nonsense").is_err());
        assert!(StorageArea::from_arg(&json!("session")).is_err());
        assert!(StorageArea::from_arg(&json!(1)).is_err());
    }

    #[test]
    fn every_method_round_trips_through_its_name() {
        for m in Method::ALL {
            assert_eq!(m.name().parse::<Method>().unwrap(), m);
            assert_eq!(m.to_string(), m.name());
        }
        assert!(Method::StorageSet.allowed_in_content_script());
        assert!(!Method::TabsCreate.allowed_in_content_script());
    }

    #[test]
    fn reply_shapes() {
        assert_eq!(reply_json(None), "{}");
        assert_eq!(reply_json(Some(json!({"ok": true}))), r#"{"v":{"ok":true}}"#);
        assert_eq!(reply_json(Some(Value::Null)), r#"{"v":null}"#);
    }

    #[test]
    fn dispatched_parsing() {
        assert_eq!(Dispatched::parse(Some(r#"{"v":{"ok":true}}"#)).value, Some(json!({"ok": true})));
        assert!(Dispatched::parse(Some(r#"{"none":true}"#)).none);
        assert!(Dispatched::parse(None).none);
        assert!(Dispatched::parse(Some("garbage")).none);
        let d = Dispatched::parse(Some("{}"));
        assert!(!d.none && d.value.is_none() && d.error.is_none());
    }

    #[test]
    fn sender_and_sources() {
        assert_eq!(Sender::origin_of("http://127.0.0.1:8080/index.html").as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(Sender::origin_of("chrome-extension://abc/popup.html").as_deref(), Some("chrome-extension://abc"));
        assert_eq!(Sender::origin_of("about:blank"), None);
        let src = emit_source("tabs.onUpdated", &[json!(3), json!({"url": "http://x/"})]);
        assert_eq!(src, r#"globalThis.__vsesvit && globalThis.__vsesvit.emit("tabs.onUpdated", 3, {"url":"http://x/"});"#);
        let s = Sender { id: "ext".into(), url: Some("http://x/".into()), ..Sender::default() };
        let d = dispatch_source(&json!({"type": "hello"}), &s);
        assert!(d.starts_with("if (!globalThis.__vsesvit) return { none: true };"));
        assert!(d.contains(r#"dispatchMessage({"type":"hello"}, {"id":"ext","url":"http://x/"})"#), "{d}");
        assert_eq!(js_string("a\"b</script>"), r#""a\"b</script>""#);
    }
}
