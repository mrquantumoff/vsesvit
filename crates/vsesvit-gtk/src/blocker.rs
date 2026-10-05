//! A WebKit content blocker kept on every tab's `UserContentManager`: compiled into a store of
//! its own, attached to each new tab, and swapped on every tab when it changes. Tracking
//! protection and cookie rules each keep one, beside the extensions' blockers.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

/// Cheap to clone; every clone is the same state.
#[derive(Clone)]
pub(crate) struct Blocker(Rc<Inner>);

struct Inner {
    /// The blocker's name in its store and in each manager, apart from every extension's.
    identifier: &'static str,
    store: webkit::UserContentFilterStore,
    /// Every tab's manager. A closed tab's goes away with its view.
    managers: RefCell<Vec<glib::WeakRef<webkit::UserContentManager>>>,
    attached: RefCell<Option<webkit::UserContentFilter>>,
    /// Counts [`Blocker::apply`] calls; a compile that a later call overtook is dropped.
    generation: Cell<u64>,
    /// The apply whose compile is still running, if any.
    compiling: Cell<Option<u64>>,
    waiters: RefCell<Vec<Box<dyn FnOnce()>>>,
}

impl Blocker {
    /// Compiled blockers are kept in `dir`.
    pub(crate) fn new(dir: &Path, identifier: &'static str) -> Blocker {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("{}: {e}", dir.display());
        }
        Blocker(Rc::new(Inner {
            identifier,
            store: webkit::UserContentFilterStore::new(&dir.to_string_lossy()),
            managers: RefCell::new(Vec::new()),
            attached: RefCell::new(None),
            generation: Cell::new(0),
            compiling: Cell::new(None),
            waiters: RefCell::new(Vec::new()),
        }))
    }

    /// A new tab's manager: it gets the blocker now and every one after it.
    pub(crate) fn attach(&self, manager: &webkit::UserContentManager) {
        if let Some(filter) = self.0.attached.borrow().as_ref() {
            manager.add_filter(filter);
        }
        self.0.managers.borrow_mut().push(manager.downgrade());
    }

    /// The managers of the tabs still open.
    pub(crate) fn managers(&self) -> Vec<webkit::UserContentManager> {
        let mut managers = self.0.managers.borrow_mut();
        managers.retain(|m| m.upgrade().is_some());
        managers.iter().filter_map(glib::WeakRef::upgrade).collect()
    }

    /// Puts the blocker `json` describes on every tab, or takes it off for `None`. Compiling is
    /// asynchronous; [`Blocker::when_applied`] runs once the result is on the tabs.
    pub(crate) fn apply(&self, json: Option<String>) {
        let generation = self.0.generation.get() + 1;
        self.0.generation.set(generation);
        let Some(json) = json else {
            self.0.compiling.set(None);
            self.swap(None);
            self.0.settle();
            return;
        };
        self.0.compiling.set(Some(generation));
        let weak = Rc::downgrade(&self.0);
        let bytes = glib::Bytes::from_owned(json.into_bytes());
        self.0.store.save(self.0.identifier, &bytes, None::<&gio::Cancellable>, move |result| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.compiling.get() != Some(generation) {
                return;
            }
            inner.compiling.set(None);
            let blocker = Blocker(inner);
            match result {
                Ok(filter) => {
                    blocker.swap(Some(filter));
                    log::info!("{}: content blocker attached", blocker.0.identifier);
                }
                Err(e) => log::warn!("{}: the content blocker did not compile: {e}", blocker.0.identifier),
            }
            blocker.0.settle();
        });
    }

    /// Runs `f` once the latest [`Blocker::apply`] is on every tab (at once when it is).
    pub(crate) fn when_applied(&self, f: impl FnOnce() + 'static) {
        if self.0.compiling.get().is_none() {
            f();
        } else {
            self.0.waiters.borrow_mut().push(Box::new(f));
        }
    }

    fn swap(&self, filter: Option<webkit::UserContentFilter>) {
        let old = self.0.attached.replace(filter.clone());
        for manager in self.managers() {
            if let Some(old) = &old {
                manager.remove_filter(old);
            }
            if let Some(filter) = &filter {
                manager.add_filter(filter);
            }
        }
    }
}

impl Inner {
    fn settle(&self) {
        for waiter in std::mem::take(&mut *self.waiters.borrow_mut()) {
            waiter();
        }
    }
}
