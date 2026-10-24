//! Owned by W1.3.
//! Request contexts per `Browsing` (design section 3.2): the normal context is CEF's global one, whose
//! cache is `<root_cache_path>/Default`; the private context is created with an empty `cache_path`
//! (in memory) at the first private tab and dropped once the shell ended the private session and its
//! last browser closed.

use std::cell::RefCell;

use cef::{CefString, RequestContext, RequestContextSettings, request_context_create_context};
use vsesvit_engine::Browsing;

use crate::registry;

/// The contexts the glue holds. Main thread, inside `client::CefBackend`.
#[derive(Default)]
pub struct Contexts {
    private: RefCell<Option<RequestContext>>,
}

impl Contexts {
    /// The context a browser of `browsing` is created in; `None` is CEF's global context. Creates the
    /// private context if the shell did not ask for it first.
    pub fn for_browsing(&self, browsing: Browsing) -> Option<RequestContext> {
        match browsing {
            Browsing::Normal => None,
            Browsing::Private => {
                if self.private.borrow().is_none() {
                    self.set_private(true);
                }
                self.private.borrow().clone()
            }
        }
    }

    /// Creates or drops the private context.
    pub fn set_private(&self, on: bool) {
        if !on {
            self.private.borrow_mut().take();
            return;
        }
        if self.private.borrow().is_some() {
            return;
        }
        let settings = RequestContextSettings { cache_path: CefString::from(""), ..Default::default() };
        let context = request_context_create_context(Some(&settings), None);
        if context.is_none() {
            log::error!("vsesvit-cef: CEF refused to create the private request context");
        }
        *self.private.borrow_mut() = context;
    }
}

/// `Engine::begin_private_context`.
pub fn begin_private() {
    let backend = registry::with(|r| {
        r.private_ending = false;
        r.private_open = true;
        r.backend.clone()
    });
    if let Some(backend) = backend {
        backend.set_private_context(true);
    }
}

/// `Engine::end_private_context`: now if no private browser is left, else after the last one closes.
pub fn end_private() {
    registry::with(|r| r.private_ending = true);
    drop_private_if_done();
}

/// Drops the private context once the session ended and no private browser is alive or in flight.
pub fn drop_private_if_done() {
    let backend = registry::with(|r| {
        if r.private_open && r.private_ending && !r.private_alive() {
            r.private_open = false;
            r.private_ending = false;
            Some(r.backend.clone())
        } else {
            None
        }
    })
    .flatten();
    if let Some(backend) = backend {
        backend.set_private_context(false);
    }
}
