//! Owned by W1.6.
//! Slow engine operations as tasks (UA section 4.3): the engine answers a `RequestId` later with
//! `BrowserEvent::RequestFinished`, and the router completes the task that waits on it. The same path
//! serves eval, capture, PDF, MHTML, cookies and site data.

use std::collections::HashMap;

use futures::channel::oneshot;
use gpui::{App, Task};
use vsesvit_engine::{Engine, RequestId, RequestResult, TabId};

/// The tasks waiting on the engine, by request.
#[derive(Default)]
pub(crate) struct Pending(HashMap<RequestId, oneshot::Sender<RequestResult>>);

/// Starts an operation and waits for its result. `None` only if the bridge is dropped first; a request the
/// engine never answers (its tab closed) stays pending until the app quits.
pub fn send(cx: &mut App, start: impl FnOnce(&dyn Engine) -> RequestId) -> Task<Option<RequestResult>> {
    let engine = super::engine(cx);
    let id = start(&*engine);
    let (done, result) = oneshot::channel();
    if super::bridge_mut(cx).requests.0.insert(id, done).is_some() {
        log::error!("requests: the engine reused request id {id:?}");
    }
    cx.foreground_executor().spawn(async move { result.await.ok() })
}

/// Evaluates a script in the tab's main frame.
pub fn eval(cx: &mut App, tab: TabId, script: &str) -> Task<Option<RequestResult>> {
    send(cx, |engine| engine.eval(tab, script))
}

/// Captures the tab's page as a PNG.
pub fn capture(cx: &mut App, tab: TabId) -> Task<Option<RequestResult>> {
    send(cx, |engine| engine.capture_page(tab))
}

pub(crate) fn complete(id: RequestId, result: RequestResult, cx: &mut App) {
    match super::bridge_mut(cx).requests.0.remove(&id) {
        Some(done) => {
            // The task may have been dropped; then nobody wants the result.
            let _ = done.send(result);
        }
        None => log::debug!("requests: a result for {id:?}, which nobody waits on"),
    }
}
