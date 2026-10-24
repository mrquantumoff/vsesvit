//! Owned by W1.3.
//! The prompt protocol in the glue (design section 1.5): `PromptId`s, the callback registry's
//! take-once rule, admission by scope, and cancellation. The bookkeeping is `vsesvit_engine`'s
//! `PromptLedger`, the same one `FakeEngine` runs; this file adds the CEF side: the per-kind answer and
//! cancel actions, the scope atomics any thread reads, and the rule that a callback is always called
//! outside the registry borrow.
//!
//! Every prompt is admitted, answered and ended on main. A handler on the UI thread admits inside its
//! callback; an IO-thread handler reads [`scope_now`] there and admits through [`queue_from_io`].

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cef::{
    AuthCallback, CefString, CefStringList, EventFlags, FileDialogCallback, ImplAuthCallback, ImplFileDialogCallback,
    ImplJsdialogCallback, ImplMediaAccessCallback, ImplPermissionPromptCallback, ImplRunContextMenuCallback,
    JsdialogCallback, MediaAccessCallback, PermissionPromptCallback, PermissionRequestResult, RunContextMenuCallback,
};
use vsesvit_engine::{
    BrowserEvent, PermissionAnswer, PromptAnswer, PromptEnd, PromptError, PromptId, PromptRequest, PromptScope, Refused,
    ScopeState,
};

use crate::registry::{self, BrowserId};

/// A browser's scope values, readable from any thread (section 1.5, Scope): the document generation,
/// raised on main when the main frame commits a new document, and whether the glue is closing it.
#[derive(Debug, Default)]
pub struct ScopeAtoms {
    /// The document generation.
    pub generation: AtomicU64,
    /// Set when the glue force-closes the browser and in `on_before_close`.
    pub closing: AtomicBool,
}

static SCOPES: Mutex<BTreeMap<BrowserId, Arc<ScopeAtoms>>> = Mutex::new(BTreeMap::new());

/// Registers a new browser's atomics (`on_after_created`).
pub fn register_scope(browser: BrowserId) -> Arc<ScopeAtoms> {
    let atoms = Arc::new(ScopeAtoms::default());
    if let Ok(mut map) = SCOPES.lock() {
        map.insert(browser, atoms.clone());
    }
    atoms
}

/// Forgets a closed browser's atomics (`on_before_close`).
pub fn forget_scope(browser: BrowserId) {
    if let Ok(mut map) = SCOPES.lock() {
        map.remove(&browser);
    }
}

fn atoms(browser: BrowserId) -> Option<Arc<ScopeAtoms>> {
    SCOPES.lock().ok()?.get(&browser).cloned()
}

/// The scope a prompt asked for now belongs to: the browser and its current generation. Any thread.
pub fn scope_now(browser: BrowserId) -> Option<PromptScope<BrowserId>> {
    let atoms = atoms(browser)?;
    Some(PromptScope { browser, generation: atoms.generation.load(Ordering::SeqCst) })
}

fn state_of(browser: BrowserId) -> Option<ScopeState> {
    let atoms = atoms(browser)?;
    Some(ScopeState { generation: atoms.generation.load(Ordering::SeqCst), closing: atoms.closing.load(Ordering::SeqCst) })
}

/// One kept engine callback with its two actions (section 1.5's table). Called at most once, on main,
/// outside any registry borrow.
pub trait PromptCallback {
    /// The answer action. The answer's kind is the prompt's (the ledger checked it).
    fn answer(self: Box<Self>, answer: PromptAnswer);
    /// The cancel action.
    fn cancel(self: Box<Self>);
}

/// Admits a prompt (section 1.5, Admission). `scope` is where the engine asked, `None` for a request
/// of no browser. On a refusal the callback's cancel action runs at once and the shell never sees it.
pub fn admit(
    scope: Option<PromptScope<BrowserId>>,
    request: PromptRequest,
    callback: Box<dyn PromptCallback>,
) -> Result<PromptId, Refused> {
    let kind = request.kind();
    let mut callback = Some(callback);
    let admitted = registry::with(|r| {
        let cb = callback.take().expect("taken once");
        match r.prompts.admit(kind, scope, state_of, cb) {
            Ok(id) => Ok((id, r.sink.clone())),
            Err((why, cb)) => Err((why, cb)),
        }
    });
    match admitted {
        Some(Ok((id, sink))) => {
            sink.send(BrowserEvent::Prompt { id, request });
            Ok(id)
        }
        Some(Err((why, cb))) => {
            cb.cancel();
            Err(why)
        }
        None => {
            if let Some(cb) = callback {
                cb.cancel();
            }
            Err(Refused::ShuttingDown)
        }
    }
}

/// `Engine::answer_prompt`: exactly once, the right kind, outside the borrow.
pub fn answer(id: PromptId, answer: PromptAnswer) -> Result<(), PromptError> {
    let callback = registry::with(|r| r.prompts.answer(id, &answer)).unwrap_or(Err(PromptError::Unknown))?;
    callback.answer(answer);
    Ok(())
}

fn ended(ended: Vec<(PromptId, Box<dyn PromptCallback>)>, why: PromptEnd) {
    let Some(sink) = registry::sink() else { return };
    for (id, callback) in ended {
        sink.send(BrowserEvent::PromptEnded { id, why });
        if why.engine_side() {
            // CEF has given the callback up already (section 1.5, Ends).
            drop(callback);
        } else {
            callback.cancel();
        }
    }
}

/// Ends one prompt for an engine-side reason. False if it was not open.
pub fn end(id: PromptId, why: PromptEnd) -> bool {
    match registry::with(|r| r.prompts.end(id)).and_then(Result::ok) {
        Some(callback) => {
            ended(vec![(id, callback)], why);
            true
        }
        None => false,
    }
}

/// Ends every prompt of a browser: `Navigated` when its main frame committed a new document, or
/// `TabClosed`.
pub fn end_browser(browser: BrowserId, why: PromptEnd) {
    if let Some(list) = registry::with(|r| r.prompts.end_browser(browser)) {
        ended(list, why);
    }
}

/// Stops admission and ends every open prompt with `Shutdown` (section 3.6 step 3). Idempotent.
pub fn shut() {
    if let Some(list) = registry::with(|r| r.prompts.shut()) {
        ended(list, PromptEnd::Shutdown);
    }
}

/// The main frame committed a new document: raise the generation, end the old document's prompts.
pub fn new_document(browser: BrowserId) {
    if let Some(atoms) = atoms(browser) {
        atoms.generation.fetch_add(1, Ordering::SeqCst);
    }
    end_browser(browser, PromptEnd::Navigated);
}

/// An IO-thread prompt (`get_auth_credentials`): `scope` was read on IO with [`scope_now`]; admission
/// happens in a task on main, so a tab that closed or navigated in between never shows it (R2-3).
pub fn queue_from_io(
    scope: Option<PromptScope<BrowserId>>,
    request: PromptRequest,
    callback: Box<dyn PromptCallback + Send>,
) {
    crate::client::post_ui(Box::new(move || {
        let _ = admit(scope, request, callback);
    }));
}

/// The engine's kept callbacks, with section 1.5's actions.
pub enum CefPrompt {
    /// `run_context_menu`.
    ContextMenu(RunContextMenuCallback),
    /// `on_jsdialog`.
    JsDialog(JsdialogCallback),
    /// `on_before_unload_dialog`.
    BeforeUnload(JsdialogCallback),
    /// `on_file_dialog`.
    FileChooser(FileDialogCallback),
    /// `get_auth_credentials`.
    Auth(AuthCallback),
    /// `on_show_permission_prompt`.
    Permission(PermissionPromptCallback),
    /// `on_request_media_access_permission`, with the permissions asked for.
    MediaAccess {
        /// The callback.
        callback: MediaAccessCallback,
        /// `cef_media_access_permission_types_t` bits asked for.
        requested: u32,
    },
}

impl PromptCallback for CefPrompt {
    fn answer(self: Box<Self>, answer: PromptAnswer) {
        match (*self, answer) {
            (CefPrompt::ContextMenu(cb), PromptAnswer::Menu(Some(id))) => cb.cont(id.0, EventFlags::default()),
            (CefPrompt::ContextMenu(cb), PromptAnswer::Menu(None)) => cb.cancel(),
            (CefPrompt::JsDialog(cb), PromptAnswer::JsDialog { accept, text }) => {
                cb.cont(accept as i32, Some(&CefString::from(text.unwrap_or_default().as_str())))
            }
            (CefPrompt::BeforeUnload(cb), PromptAnswer::BeforeUnload { leave }) => {
                cb.cont(leave as i32, Some(&CefString::from("")))
            }
            (CefPrompt::FileChooser(cb), PromptAnswer::Files(Some(paths))) => {
                let mut list = CefStringList::new();
                for path in paths {
                    list.append(&path.display().to_string());
                }
                cb.cont(Some(&mut list));
            }
            (CefPrompt::FileChooser(cb), PromptAnswer::Files(None)) => cb.cancel(),
            (CefPrompt::Auth(cb), PromptAnswer::Auth(Some((user, pass)))) => {
                cb.cont(Some(&CefString::from(user.as_str())), Some(&CefString::from(pass.as_str())))
            }
            (CefPrompt::Auth(cb), PromptAnswer::Auth(None)) => cb.cancel(),
            (CefPrompt::Permission(cb), PromptAnswer::Permission(answer)) => cb.cont(match answer {
                PermissionAnswer::Allow => PermissionRequestResult::ACCEPT,
                PermissionAnswer::Deny => PermissionRequestResult::DENY,
                PermissionAnswer::Dismiss => PermissionRequestResult::DISMISS,
            }),
            (CefPrompt::MediaAccess { callback, requested }, PromptAnswer::Permission(PermissionAnswer::Allow)) => {
                callback.cont(requested)
            }
            (CefPrompt::MediaAccess { callback, .. }, PromptAnswer::Permission(_)) => callback.cancel(),
            (prompt, answer) => {
                log::error!("vsesvit-cef: a {:?} answer reached another kind of prompt; cancelled", answer.kind());
                Box::new(prompt).cancel();
            }
        }
    }

    fn cancel(self: Box<Self>) {
        match *self {
            CefPrompt::ContextMenu(cb) => cb.cancel(),
            CefPrompt::JsDialog(cb) | CefPrompt::BeforeUnload(cb) => cb.cont(0, Some(&CefString::from(""))),
            CefPrompt::FileChooser(cb) => cb.cancel(),
            CefPrompt::Auth(cb) => cb.cancel(),
            CefPrompt::Permission(cb) => cb.cont(PermissionRequestResult::DISMISS),
            CefPrompt::MediaAccess { callback, .. } => callback.cancel(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use vsesvit_engine::{
        AuthChallenger, BrowserEvent, ContextMenuParams, FileChooserMode, JsDialogKind, MenuCommandId, MenuModel,
        PagePoint, PermissionRequest, PromptKind, TabId,
    };

    use super::*;
    use crate::registry::testing::{self, Harness};

    type Log = Rc<RefCell<Vec<String>>>;

    /// A kept callback that records which action ran, or that it was dropped uncalled.
    struct Recorded {
        log: Log,
        done: bool,
    }

    impl PromptCallback for Recorded {
        fn answer(mut self: Box<Self>, answer: PromptAnswer) {
            self.done = true;
            self.log.borrow_mut().push(format!("answer {:?}", answer.kind()));
        }
        fn cancel(mut self: Box<Self>) {
            self.done = true;
            self.log.borrow_mut().push("cancel".into());
        }
    }

    impl Drop for Recorded {
        fn drop(&mut self) {
            if !self.done {
                self.log.borrow_mut().push("dropped".into());
            }
        }
    }

    fn callback(log: &Log) -> Box<dyn PromptCallback> {
        Box::new(Recorded { log: log.clone(), done: false })
    }

    fn request(kind: PromptKind, tab: TabId) -> PromptRequest {
        match kind {
            PromptKind::ContextMenu => PromptRequest::ContextMenu {
                tab,
                at: PagePoint::default(),
                params: ContextMenuParams::default(),
                model: MenuModel::default(),
            },
            PromptKind::JsDialog => {
                PromptRequest::JsDialog { tab, origin: None, kind: JsDialogKind::Alert, message: "hi".into() }
            }
            PromptKind::BeforeUnload => PromptRequest::BeforeUnload { tab, is_reload: false },
            PromptKind::FileChooser => PromptRequest::FileChooser {
                tab,
                mode: FileChooserMode::Open,
                title: None,
                default_path: None,
                accept: Vec::new(),
            },
            PromptKind::Auth => PromptRequest::Auth {
                tab: Some(tab),
                requester: None,
                challenger: AuthChallenger { host: "proxy".into(), port: 3128, is_proxy: true },
                realm: None,
                scheme: Some("basic".into()),
            },
            PromptKind::Permission => {
                PromptRequest::Permission(PermissionRequest { tab, origin: None, permissions: Vec::new() })
            }
        }
    }

    fn answer_for(kind: PromptKind) -> PromptAnswer {
        match kind {
            PromptKind::ContextMenu => PromptAnswer::Menu(Some(MenuCommandId(26500))),
            PromptKind::JsDialog => PromptAnswer::JsDialog { accept: true, text: None },
            PromptKind::BeforeUnload => PromptAnswer::BeforeUnload { leave: true },
            PromptKind::FileChooser => PromptAnswer::Files(None),
            PromptKind::Auth => PromptAnswer::Auth(Some(("u".into(), "p".into()))),
            PromptKind::Permission => PromptAnswer::Permission(PermissionAnswer::Allow),
        }
    }

    fn wrong_answer_for(kind: PromptKind) -> PromptAnswer {
        match kind {
            PromptKind::ContextMenu => PromptAnswer::BeforeUnload { leave: true },
            _ => PromptAnswer::Menu(None),
        }
    }

    /// A live tab with a browser, on a fresh registry.
    fn tab_with_browser() -> (Harness, TabId, BrowserId) {
        let mut h = testing::install();
        let tab = TabId::next();
        crate::handlers::life_span::create_tab(crate::engine::tests::spec(tab)).expect("created");
        let page = h.created(h.last_creation().id);
        h.events();
        (h, tab, page.id)
    }

    fn admitted(h: &mut Harness) -> Vec<PromptId> {
        h.events()
            .into_iter()
            .filter_map(|e| if let BrowserEvent::Prompt { id, .. } = e { Some(id) } else { None })
            .collect()
    }

    #[test]
    fn every_kind_answers_once_and_keeps_the_rules() {
        for kind in PromptKind::ALL {
            let (mut h, tab, browser) = tab_with_browser();
            let log = Log::default();
            let scope = scope_now(browser);

            let id = admit(scope, request(kind, tab), callback(&log)).expect("admitted");
            assert_eq!(admitted(&mut h), [id], "{kind:?}: the shell sees it");
            assert_eq!(answer(id, wrong_answer_for(kind)), Err(PromptError::WrongKind), "{kind:?}");
            assert_eq!(answer(id, answer_for(kind)), Ok(()), "{kind:?}: the right answer still works");
            assert_eq!(answer(id, answer_for(kind)), Err(PromptError::AlreadyEnded), "{kind:?}: duplicate");
            assert!(!end(id, PromptEnd::Navigated), "{kind:?}: answer then cancel finds nothing");
            assert_eq!(*log.borrow(), [format!("answer {kind:?}")], "{kind:?}");

            let id = admit(scope, request(kind, tab), callback(&log)).expect("admitted");
            assert!(end(id, PromptEnd::Navigated));
            assert_eq!(answer(id, answer_for(kind)), Err(PromptError::AlreadyEnded), "{kind:?}: cancel then answer");
            assert_eq!(log.borrow().last().map(String::as_str), Some("cancel"), "{kind:?}: the cancel action ran");
            assert!(h.events().contains(&BrowserEvent::PromptEnded { id, why: PromptEnd::Navigated }));

            assert_eq!(answer(PromptId(9_999_999), answer_for(kind)), Err(PromptError::Unknown), "{kind:?}");
        }
    }

    #[test]
    fn a_dismissed_menu_drops_its_callback_and_refuses_a_late_answer() {
        let (mut h, tab, browser) = tab_with_browser();
        let log = Log::default();
        let id = admit(scope_now(browser), request(PromptKind::ContextMenu, tab), callback(&log)).expect("admitted");
        assert!(end(id, PromptEnd::MenuDismissed));
        assert_eq!(*log.borrow(), ["dropped"], "CEF gave the callback up; it is not called");
        assert_eq!(answer(id, PromptAnswer::Menu(None)), Err(PromptError::AlreadyEnded));
        assert!(h.events().contains(&BrowserEvent::PromptEnded { id, why: PromptEnd::MenuDismissed }));
    }

    #[test]
    fn an_io_prompt_is_refused_after_close_navigation_or_shutdown() {
        let (mut h, tab, browser) = tab_with_browser();
        let log = Log::default();
        let scope = scope_now(browser);
        crate::handlers::life_span::before_close(browser);
        assert_eq!(admit(scope, request(PromptKind::Auth, tab), callback(&log)), Err(Refused::BrowserGone));
        assert_eq!(*log.borrow(), ["cancel"]);
        assert!(admitted(&mut h).is_empty(), "never shown after the tab closed");

        let (mut h, tab, browser) = tab_with_browser();
        let log = Log::default();
        let scope = scope_now(browser);
        new_document(browser);
        assert_eq!(admit(scope, request(PromptKind::Auth, tab), callback(&log)), Err(Refused::NewDocument));
        assert_eq!(*log.borrow(), ["cancel"]);
        assert!(admitted(&mut h).is_empty(), "never shown after a new document");

        let (mut h, tab, browser) = tab_with_browser();
        let log = Log::default();
        let scope = scope_now(browser);
        crate::handlers::life_span::begin_shutdown();
        assert_eq!(admit(scope, request(PromptKind::Auth, tab), callback(&log)), Err(Refused::ShuttingDown));
        assert_eq!(*log.borrow(), ["cancel"]);
        assert!(admitted(&mut h).is_empty(), "never shown after begin_shutdown");
    }

    #[test]
    fn closing_navigating_and_shutdown_end_open_prompts() {
        let (mut h, tab, browser) = tab_with_browser();
        let log = Log::default();
        let a = admit(scope_now(browser), request(PromptKind::JsDialog, tab), callback(&log)).expect("admitted");
        new_document(browser);
        let b = admit(scope_now(browser), request(PromptKind::Permission, tab), callback(&log)).expect("admitted");
        crate::handlers::life_span::begin_shutdown();
        let events = h.events();
        assert!(events.contains(&BrowserEvent::PromptEnded { id: a, why: PromptEnd::Navigated }));
        assert!(events.contains(&BrowserEvent::PromptEnded { id: b, why: PromptEnd::Shutdown }));
        assert_eq!(*log.borrow(), ["cancel", "cancel"]);

        let (mut h, tab, browser) = tab_with_browser();
        let log = Log::default();
        let c = admit(scope_now(browser), request(PromptKind::FileChooser, tab), callback(&log)).expect("admitted");
        crate::handlers::life_span::close_tab(tab, vsesvit_engine::CloseMode::Force);
        assert!(h.events().contains(&BrowserEvent::PromptEnded { id: c, why: PromptEnd::TabClosed }));
        assert_eq!(*log.borrow(), ["cancel"]);
        let refused = admit(scope_now(browser), request(PromptKind::FileChooser, tab), callback(&log));
        assert_eq!(refused, Err(Refused::BrowserClosing), "a closing browser admits nothing");
    }
}
