//! The wire protocol between the JS shim (`src/js/api.js`) and Rust.
//!
//! Every `chrome.*` call that needs the browser posts one JSON object through
//! `window.webkit.messageHandlers.<handler>.postMessage(...)`, whose Promise resolves with
//! the reply:
//!
//! ```text
//! request:  {"m": "storage.get", "a": ["local", ["visits"]], "u": "<location.href>", "top": true, "t": "<token>"}
//! reply:    {"v": <json>}      the call's result
//!           {}                 the call returned undefined
//!           rejected Promise   the call failed; the message is what chrome.runtime.lastError shows
//! ```
//!
//! `t` is the extension-page token: the page handler lives in the default world, where
//! any document in the same view could reach it, so a call is honoured only when it
//! carries the secret the page bootstrap (injected into that extension's documents only)
//! was given. Content scripts talk to a handler in their isolated world and send no token.
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
    /// The extension-page token (see the module docs); absent from content scripts.
    #[serde(rename = "t", default)]
    pub token: Option<String>,
}

/// The shim with `config` bound: `api.js` is one function expression taking the
/// context's configuration, so the bootstrap declares nothing in the global scope and
/// may run any number of times in one world (each `content_scripts` entry, then
/// `scripting.executeScript`); the shim's own `__vsesvit` guard makes repeats no-ops.
pub fn bootstrap(config: &Value) -> String {
    format!("{}({});\n", crate::API_JS.trim_end(), config)
}

/// A JavaScript expression that is true only when evaluated in a document of the
/// extension whose URL host is `host`. Both `location` accessors are unforgeable, so a
/// web page that happens to share the view cannot make it true.
pub fn page_guard(host: &str) -> String {
    format!("(location.protocol === \"chrome-extension:\" && location.host === {})", js_string(host))
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

/// The one answer a message sender gets from every context the message went to, as in
/// Chrome: the first response (or listener error) wins; without one, `undefined` once
/// every context has finished, or [`NO_RECEIVER`] when none had a listener.
#[derive(Clone, Debug)]
pub struct Replies {
    remaining: usize,
    any_listener: bool,
    done: bool,
}

impl Replies {
    pub fn new(targets: usize) -> Replies {
        Replies { remaining: targets, any_listener: false, done: false }
    }

    /// One context finished. `Some` is the answer, exactly once.
    pub fn settle(&mut self, d: Dispatched) -> Option<Result<Option<Value>, String>> {
        self.remaining = self.remaining.saturating_sub(1);
        if self.done {
            return None;
        }
        self.any_listener |= !d.none;
        let answer = if let Some(e) = d.error {
            Err(e)
        } else if d.value.is_some() {
            Ok(d.value)
        } else if self.remaining > 0 {
            return None;
        } else if self.any_listener {
            Ok(None)
        } else {
            Err(NO_RECEIVER.to_owned())
        };
        self.done = true;
        Some(answer)
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

/// [`emit_source`] for the main world of a tab that may or may not show one of the
/// extension's own pages: outside them it evaluates nothing, not even the arguments.
pub fn emit_source_in_page(host: &str, event: &str, args: &[Value]) -> String {
    format!("{} && {}", page_guard(host), emit_source(event, args))
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

/// [`dispatch_source`] guarded like [`emit_source_in_page`].
pub fn dispatch_source_in_page(host: &str, message: &Value, sender: &Sender) -> String {
    format!("if (!{}) return {{ none: true }}; {}", page_guard(host), dispatch_source(message, sender))
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

    /// Chrome delivers `runtime.sendMessage` to every context and the first response wins,
    /// so a background listener that does not answer keeps no other page from answering.
    #[test]
    fn replies_wait_for_every_context_and_take_the_first_answer() {
        let parsed = |j: &str| Dispatched::parse(Some(j));
        let mut r = Replies::new(2);
        assert_eq!(r.settle(parsed("{}")), None, "a listener without an answer");
        assert_eq!(r.settle(parsed(r#"{"v":"pong"}"#)), Some(Ok(Some(json!("pong")))));

        let mut r = Replies::new(3);
        assert_eq!(r.settle(parsed(r#"{"v":1}"#)), Some(Ok(Some(json!(1)))));
        assert_eq!(r.settle(parsed(r#"{"e":"late"}"#)), None, "answered once");
        assert_eq!(r.settle(parsed(r#"{"v":2}"#)), None);

        let mut r = Replies::new(2);
        assert_eq!(r.settle(parsed(r#"{"none":true}"#)), None);
        assert_eq!(r.settle(parsed(r#"{"none":true}"#)), Some(Err(NO_RECEIVER.to_owned())));

        let mut r = Replies::new(2);
        assert_eq!(r.settle(parsed(r#"{"none":true}"#)), None);
        assert_eq!(r.settle(parsed("{}")), Some(Ok(None)), "undefined once every context finished");

        let mut r = Replies::new(2);
        assert_eq!(r.settle(parsed(r#"{"e":"boom"}"#)), Some(Err("boom".to_owned())));
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

    #[test]
    fn page_calls_carry_a_token_and_content_calls_do_not() {
        let page = Call::from_json(r#"{"m":"tabs.query","a":[{}],"t":"s3cret"}"#).unwrap();
        assert_eq!(page.token.as_deref(), Some("s3cret"));
        let content = Call::from_json(r#"{"m":"storage.get","a":["local",null]}"#).unwrap();
        assert_eq!(content.token, None);
    }

    /// Two `content_scripts` entries run as two user scripts in one world, so the
    /// bootstrap may not declare anything at the top level (a repeated `const` is a
    /// SyntaxError before the shim's own guard can run).
    #[test]
    fn bootstrap_declares_nothing_global_and_binds_the_config_as_an_argument() {
        let config = json!({ "id": "twin@vsesvit.test", "host": "0123456789abcdef0123456789abcdef", "handler": "h", "kind": "page", "token": "s3cret" });
        let src = bootstrap(&config);
        assert!(src.starts_with("(function (config)"), "{}", &src[..60]);
        assert!(src.trim_end().ends_with(&format!("}})({config});")), "{}", &src[src.len() - 120..]);
        assert!(!src.contains("__VSESVIT_CONFIG__"), "the config must not be a global binding");
        for decl in ["\nconst ", "\nlet ", "\nvar "] {
            assert!(!src.contains(decl), "top-level {decl:?} declaration in the bootstrap");
        }
        assert_eq!(src.matches("(function (config)").count(), 1);
    }

    /// The shim's structural contracts with the Rust side: URLs are built on the URL host
    /// (a Gecko id is not a valid host, see `extension::url_host`), and every page call
    /// carries the token.
    #[test]
    fn shim_uses_the_url_host_and_sends_the_token() {
        let shim = crate::API_JS;
        assert!(shim.contains(r#"const baseUrl = "chrome-extension://" + config.host + "/";"#), "getURL must use config.host");
        assert!(!shim.contains(r#""chrome-extension://" + config.id"#));
        assert!(shim.contains("t: config.token"), "calls must carry the page token");
    }

    #[test]
    fn page_guards_wrap_the_sources() {
        let guard = page_guard("abc");
        assert_eq!(guard, r#"(location.protocol === "chrome-extension:" && location.host === "abc")"#);
        let e = emit_source_in_page("abc", "alarms.onAlarm", &[json!({"name": "n"})]);
        assert_eq!(e, format!("{guard} && {}", emit_source("alarms.onAlarm", &[json!({"name": "n"})])));
        let d = dispatch_source_in_page("abc", &json!(1), &Sender::default());
        assert!(d.starts_with(&format!("if (!{guard}) return {{ none: true }}; if (!globalThis.__vsesvit)")), "{d}");
    }
}
