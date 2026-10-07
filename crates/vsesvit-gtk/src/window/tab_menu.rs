//! A tab's context menu, with Chrome's items, the same on the top tab bar and in the tab list.
//! `AdwTabView` holds the menu: the tab bar opens it on a right click after `setup-menu`
//! names the page, and the list does the same for its rows. The menu is rebuilt for each
//! page, so its labels follow the tab (Pin or Unpin, to the Right or Below, the window's tab
//! groups), and its actions (`tab.*`) act on the page it was set up for.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::history::Transition;
use vsesvit_core::tab_groups::{GroupId, TabGroup};
use vsesvit_core::tab_place::TabPlace;
use webkit::prelude::*;

use super::{BrowserWindow, Layout, groups};
use crate::tab::Tab;

/// What a tab's menu does to its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabAction {
    NewTabNext,
    NewGroup,
    /// A submenu, whose entries name the group.
    AddToGroup,
    RemoveFromGroup,
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
    const ALL: [Self; 16] = [
        Self::NewTabNext,
        Self::NewGroup,
        Self::AddToGroup,
        Self::RemoveFromGroup,
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
            Self::NewGroup => "add-to-new-group",
            Self::AddToGroup => "add-to-group",
            Self::RemoveFromGroup => "remove-from-group",
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

/// What the menu depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabFacts {
    place: TabPlace,
    /// The tabs are a list in the sidebar, where Chrome's "to the right" reads "below".
    vertical: bool,
    muted: bool,
    /// The tab shows a page with an address worth copying.
    has_link: bool,
    can_reopen: bool,
    grouped: bool,
    /// The window has a group the tab is not in.
    other_groups: bool,
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
    let mut tabs = vec![Item(new_tab, NewTabNext, true)];
    tabs.push(if facts.other_groups {
        Item("_Add Tab to Group", AddToGroup, true)
    } else {
        Item("_Add Tab to New Group", NewGroup, true)
    });
    if facts.grouped {
        tabs.push(Item("Remove _From Group", RemoveFromGroup, true));
    }
    tabs.push(Item("Move Tab to New _Window", MoveToNewWindow, place.can_move_out()));
    [
        tabs,
        page,
        vec![
            Item("_Close Tab", Close, true),
            Item("Close _Other Tabs", CloseOthers, !place.closes_others().is_empty()),
            Item(close_after, CloseAfter, !place.closes_after().is_empty()),
            Item("R_eopen Closed Tab", ReopenClosed, facts.can_reopen),
        ],
    ]
}

/// "Add Tab to Group"'s submenu: a new group, then the window's other groups by name.
fn group_choices(others: &[TabGroup]) -> Vec<(String, Option<GroupId>)> {
    let named = others.iter().map(|group| (group.name().replace('_', "__"), Some(group.id)));
    std::iter::once(("_New Group".to_owned(), None)).chain(named).collect()
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
        let parameter = (action == TabAction::AddToGroup).then_some(glib::VariantTy::STRING);
        let entry = gio::SimpleAction::new(action.name(), parameter);
        entry.connect_activate(glib::clone!(
            #[weak]
            window,
            move |_, target| {
                let Some(page) = window.imp().tab_menu.page.upgrade() else { return };
                match groups::parse_target(target) {
                    Some(group) => window.add_page_to_group(&page, group),
                    None => window.tab_action(&page, action),
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
        let (own, others) = self.groups_for_menu(&tab);
        let facts = TabFacts {
            place: place_of(view, page),
            vertical: matches!(self.imp().layout.get(), Some(Layout::Sidebar(_))),
            muted: tab.web_view().is_muted(),
            has_link: tab.link().is_some(),
            can_reopen: self.browser().can_reopen_closed_tab(self.browsing()),
            grouped: own.is_some(),
            other_groups: !others.is_empty(),
        };
        let sections = sections(facts);
        state.menu.remove_all();
        for section in &sections {
            let part = gio::Menu::new();
            for &Item(label, action, enabled) in section {
                if let Some(entry) = state.actions.lookup_action(action.name()).and_downcast::<gio::SimpleAction>() {
                    entry.set_enabled(enabled);
                }
                if action == TabAction::AddToGroup {
                    part.append_submenu(Some(label), &group_submenu(&others));
                } else {
                    part.append(Some(label), Some(&format!("tab.{}", action.name())));
                }
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
                let new = Tab::new(self.browser(), self.browsing());
                self.adopt_into_group(&new, &tab);
                let at = (place.index + 1).max(place.pinned);
                view.set_selected_page(&view.insert(&new, to_i32(at)));
                self.load_new_tab_page(&new);
            }
            TabAction::NewGroup => self.group_tab(&tab),
            // Its submenu's entries carry the group, which `add_page_to_group` takes.
            TabAction::AddToGroup => {}
            TabAction::RemoveFromGroup => self.remove_tab_from_group(&tab),
            TabAction::MoveToNewWindow => {
                let target = BrowserWindow::with_browsing(self.browser(), self.browsing());
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
                let copy = Tab::new(self.browser(), self.browsing());
                self.adopt_into_group(&copy, &tab);
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
                let closes = if action == TabAction::CloseOthers { place.closes_others() } else { place.closes_after() };
                for index in closes {
                    view.close_page(&view.nth_page(to_i32(index)));
                }
            }
            TabAction::ReopenClosed => self.browser().reopen_closed_tab(self),
        }
    }

    fn add_page_to_group(&self, page: &adw::TabPage, group: GroupId) {
        if let Ok(tab) = page.child().downcast::<Tab>() {
            self.add_tab_to_group(&tab, group);
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
            let enabled = match part.item_attribute_value(i, "action", None).and_then(|a| a.get::<String>()) {
                Some(action) => state.actions.is_action_enabled(action.strip_prefix("tab.")?),
                None => part.item_link(i, "submenu").is_some(),
            };
            Some((label, enabled))
        };
        let menu = state.menu.upcast_ref::<gio::MenuModel>();
        (0..menu.n_items())
            .filter_map(|s| menu.item_link(s, "section"))
            .map(|part| (0..part.n_items()).filter_map(|i| line(&part, i)).collect())
            .collect()
    }
}

fn group_submenu(others: &[TabGroup]) -> gio::Menu {
    let menu = gio::Menu::new();
    for (label, group) in group_choices(others) {
        let item = gio::MenuItem::new(Some(&label), None);
        match group {
            Some(group) => item.set_action_and_target_value(Some("tab.add-to-group"), Some(&groups::target(group))),
            None => item.set_detailed_action("tab.add-to-new-group"),
        }
        menu.append_item(&item);
    }
    menu
}

fn place_of(view: &adw::TabView, page: &adw::TabPage) -> TabPlace {
    let count = |n: i32| usize::try_from(n).unwrap_or(0);
    TabPlace {
        index: count(view.page_position(page)),
        count: count(view.n_pages()),
        pinned: count(view.n_pinned_pages()),
    }
}

fn to_i32(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::browser;
    use crate::window::Focus;
    use vsesvit_core::private::Browsing;

    fn facts(index: usize, count: usize, pinned: usize) -> TabFacts {
        TabFacts {
            place: TabPlace { index, count, pinned },
            vertical: false,
            muted: false,
            has_link: true,
            can_reopen: true,
            grouped: false,
            other_groups: false,
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
                vec!["_New Tab to the Right", "_Add Tab to New Group", "Move Tab to New _Window"],
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

    #[test]
    fn a_tab_is_added_to_a_new_group_until_the_window_has_another() {
        let labels = |facts: TabFacts| sections(facts)[0].iter().map(|i| i.0).collect::<Vec<_>>();
        assert_eq!(labels(facts(0, 2, 0))[1], "_Add Tab to New Group");
        let grouped = TabFacts { grouped: true, ..facts(0, 2, 0) };
        assert_eq!(labels(grouped)[1..3], ["_Add Tab to New Group", "Remove _From Group"]);
        let others = TabFacts { other_groups: true, ..facts(0, 2, 1) };
        assert_eq!(labels(others), ["_New Tab to the Right", "_Add Tab to Group", "Move Tab to New _Window"], "a pinned tab too");
        assert_eq!(enabled(others, TabAction::AddToGroup), Some(true));
    }

    #[test]
    fn the_group_submenu_offers_a_new_group_then_names_the_others() {
        use vsesvit_core::tab_groups::GroupColor;

        let group = |title: &str, color| TabGroup { id: GroupId::new(), title: title.into(), color, collapsed: false };
        let (work, untitled) = (group("Work_2", GroupColor::Blue), group("", GroupColor::Red));
        let choices = group_choices(&[work.clone(), untitled.clone()]);
        assert_eq!(
            choices,
            [("_New Group".to_owned(), None), ("Work__2".to_owned(), Some(work.id)), ("Red group".to_owned(), Some(untitled.id))]
        );
    }

    #[gtk::test]
    fn the_menu_groups_a_tab_and_adds_another_to_its_group() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        let (a, b, c) = (
            window.open_tab(None, None, Focus::Foreground),
            window.open_tab(None, None, Focus::Foreground),
            window.open_tab(None, None, Focus::Foreground),
        );
        let page = |tab: &Tab| window.page_of(tab).expect("the tab's page");
        window.tab_action(&page(&a), TabAction::NewGroup);
        window.close_group_editor();
        let group = window.group_of(&a).expect("the tab is in a new group");
        window.setup_tab_menu(Some(&page(&c)));
        let offered = window.groups_for_menu(&c).1;
        window.add_page_to_group(&page(&c), group.id);
        let joined = (window.tabs(), window.group_of(&c).map(|g| g.id));
        let opened = {
            window.tab_action(&page(&c), TabAction::NewTabNext);
            window.selected_tab().and_then(|tab| window.group_of(&tab)).map(|g| g.id)
        };
        window.tab_action(&page(&a), TabAction::RemoveFromGroup);
        let left = window.group_of(&a);
        window.destroy();
        assert_eq!(offered, std::slice::from_ref(&group));
        assert_eq!(joined, (vec![a.clone(), c.clone(), b.clone()], Some(group.id)), "it joins the end of the group");
        assert_eq!(opened, Some(group.id), "New Tab to the Right from a grouped tab joins the group");
        assert_eq!(left, None);
    }

    #[gtk::test]
    fn moving_a_private_tab_to_a_new_window_keeps_it_private() {
        let browser = browser();
        let private = BrowserWindow::with_browsing(&browser, Browsing::Private);
        private.open_tab(None, None, Focus::Foreground);
        let moved = private.open_tab(None, None, Focus::Foreground);
        let page = private.page_of(&moved).expect("the tab's page");
        private.tab_action(&page, TabAction::MoveToNewWindow);
        let target = moved.window().expect("the tab is in a window");
        let kind = target.browsing();
        let title = target.title();
        target.destroy();
        private.destroy();
        assert_eq!(kind, Browsing::Private);
        assert_eq!(title.as_deref(), Some("New Tab (Private)"));
    }
}
