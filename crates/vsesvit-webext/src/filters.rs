//! Compiling an extension's content-blocker JSON with `UserContentFilterStore` and
//! attaching the result to every tab. Compilation is asynchronous; `Runtime::on_filters_ready`
//! reports when nothing is pending.

use std::rc::Rc;

use webkit::{gio, glib};

use crate::extension::Extension;
use crate::runtime::Inner;

pub(crate) fn compile(inner: &Rc<Inner>, ext: &Rc<Extension>) {
    let Some(json) = &ext.dnr_json else { return };
    inner.pending_filters.set(inner.pending_filters.get() + 1);
    let weak_inner = Rc::downgrade(inner);
    let ext = ext.clone();
    let identifier = ext.host.clone();
    let bytes = glib::Bytes::from(json.as_bytes());
    inner.filter_store.save(&identifier, &bytes, None::<&gio::Cancellable>, move |result| {
        let Some(inner) = weak_inner.upgrade() else { return };
        match result {
            Ok(filter) => {
                let still_loaded = inner.extensions.borrow().get(&ext.id).is_some_and(|e| Rc::ptr_eq(e, &ext));
                if still_loaded {
                    for ucm in inner.tab_managers() {
                        ucm.add_filter(&filter);
                    }
                    *ext.filter.borrow_mut() = Some(filter);
                    log::info!("{}: declarativeNetRequest content blocker attached", ext.id.as_str());
                }
            }
            Err(e) => log::warn!("{}: content blocker compile failed: {e}", ext.id.as_str()),
        }
        inner.filter_finished();
    });
}

impl Inner {
    pub(crate) fn filter_finished(&self) {
        let left = self.pending_filters.get().saturating_sub(1);
        self.pending_filters.set(left);
        if left == 0 {
            for waiter in std::mem::take(&mut *self.filters_waiters.borrow_mut()) {
                waiter();
            }
        }
    }
}
