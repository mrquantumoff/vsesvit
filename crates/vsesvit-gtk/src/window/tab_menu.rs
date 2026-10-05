//! A tab's context menu, with Chrome's items, the same on the top tab bar and in the tab list.
//! `AdwTabView` holds the menu: the tab bar opens it on a right click after `setup-menu`
//! names the page, and the list does the same for its rows. The menu is rebuilt for each
//! page, so its labels follow the tab (Pin or Unpin, to the Right or Below), and its actions
//! (`tab.*`) act on the page it was set up for.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::history::Transition;
use webkit::prelude::*;

use super::{BrowserWindow, Layout};
use crate::tab::Tab;

/// What a tab's menu does to its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabAction {
    NewTabNext,
    MoveToNewWindow,
    Reload,
    Duplicate,
    Pin(bool),
    Mute(bool),
    /// Copies the tab's address without its tracking parameters.
    CopyLink,
    Close,
    CloseOthers,
    CloseAfter,
    ReopenClosed,
}

impl TabAction {
    const ALL: [Self; 13] = [
        Self::NewTabNext,
        Self::MoveToNewWindow,
        Self::Reload,
        Self::Duplicate,
        Self::Pin(true),
        Self::Pin(false),
        Self::Mute(true),
        Self::Mute(false),
        Self::CopyLink,
        Self::Close,
        Self::CloseOthers,
        Self::CloseAfter,
        Self::ReopenClosed,
    ];

    /// Its action in the window's `tab` group.
    fn name(self) -> &'static str {
        match self {
            Self::NewTabNext => "new-tab-next",
            Self::MoveToNewWindow => "move-to-new-window",
            Self::Reload => "reload",
            Self::Duplicate => "duplicate",
            Self::Pin(true) => "pin",
            Self::Pin(false) => "unpin",
            Self::Mute(true) => "mute",
            Self::Mute(false) => "unmute",
            Self::CopyLink => "copy-link",
            Self::Close => "close",
            Self::CloseOthers => "close-others",
            Self::CloseAfter => "close-after",
            Self::ReopenClosed => "reopen-closed",
        }
    }
}

/// Where a tab is in its window: its index, how many tabs there are, and how many of them
/// are pinned, which lead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    index: u32,
    count: u32,
    pinned: u32,
}

impl Place {
    fn is_pinned(self) -> bool {
        self.index < self.pinned
    }

    /// The indices of the other tabs `action` closes, last first. As in Chrome, Close Other
    /// Tabs and Close Tabs to the Right leave pinned tabs open.
    fn closes(self, action: TabAction) -> Vec<u32> {
        let range = match action {
            TabAction::CloseOthers => self.pinned..self.count,
            TabAction::CloseAfter => (self.index + 1).max(self.pinned)..self.count,
            _ => return Vec::new(),
        };
        range.rev().filter(|&i| i != self.index).collect()
    }
}

/// What the menu depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabFacts {
    place: Place,
    /// The tabs are a list in the sidebar, where Chrome's "to the right" reads "below".
    vertical: bool,
    muted: bool,
    /// The tab shows a page with an address worth copying.
    has_link: bool,
    can_reopen: bool,
}

/// A line of the menu: its label, what it does, and whether it can be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item(&'static str, TabAction, bool);

/// The menu's sections, in Chrome's order.
fn sections(facts: TabFacts) -> [Vec<Item>; 3] {
    use TabAction::*;
    let place = facts.place;
    let (new_tab, close_after) = if facts.vertical {
        ("_New Tab Below", "Close Tabs _Below")
    } else {
        ("_New Tab to the Right", "Close Tabs to the Ri_ght")
    };
    let pin = if place.is_pinned() {
        Item("Unp_in Tab", Pin(false), true)
    } else {
        Item("P_in Tab", Pin(true), true)
    };
    let mute = if facts.muted {
        Item("Un_mute Tab", Mute(false), true)
    } else {
        Item("_Mute Tab", Mute(true), true)
    };
    let mut page = vec![Item("_Reload", Reload, true), Item("_Duplicate", Duplicate, true), pin, mute];
    if facts.has_link {
        page.push(Item("Copy _Link", CopyLink, true));
    }
    [
        vec![Item(new_tab, NewTabNext, true), Item("Move Tab to New _Window", MoveToNewWindow, place.count > 1)],
        page,
        vec![
            Item("_Close Tab", Close, true),
            Item("Close _Other Tabs", CloseOthers, !place.closes(CloseOthers).is_empty()),
            Item(close_after, CloseAfter, !place.closes(CloseAfter).is_empty()),
            Item("R_eopen Closed Tab", ReopenClosed, facts.can_reopen),
        ],
    ]
}

/// The menu `AdwTabView` shows, its actions, and the page it was last set up for.
#[derive(Default)]
pub(super) struct TabMenu {
    menu: gio::Menu,
    actions: gio::SimpleActionGroup,
    page: glib::WeakRef<adw::TabPage>,
}

pub(super) fn install(window: &BrowserWindow) {
    let state = &window.imp().tab_menu;
    for action in TabAction::ALL {
        let entry = gio::SimpleAction::new(action.name(), None);
        entry.connect_activate(glib::clone!(
            #[weak]
            window,
            move |_, _| {
                if let Some(page) = window.imp().tab_menu.page.upgrade() {
                    window.tab_action(&page, action);
                }
            }
        ));
        state.actions.add_action(&entry);
    }
    window.insert_action_group("tab", Some(&state.actions));
    let view = &window.ui().tab_view;
    view.set_menu_model(Some(&state.menu));
    view.connect_setup_menu(glib::clone!(
        #[weak]
        window,
        move |_, page| window.setup_tab_menu(page)
    ));
}

impl BrowserWindow {
    fn setup_tab_menu(&self, page: Option<&adw::TabPage>) {
        let state = &self.imp().tab_menu;
        state.page.set(page);
        let Some((page, tab)) = page.and_then(|page| Some((page, page.child().downcast::<Tab>().ok()?))) else {
            return;
        };
        let view = &self.ui().tab_view;
        let facts = TabFacts {
            place: place_of(view, page),
            vertical: matches!(self.imp().layout.get(), Some(Layout::Sidebar(_))),
            muted: tab.web_view().is_muted(),
            has_link: tab.link().is_some(),
            can_reopen: self.browser().can_reopen_closed_tab(),
        };
        let sections = sections(facts);
        state.menu.remove_all();
        for section in &sections {
            let part = gio::Menu::new();
            for &Item(label, action, enabled) in section {
                if let Some(entry) = state.actions.lookup_action(action.name()).and_downcast::<gio::SimpleAction>() {
                    entry.set_enabled(enabled);
                }
                part.append(Some(label), Some(&format!("tab.{}", action.name())));
            }
            state.menu.append_section(None, &part);
        }
    }

    fn tab_action(&self, page: &adw::TabPage, action: TabAction) {
        let Ok(tab) = page.child().downcast::<Tab>() else { return };
        let view = &self.ui().tab_view;
        let place = place_of(view, page);
        match action {
            TabAction::NewTabNext => {
                let new = Tab::new(self.browser());
                let at = (place.index + 1).max(place.pinned);
                view.set_selected_page(&view.insert(&new, to_i32(at)));
                self.load_new_tab_page(&new);
            }
            TabAction::MoveToNewWindow => {
                let target = BrowserWindow::new(self.browser());
                let target_view = &target.ui().tab_view;
                view.transfer_page(page, target_view, 0);
                target_view.set_selected_page(page);
                target.present();
            }
            TabAction::Reload => {
                tab.set_pending_transition(Transition::Reload);
                tab.reload();
            }
            TabAction::Duplicate => {
                let copy = Tab::new(self.browser());
                let at = to_i32(place.index + 1);
                let added = if place.is_pinned() { view.insert_pinned(&copy, at) } else { view.insert(&copy, at) };
                view.set_selected_page(&added);
                match tab.session_uri() {
                    Some(uri) => copy.restore(tab.web_view().session_state().as_ref(), &uri),
                    None => self.load_new_tab_page(&copy),
                }
            }
            TabAction::Pin(pinned) => {
                view.set_page_pinned(page, pinned);
                self.browser().schedule_session_save();
            }
            TabAction::Mute(muted) => tab.web_view().set_is_muted(muted),
            TabAction::CopyLink => self.copy_link(&tab, true),
            TabAction::Close => view.close_page(page),
            TabAction::CloseOthers | TabAction::CloseAfter => {
                for index in place.closes(action) {
                    view.close_page(&view.nth_page(to_i32(index)));
                }
            }
            TabAction::ReopenClosed => self.browser().reopen_closed_tab(self),
        }
    }

    /// Opens `tab`'s menu from its row in the tab list, as a right click does.
    #[cfg(feature = "self-test")]
    pub(crate) fn open_tab_menu(&self, tab: &Tab) -> Option<gtk::PopoverMenu> {
        self.ui().tab_list.open_menu(&self.page_of(tab)?)
    }

    /// The tab menu as last set up, by section: each line's label and whether it can be chosen.
    #[cfg(feature = "self-test")]
    pub(crate) fn tab_menu_lines(&self) -> Vec<Vec<(String, bool)>> {
        let state = &self.imp().tab_menu;
        let line = |part: &gio::MenuModel, i: i32| {
            let label = part.item_attribute_value(i, "label", None)?.get::<String>()?;
            let action = part.item_attribute_value(i, "action", None)?.get::<String>()?;
            Some((label, state.actions.is_action_enabled(action.strip_prefix("tab.")?)))
        };
        let menu = state.menu.upcast_ref::<gio::MenuModel>();
        (0..menu.n_items())
            .filter_map(|s| menu.item_link(s, "section"))
            .map(|part| (0..part.n_items()).filter_map(|i| line(&part, i)).collect())
            .collect()
    }
}

fn place_of(view: &adw::TabView, page: &adw::TabPage) -> Place {
    let count = |n: i32| u32::try_from(n).unwrap_or(0);
    Place {
        index: count(view.page_position(page)),
        count: count(view.n_pages()),
        pinned: count(view.n_pinned_pages()),
    }
}

fn to_i32(index: u32) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(index: u32, count: u32, pinned: u32) -> TabFacts {
        TabFacts {
            place: Place { index, count, pinned },
            vertical: false,
            muted: false,
            has_link: true,
            can_reopen: true,
        }
    }

    fn items(facts: TabFacts) -> Vec<Item> {
        sections(facts).into_iter().flatten().collect()
    }

    fn enabled(facts: TabFacts, action: TabAction) -> Option<bool> {
        items(facts).into_iter().find(|i| i.1 == action).map(|i| i.2)
    }

    #[test]
    fn the_menu_has_chromes_items_in_chromes_order() {
        let labels: Vec<Vec<&str>> = sections(facts(1, 3, 0)).iter().map(|s| s.iter().map(|i| i.0).collect()).collect();
        assert_eq!(
            labels,
            [
                vec!["_New Tab to the Right", "Move Tab to New _Window"],
                vec!["_Reload", "_Duplicate", "P_in Tab", "_Mute Tab", "Copy _Link"],
                vec!["_Close Tab", "Close _Other Tabs", "Close Tabs to the Ri_ght", "R_eopen Closed Tab"],
            ]
        );
    }

    #[test]
    fn a_sidebar_says_below_instead_of_to_the_right() {
        let vertical = TabFacts { vertical: true, ..facts(0, 2, 0) };
        let labels: Vec<&str> = items(vertical).iter().map(|i| i.0).collect();
        assert!(labels.contains(&"_New Tab Below") && labels.contains(&"Close Tabs _Below"), "{labels:?}");
    }

    #[test]
    fn toggles_offer_the_opposite_state() {
        let plain = items(facts(1, 2, 0));
        assert!(plain.iter().any(|i| i.1 == TabAction::Pin(true)) && plain.iter().any(|i| i.1 == TabAction::Mute(true)));
        let set = items(TabFacts { muted: true, ..facts(0, 2, 1) });
        assert!(set.iter().any(|i| i.1 == TabAction::Pin(false)) && set.iter().any(|i| i.1 == TabAction::Mute(false)));
    }

    #[test]
    fn copy_link_only_for_pages() {
        assert_eq!(enabled(TabFacts { has_link: false, ..facts(0, 1, 0) }, TabAction::CopyLink), None);
        assert_eq!(enabled(facts(0, 1, 0), TabAction::CopyLink), Some(true));
    }

    #[test]
    fn closing_others_and_after_leaves_pinned_tabs() {
        let place = |index, count, pinned| Place { index, count, pinned };
        assert_eq!(place(1, 4, 0).closes(TabAction::CloseOthers), [3, 2, 0]);
        assert_eq!(place(2, 4, 2).closes(TabAction::CloseOthers), [3]);
        assert_eq!(place(0, 4, 2).closes(TabAction::CloseOthers), [3, 2]);
        assert_eq!(place(1, 4, 0).closes(TabAction::CloseAfter), [3, 2]);
        assert_eq!(place(0, 4, 2).closes(TabAction::CloseAfter), [3, 2]);
        assert_eq!(place(3, 4, 0).closes(TabAction::CloseAfter), Vec::<u32>::new());
    }

    #[test]
    fn items_that_would_do_nothing_are_off() {
        let alone = facts(0, 1, 0);
        assert_eq!(enabled(alone, TabAction::MoveToNewWindow), Some(false));
        assert_eq!(enabled(alone, TabAction::CloseOthers), Some(false));
        assert_eq!(enabled(alone, TabAction::CloseAfter), Some(false));
        assert_eq!(enabled(TabFacts { can_reopen: false, ..alone }, TabAction::ReopenClosed), Some(false));
        let all_pinned = facts(1, 2, 2);
        assert_eq!(enabled(all_pinned, TabAction::CloseOthers), Some(false));
        assert_eq!(enabled(facts(0, 2, 0), TabAction::CloseAfter), Some(true));
    }
}
