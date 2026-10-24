//! Owned by W1.3.
//! DevTools protocol calls on a page's own session (`execute_dev_tools_method`) and their results
//! (`DevToolsMessageObserver::on_dev_tools_method_result`, on UI), keyed by message id and answered
//! with the `RequestId` the shell got. `eval`, `capture_page`, `save_mhtml` and `print_to_pdf` are
//! calls here; later units use [`call_then`] for their own (site data, the extension bridge).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use base64::Engine as _;
use cef::*;
use serde_json::{Value, json};
use vsesvit_engine::{BrowserEvent, RequestId, RequestResult, TabId};

use crate::registry::{self, BrowserId};

/// What a call's result becomes.
pub enum Then {
    /// `Runtime.evaluate` → `RequestResult::Eval`.
    Eval,
    /// `Page.captureScreenshot` → `RequestResult::Captured`.
    Capture,
    /// `Page.captureSnapshot` written to the path → `RequestResult::MhtmlSaved`.
    Mhtml(PathBuf),
    /// `Page.printToPDF` written to the path → `RequestResult::PdfPrinted`.
    Pdf(PathBuf),
    /// Another unit's own handling of the raw result.
    Call(Box<dyn FnOnce(Result<Value, String>)>),
}

/// The calls in flight, by DevTools message id. In the registry.
#[derive(Default)]
pub struct Calls(HashMap<i32, (Option<RequestId>, Then)>);

impl Calls {
    /// The message ids in flight.
    pub fn ids(&self) -> Vec<i32> {
        self.0.keys().copied().collect()
    }
}

fn next_message_id() -> i32 {
    static NEXT: AtomicI32 = AtomicI32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Calls `method` on the browser's page session; `then` gets the result.
pub fn call_on(browser: BrowserId, method: &str, params: Value, request: Option<RequestId>, then: Then) {
    let message = next_message_id();
    let mut then = Some(then);
    let page = registry::with(|r| {
        let page = r.entries.get(&browser).map(|e| e.page.clone())?;
        r.devtools.0.insert(message, (request, then.take().expect("taken once")));
        Some(page)
    })
    .flatten();
    match (page, then) {
        (Some(page), _) => {
            if !page.devtools_method(message, method, &params) {
                finish(message, Err(format!("CEF refused {method}")));
            }
        }
        (None, Some(then)) => match request {
            Some(id) => finish_request(id, then, Err("no such browser".to_owned())),
            None => {
                result_of(then, Err("no such browser".to_owned()));
            }
        },
        (None, None) => {}
    }
}

/// Calls `method` on a tab and answers the returned `RequestId` through `then`.
pub fn call(tab: TabId, method: &str, params: Value, then: Then) -> RequestId {
    let id = registry::next_request();
    let browser = registry::with(|r| r.tabs.get(&tab).copied()).flatten();
    match browser {
        Some(browser) => call_on(browser, method, params, Some(id), then),
        None => finish_request(id, then, Err("no such tab".to_owned())),
    }
    id
}

/// For other units: a call whose raw result goes to `f`.
pub fn call_then(browser: BrowserId, method: &str, params: Value, f: impl FnOnce(Result<Value, String>) + 'static) {
    call_on(browser, method, params, None, Then::Call(Box::new(f)));
}

/// `Engine::eval`.
pub fn eval(tab: TabId, script: &str) -> RequestId {
    call(tab, "Runtime.evaluate", json!({"expression": script, "returnByValue": true, "awaitPromise": true}), Then::Eval)
}

/// `Engine::capture_page`.
pub fn capture(tab: TabId) -> RequestId {
    call(tab, "Page.captureScreenshot", json!({"format": "png"}), Then::Capture)
}

/// `Engine::save_mhtml`.
pub fn save_mhtml(tab: TabId, path: PathBuf) -> RequestId {
    call(tab, "Page.captureSnapshot", json!({"format": "mhtml"}), Then::Mhtml(path))
}

/// `Engine::print_to_pdf`.
pub fn print_to_pdf(tab: TabId, path: PathBuf) -> RequestId {
    call(tab, "Page.printToPDF", json!({}), Then::Pdf(path))
}

/// `Engine::show_devtools`: a DevTools window of its own, created with our client so its life span
/// reaches the registry. Does nothing when one is open.
pub fn show(tab: TabId, inspect_at: Option<vsesvit_engine::PagePoint>) {
    let Some(page) = registry::page_of(tab) else { return };
    if page.has_devtools() {
        return;
    }
    let browser = registry::with(|r| r.tabs.get(&tab).copied()).flatten();
    let Some(creation) = browser.and_then(crate::handlers::life_span::devtools_creation) else { return };
    page.show_devtools(creation, inspect_at.map(|p| (p.x, p.y)));
}

/// `Engine::show_console`: Chrome's own DevTools-console command, let through the default-deny
/// filter once; its window arrives through `on_before_dev_tools_popup`.
pub fn show_console(tab: TabId) {
    let Some(command) = crate::idc::id_of("IDC_DEV_TOOLS_CONSOLE") else { return };
    registry::on_tab(tab, |page| crate::idc::issuing(command, || page.execute_chrome_command(command)));
}

/// `Engine::close_devtools`.
pub fn close(tab: TabId) {
    registry::on_tab(tab, |page| page.close_devtools());
}

/// `Engine::has_devtools`.
pub fn has(tab: TabId) -> bool {
    registry::on_tab(tab, |page| page.has_devtools()).unwrap_or(false)
}

/// A method result arrived (UI thread). `success` false carries CEF's error object.
pub fn on_result(message: i32, success: bool, result: &[u8]) {
    let parsed: Result<Value, String> = match serde_json::from_slice::<Value>(result) {
        Ok(value) if success => Ok(value),
        Ok(error) => Err(error.get("message").and_then(Value::as_str).unwrap_or("DevTools error").to_owned()),
        Err(e) => Err(format!("unreadable DevTools result: {e}")),
    };
    finish(message, parsed);
}

fn finish(message: i32, result: Result<Value, String>) {
    let Some(Some((request, then))) = registry::with(|r| r.devtools.0.remove(&message)) else { return };
    match request {
        Some(id) => finish_request(id, then, result),
        None => {
            if let Then::Call(f) = then {
                f(result);
            }
        }
    }
}

fn data(value: &Value) -> Result<&str, String> {
    value.get("data").and_then(Value::as_str).ok_or_else(|| "no data in the result".to_owned())
}

fn decode(value: &Value) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD.decode(data(value)?).map_err(|e| e.to_string())
}

fn write(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// What a raw result means for each kind of call.
pub fn result_of(then: Then, raw: Result<Value, String>) -> Option<RequestResult> {
    Some(match then {
        Then::Eval => RequestResult::Eval(raw.and_then(|v| eval_result(&v))),
        Then::Capture => RequestResult::Captured(raw.and_then(|v| decode(&v))),
        Then::Mhtml(path) => RequestResult::MhtmlSaved(raw.and_then(|v| write(&path, data(&v)?.as_bytes()))),
        Then::Pdf(path) => RequestResult::PdfPrinted(raw.and_then(|v| write(&path, &decode(&v)?))),
        Then::Call(f) => {
            f(raw);
            return None;
        }
    })
}

/// `Runtime.evaluate`'s result: the value's JSON text, or the exception's description.
pub fn eval_result(v: &Value) -> Result<String, String> {
    if let Some(details) = v.get("exceptionDetails") {
        let text = details
            .pointer("/exception/description")
            .or_else(|| details.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("exception");
        return Err(text.to_owned());
    }
    let value = v.pointer("/result/value").cloned().unwrap_or(Value::Null);
    Ok(value.to_string())
}

fn finish_request(id: RequestId, then: Then, raw: Result<Value, String>) {
    if let Some(result) = result_of(then, raw)
        && let Some(sink) = registry::sink()
    {
        sink.send(BrowserEvent::RequestFinished { id, result });
    }
}

wrap_dev_tools_message_observer! {
    pub struct Observer {
        _keep: Arc<()>,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _browser: Option<&mut Browser>, message_id: i32, success: i32, result: Option<&[u8]>) {
            on_result(message_id, success != 0, result.unwrap_or_default());
        }
    }
}

impl Observer {
    /// The one observer; each browser that is called registers it once.
    pub fn shared() -> DevToolsMessageObserver {
        Observer::new(Arc::new(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_results_are_json_text_or_the_exception() {
        assert_eq!(eval_result(&json!({"result": {"type": "number", "value": 3}})), Ok("3".to_owned()));
        assert_eq!(eval_result(&json!({"result": {"type": "string", "value": "a\"b"}})), Ok("\"a\\\"b\"".to_owned()));
        assert_eq!(eval_result(&json!({"result": {"type": "undefined"}})), Ok("null".to_owned()));
        assert_eq!(
            eval_result(&json!({"result": {}, "exceptionDetails": {"text": "Uncaught", "exception": {"description": "ReferenceError: x"}}})),
            Err("ReferenceError: x".to_owned())
        );
    }

    #[test]
    fn captures_decode_and_files_are_written() {
        let png = result_of(Then::Capture, Ok(json!({"data": "iVBORw=="})));
        assert_eq!(png, Some(RequestResult::Captured(Ok(vec![0x89, b'P', b'N', b'G']))));
        let dir = std::env::temp_dir().join(format!("vsesvit-cef-devtools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mhtml = dir.join("page.mhtml");
        assert_eq!(
            result_of(Then::Mhtml(mhtml.clone()), Ok(json!({"data": "MIME-Version: 1.0"}))),
            Some(RequestResult::MhtmlSaved(Ok(())))
        );
        assert_eq!(std::fs::read_to_string(&mhtml).expect("written"), "MIME-Version: 1.0");
        assert_eq!(
            result_of(Then::Pdf(dir.join("p.pdf")), Err("Printing failed".into())),
            Some(RequestResult::PdfPrinted(Err("Printing failed".into())))
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
