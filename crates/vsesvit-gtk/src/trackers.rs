//! Tracking protection on Linux: core's tracker list (`vsesvit_core::trackers`) at the level
//! the user chose, minus the sites they turned it off for, translated into a WebKit content
//! blocker by the same code as extensions' declarativeNetRequest rules and attached to every
//! tab's `UserContentManager` beside theirs. Also the switch for it in the site-info popover.
//!
//! WebKit reports nothing about what a content blocker stopped, so the popover gives no count.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::Url;
use vsesvit_core::permissions::Origin;
use vsesvit_core::prefs::keys;
use vsesvit_core::trackers::{self, TrackerList, TrackingProtection};
use vsesvit_webext::dnr;

use crate::browser::Browser;
use crate::profile::Core;
use crate::tab::Tab;

/// The blocker's name in its store and in each manager, apart from every extension's.
const IDENTIFIER: &str = "vsesvit-tracking-protection";

/// Cheap to clone; every clone is the same state.
#[derive(Clone)]
pub(crate) struct Trackers(Rc<Inner>);

struct Inner {
    core: Core,
    store: webkit::UserContentFilterStore,
    list: RefCell<Cow<'static, TrackerList>>,
    /// Every tab's manager. A closed tab's goes away with its view.
    managers: RefCell<Vec<glib::WeakRef<webkit::UserContentManager>>>,
    attached: RefCell<Option<webkit::UserContentFilter>>,
    /// Counts [`Trackers::apply`] calls; a compile that a later call overtook is dropped.
    generation: Cell<u64>,
    /// The apply whose compile is still running, if any.
    compiling: Cell<Option<u64>>,
    waiters: RefCell<Vec<Box<dyn FnOnce()>>>,
}

impl Trackers {
    /// Compiled blockers are kept in the profile's `trackers` folder, beside the extensions'
    /// in `webext`.
    pub(crate) fn new(core: Core) -> Trackers {
        let dir = core.borrow().paths().root.join("trackers");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("{}: {e}", dir.display());
        }
        Trackers(Rc::new(Inner {
            core,
            store: webkit::UserContentFilterStore::new(&dir.to_string_lossy()),
            list: RefCell::new(Cow::Borrowed(TrackerList::bundled())),
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

    /// Brings every tab's blocker in line with the profile's level and exceptions. Compiling is
    /// asynchronous; [`Trackers::when_applied`] runs once the result is on the tabs.
    pub(crate) fn apply(&self) {
        let blocker = {
            let mut profile = self.0.core.borrow_mut();
            let level = profile.prefs().get(&keys::TRACKING_PROTECTION);
            let allowed = trackers::allowed_sites(&mut profile);
            content_blocker(&self.0.list.borrow(), level, &allowed)
        };
        let generation = self.0.generation.get() + 1;
        self.0.generation.set(generation);
        let Some(json) = blocker else {
            self.0.compiling.set(None);
            self.0.swap(None);
            self.0.settle();
            return;
        };
        self.0.compiling.set(Some(generation));
        let weak = Rc::downgrade(&self.0);
        let bytes = glib::Bytes::from_owned(json.into_bytes());
        self.0.store.save(IDENTIFIER, &bytes, None::<&gio::Cancellable>, move |result| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.compiling.get() != Some(generation) {
                return;
            }
            inner.compiling.set(None);
            match result {
                Ok(filter) => {
                    inner.swap(Some(filter));
                    log::info!("tracking protection: content blocker attached");
                }
                Err(e) => log::warn!("tracking protection: the content blocker did not compile: {e}"),
            }
            inner.settle();
        });
    }

    /// Runs `f` once the latest [`Trackers::apply`] is on every tab (at once when it is).
    pub(crate) fn when_applied(&self, f: impl FnOnce() + 'static) {
        if self.0.compiling.get().is_none() {
            f();
        } else {
            self.0.waiters.borrow_mut().push(Box::new(f));
        }
    }

    /// Blocks with `list` instead of the bundled one, as the self-test does to make a host it
    /// serves a tracker.
    #[cfg(feature = "self-test")]
    pub(crate) fn use_list(&self, list: Cow<'static, TrackerList>) {
        *self.0.list.borrow_mut() = list;
        self.apply();
    }
}

impl Inner {
    fn swap(&self, filter: Option<webkit::UserContentFilter>) {
        let old = self.attached.replace(filter.clone());
        let mut managers = self.managers.borrow_mut();
        managers.retain(|m| m.upgrade().is_some());
        for manager in managers.iter().filter_map(glib::WeakRef::upgrade) {
            if let Some(old) = &old {
                manager.remove_filter(old);
            }
            if let Some(filter) = &filter {
                manager.add_filter(filter);
            }
        }
    }

    fn settle(&self) {
        for waiter in std::mem::take(&mut *self.waiters.borrow_mut()) {
            waiter();
        }
    }
}

/// WebKit's content-blocker JSON for `list` at `level`, with pages of the `allowed` sites
/// exempt. `None` when it blocks nothing.
fn content_blocker(list: &TrackerList, level: TrackingProtection, allowed: &[Origin]) -> Option<String> {
    let (rules, malformed) = dnr::parse_rules(&list.dnr_rules(level, allowed)).expect("core writes the rules as an array");
    let translation = dnr::translate(&rules, "", &dnr::Grants { hosts: dnr::HostScope::All, host_access_only: false });
    let skipped: Vec<dnr::Skipped> = malformed.into_iter().chain(translation.skipped.iter().cloned()).collect();
    if !skipped.is_empty() {
        log::warn!("tracking protection: rules left out: {}", dnr::describe_skipped(&skipped));
    }
    (!translation.is_empty()).then(|| translation.to_json())
}

/// The site-info popover's switch for `tab`'s site, on while trackers are blocked there.
/// `None` for a page that is not from a website, or while tracking protection is off.
pub(crate) fn site_info_section(browser: &Browser, tab: &Tab) -> Option<gtk::ListBox> {
    let url = tab.committed_uri().and_then(|uri| Url::parse(&uri).ok())?;
    let origin = Origin::of(&url).filter(|_| matches!(url.scheme(), "http" | "https"))?;
    let (level, on) = {
        let mut profile = browser.core().borrow_mut();
        (profile.prefs().get(&keys::TRACKING_PROTECTION), !trackers::allowed(&mut profile, &origin))
    };
    if level == TrackingProtection::Off {
        return None;
    }
    let row = adw::SwitchRow::builder().title(trackers::TITLE).subtitle(trackers::site_status(on, None)).active(on).build();
    row.connect_active_notify(glib::clone!(
        #[strong]
        browser,
        #[weak]
        tab,
        move |row| {
            let on = row.is_active();
            row.set_subtitle(&trackers::site_status(on, None));
            if let Err(e) = trackers::set_allowed(&mut browser.core().borrow_mut(), &origin, !on) {
                log::warn!("tracking protection: {e}");
                return;
            }
            browser.trackers().apply();
            browser.trackers().when_applied(glib::clone!(
                #[weak]
                tab,
                move || tab.reload()
            ));
        }
    ));
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    list.append(&row);
    Some(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vsesvit_core::trackers::Category;

    #[test]
    fn off_compiles_no_blocker() {
        let list = TrackerList::bundled().clone().with_tracker("localhost", Category::Analytics);
        let allowed = [Origin::parse("http://127.0.0.1:8080").unwrap()];
        assert_eq!(content_blocker(&list, TrackingProtection::Off, &allowed), None);
        let standard = content_blocker(&list, TrackingProtection::Standard, &allowed).expect("Standard blocks");
        assert!(standard.contains("localhost"), "{standard}");
    }
}
