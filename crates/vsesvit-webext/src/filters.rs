//! Each extension's declarativeNetRequest rules as one WebKit content blocker on every tab:
//! its enabled static rulesets with its dynamic and session rules, since WebKit lets an
//! allow rule lift only the blocks of its own filter. The rulesets are read and translated
//! on a worker thread and `UserContentFilterStore` compiles in the background, so a change
//! takes a while to reach the tabs: [`when_compiled`] runs once it has. Changes that come in
//! meanwhile are compiled together once the running compile ends.
//!
//! The blocker is named by the extension's URL host, apart from the shell's own blockers
//! (tracking protection, cookie rules), which share each tab's `UserContentManager`.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use webkit::{gio, glib};

use crate::dnr::{self, Grants, Rule};
use crate::extension::{Extension, Waiting};
use crate::runtime::Inner;

/// Where an extension's content blocker is relative to its rules.
#[derive(Default)]
pub(crate) struct Compiles {
    /// Counts the changes to the rules.
    wanted: Cell<u64>,
    /// The change the attached blocker reflects.
    applied: Cell<u64>,
    running: Cell<bool>,
    waiters: RefCell<Vec<(u64, Waiting)>>,
    /// How many rules the enabled static rulesets hold, as of the last compile.
    pub static_rules: Cell<usize>,
}

/// Puts `ext`'s rules as they are now on the tabs, unless it has no declarativeNetRequest
/// permission. Returns the change to wait for with [`when_compiled`].
pub(crate) fn compile(inner: &Rc<Inner>, ext: &Rc<Extension>) -> u64 {
    let compiles = &ext.compiles;
    compiles.wanted.set(compiles.wanted.get() + 1);
    if ext.grants.is_some() && !compiles.running.get() {
        start(inner, ext);
    }
    compiles.wanted.get()
}

/// Runs `f` once the content blocker reflects change `wanted` (at once when it does).
pub(crate) fn when_compiled(ext: &Extension, wanted: u64, f: impl FnOnce() + 'static) {
    if ext.grants.is_none() || ext.compiles.applied.get() >= wanted {
        f();
    } else {
        ext.compiles.waiters.borrow_mut().push((wanted, Box::new(f)));
    }
}

/// What a worker needs to build the content-blocker JSON.
struct Input {
    name: String,
    base: String,
    grants: Grants,
    rulesets: Vec<PathBuf>,
    added: Vec<Rule>,
}

struct Built {
    json: Option<String>,
    static_rules: usize,
}

fn start(inner: &Rc<Inner>, ext: &Rc<Extension>) {
    let Some(grants) = ext.grants.clone() else { return };
    ext.compiles.running.set(true);
    inner.pending_filters.set(inner.pending_filters.get() + 1);
    let change = ext.compiles.wanted.get();
    let input = {
        let rules = ext.dnr.borrow();
        let enabled = rules.enabled();
        let rulesets = ext.manifest.dnr_rulesets.iter().filter(|r| enabled.contains(&r.id)).map(|r| r.path.resolve(&ext.dir)).collect();
        Input { name: ext.manifest.name.clone(), base: ext.base_url.trim_end_matches('/').to_owned(), grants, rulesets, added: rules.added() }
    };
    let (weak_inner, ext) = (Rc::downgrade(inner), ext.clone());
    glib::spawn_future_local(async move {
        let built = gio::spawn_blocking(move || build(input)).await;
        let Some(inner) = weak_inner.upgrade() else { return };
        match built {
            Ok(built) => attach(&inner, &ext, change, built),
            Err(_) => {
                log::warn!("{}: building the content blocker panicked", ext.id.as_str());
                finished(&inner, &ext, change, None);
            }
        }
    });
}

/// Reads the enabled rulesets and translates them with the added rules. A ruleset that cannot
/// be read or parsed is left out, like a rule WebKit cannot express, and both are logged.
fn build(input: Input) -> Built {
    let mut rules = Vec::new();
    for path in &input.rulesets {
        let parsed = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|text| dnr::parse_rules(&text).map_err(|e| e.to_string()));
        match parsed {
            Ok((parsed, malformed)) => {
                if !malformed.is_empty() {
                    log::warn!("{}: skipped malformed rules: {}", path.display(), dnr::describe_skipped(&malformed));
                }
                rules.extend(parsed);
            }
            Err(e) => log::warn!("{}: ruleset left out: {e}", path.display()),
        }
    }
    let static_rules = rules.len();
    rules.extend(input.added);
    let translation = dnr::translate(&rules, &input.base, &input.grants);
    if !translation.skipped.is_empty() {
        log::warn!("{}: declarativeNetRequest rules WebKit cannot express: {}", input.name, dnr::describe_skipped(&translation.skipped));
    }
    Built { json: (!translation.is_empty()).then(|| translation.to_json()), static_rules }
}

fn attach(inner: &Rc<Inner>, ext: &Rc<Extension>, change: u64, built: Built) {
    ext.compiles.static_rules.set(built.static_rules);
    let Some(json) = built.json else {
        finished(inner, ext, change, Some(None));
        return;
    };
    let (weak_inner, ext, identifier) = (Rc::downgrade(inner), ext.clone(), ext.host.clone());
    inner.filter_store.save(&identifier, &glib::Bytes::from_owned(json.into_bytes()), None::<&gio::Cancellable>, move |result| {
        let Some(inner) = weak_inner.upgrade() else { return };
        match result {
            Ok(filter) => finished(&inner, &ext, change, Some(Some(filter))),
            Err(e) => {
                log::warn!("{}: content blocker compile failed: {e}", ext.id.as_str());
                finished(&inner, &ext, change, None);
            }
        }
    });
}

/// The compile for `change` ended with `filter` to put on the tabs (`Some(None)`: no blocker),
/// or `None` when it failed, which leaves the last one in place.
fn finished(inner: &Rc<Inner>, ext: &Rc<Extension>, change: u64, filter: Option<Option<webkit::UserContentFilter>>) {
    let compiles = &ext.compiles;
    let loaded = inner.extension(&ext.id).is_some_and(|e| Rc::ptr_eq(&e, ext));
    if loaded && let Some(filter) = filter {
        let old = ext.filter.replace(filter.clone());
        for ucm in inner.tab_managers() {
            // By identifier, which the new blocker shares: off first, then on.
            if let Some(old) = &old {
                ucm.remove_filter(old);
            }
            if let Some(filter) = &filter {
                ucm.add_filter(filter);
            }
        }
        log::info!("{}: declarativeNetRequest content blocker {}", ext.id.as_str(), if filter.is_some() { "attached" } else { "removed" });
    }
    compiles.applied.set(change);
    let ready: Vec<Waiting> = {
        let mut waiters = compiles.waiters.borrow_mut();
        let (ready, waiting) = std::mem::take(&mut *waiters).into_iter().partition(|(wanted, _)| !loaded || *wanted <= change);
        *waiters = waiting;
        ready.into_iter().map(|(_, f)| f).collect()
    };
    if loaded && compiles.wanted.get() > change {
        start(inner, ext);
    } else {
        compiles.running.set(false);
    }
    inner.filter_finished();
    for f in ready {
        f();
    }
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
