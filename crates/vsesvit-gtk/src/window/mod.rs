//! A browser window: the header bar with navigation buttons, the address bar, the star and
//! the extension actions; the tabs as a vertical list in an `AdwOverlaySplitView` sidebar
//! (left or right) or as an `AdwTabBar` on top; the bookmarks bar, the find bar, and the
//! tab view holding one web view per tab.

mod actions;
mod ext_actions;
mod layout;
mod menu;
mod tab_list;

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::history::Transition;
use vsesvit_core::new_tab;
use vsesvit_core::prefs::TabsPosition;
use webkit::prelude::*;

use crate::address_bar::AddressBar;
use crate::bookmarks_bar::BookmarksBar;
use crate::browser::{Browser, ClosedTab};
use crate::find_bar::FindBar;
use crate::session;
use crate::tab::{Tab, TabChange};
use crate::updates::Banner;
use crate::zoom;
use ext_actions::ExtensionActions;
use layout::{Layout, Rect};
use tab_list::TabList;

pub(crate) use layout::{LayoutProbe, classify as classify_layout};

/// Whether a newly opened tab is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Focus {
    Foreground,
    Background,
}

struct Ui {
    toolbar: adw::ToolbarView,
    header: adw::HeaderBar,
    sidebar_toggle: gtk::ToggleButton,
    address: AddressBar,
    reload: gtk::Button,
    zoom_level: gtk::Button,
    /// Hidden until a download starts.
    downloads_button: gtk::Button,
    tab_view: adw::TabView,
    tab_bar: adw::TabBar,
    tab_list: TabList,
    split: adw::OverlaySplitView,
    bookmarks_bar: BookmarksBar,
    find_bar: FindBar,
    toasts: adw::ToastOverlay,
    extension_actions: Rc<ExtensionActions>,
    update_banner: adw::Banner,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BrowserWindow {
        pub(super) browser: OnceCell<Browser>,
        pub(super) ui: OnceCell<Ui>,
        pub(super) id: Cell<u32>,
        pub(super) layout: Cell<Option<Layout>>,
        /// Views created for `window.open` that WebKit has not declared ready to show.
        pub(super) popups: RefCell<Vec<Tab>>,
        /// The tab the header currently reflects, to save its unsubmitted address text on switch.
        pub(super) chrome_tab: glib::WeakRef<Tab>,
        pub(super) closing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BrowserWindow {
        const NAME: &'static str = "VsesvitBrowserWindow";
        type Type = super::BrowserWindow;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for BrowserWindow {}
    impl WidgetImpl for BrowserWindow {}
    impl WindowImpl for BrowserWindow {
        fn close_request(&self) -> glib::Propagation {
            self.closing.set(true);
            self.obj().before_close();
            self.parent_close_request()
        }
    }
    impl ApplicationWindowImpl for BrowserWindow {}
    impl AdwApplicationWindowImpl for BrowserWindow {}
}

glib::wrapper! {
    pub struct BrowserWindow(ObjectSubclass<imp::BrowserWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl BrowserWindow {
    /// An empty window laid out per the profile's preferences. Callers add tabs.
    pub(crate) fn new(browser: &Browser) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", browser.app())
            .build();
        let imp = window.imp();
        assert!(
            imp.browser.set(browser.clone()).is_ok(),
            "new runs once per window"
        );
        imp.id.set(browser.allocate_window_id());
        window.set_default_size(1280, 820);
        window.set_title(Some("Vsesvit"));
        let ui = window.build_ui();
        assert!(imp.ui.set(ui).is_ok(), "new runs once per window");
        actions::install(&window);
        window.connect_signals();
        window.apply_layout(browser.tabs_position());
        window.set_bookmarks_bar_visible(browser.bookmarks_bar_visible());
        window.refresh_bookmarks_bar();
        window.refresh_extension_actions();
        if browser.downloads().started_this_session() {
            window.show_downloads_button();
        }
        if let Some(updates) = browser.updates() {
            updates.window_opened(&window);
        }
        window
    }

    pub(crate) fn browser(&self) -> &Browser {
        self.imp().browser.get().expect("set in BrowserWindow::new")
    }

    fn ui(&self) -> &Ui {
        self.imp().ui.get().expect("set in BrowserWindow::new")
    }

    /// The id `chrome.windows` sees.
    pub(crate) fn id(&self) -> u32 {
        self.imp().id.get()
    }

    pub(crate) fn address_bar(&self) -> &AddressBar {
        &self.ui().address
    }

    pub(crate) fn bookmarks_bar(&self) -> &BookmarksBar {
        &self.ui().bookmarks_bar
    }

    pub(crate) fn toast(&self, toast: adw::Toast) {
        self.ui().toasts.add_toast(toast);
    }

    pub(crate) fn show_downloads_button(&self) {
        self.ui().downloads_button.set_visible(true);
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn shows_downloads_button(&self) -> bool {
        self.ui().downloads_button.is_visible()
    }

    pub(crate) fn set_update_banner(&self, banner: Option<&Banner>) {
        let widget = &self.ui().update_banner;
        if let Some(banner) = banner {
            widget.set_title(&banner.title);
            widget.set_button_label(banner.button);
        }
        widget.set_revealed(banner.is_some());
    }

    fn build_ui(&self) -> Ui {
        let tab_view = adw::TabView::new();
        let tab_bar = adw::TabBar::builder()
            .view(&tab_view)
            .autohide(false)
            .build();
        let tab_list = TabList::new(&tab_view);

        let address = AddressBar::new();
        let star = gtk::ToggleButton::builder()
            .icon_name("non-starred-symbolic")
            .action_name("win.bookmark-page")
            .tooltip_text("Bookmark This Page")
            .build();
        star.connect_active_notify(|star| {
            star.set_icon_name(if star.is_active() {
                "starred-symbolic"
            } else {
                "non-starred-symbolic"
            });
        });
        let location = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        location.append(&address);
        location.append(&star);
        let title = adw::Clamp::builder()
            .maximum_size(860)
            .tightening_threshold(600)
            .hexpand(true)
            .child(&location)
            .build();

        let sidebar_toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .tooltip_text("Show Tabs")
            .build();
        let reload = icon_button("view-refresh-symbolic", "win.reload", "Reload");
        let extension_actions = ExtensionActions::new();
        let extensions_area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        extensions_area.append(extension_actions.widget());
        extensions_area.append(&icon_button(
            "application-x-addon-symbolic",
            "win.show-extensions",
            "Extensions",
        ));
        let (menu_button, zoom_level) = menu::main_menu();
        let downloads_button = icon_button("folder-download-symbolic", "win.show-downloads", "Downloads");
        downloads_button.set_visible(false);

        let header = adw::HeaderBar::new();
        header.pack_start(&sidebar_toggle);
        header.pack_start(&icon_button("go-previous-symbolic", "win.back", "Back"));
        header.pack_start(&icon_button("go-next-symbolic", "win.forward", "Forward"));
        header.pack_start(&reload);
        header.set_title_widget(Some(&title));
        header.pack_end(&menu_button);
        header.pack_end(&downloads_button);
        header.pack_end(&icon_button("tab-new-symbolic", "win.new-tab", "New Tab"));
        header.pack_end(&extensions_area);

        let update_banner = adw::Banner::builder().action_name("app.update").build();
        let bookmarks_bar = BookmarksBar::new();
        let find_bar = FindBar::new();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&tab_view));

        let split = adw::OverlaySplitView::builder()
            .sidebar(tab_list.widget())
            .content(&toasts)
            .min_sidebar_width(180.0)
            .max_sidebar_width(300.0)
            .sidebar_width_fraction(0.2)
            .build();
        split
            .bind_property("show-sidebar", &sidebar_toggle, "active")
            .bidirectional()
            .sync_create()
            .build();

        let toolbar = adw::ToolbarView::new();
        toolbar.set_top_bar_style(adw::ToolbarStyle::Raised);
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&update_banner);
        toolbar.add_top_bar(&tab_bar);
        toolbar.add_top_bar(bookmarks_bar.widget());
        toolbar.add_top_bar(find_bar.widget());
        toolbar.set_content(Some(&split));
        self.set_content(Some(&toolbar));

        // On narrow windows the tab sidebar becomes an overlay the toggle button opens.
        let narrow = adw::BreakpointCondition::parse("max-width: 720sp")
            .expect("a valid breakpoint condition");
        let breakpoint = adw::Breakpoint::new(narrow);
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        self.add_breakpoint(breakpoint);

        Ui {
            toolbar,
            header,
            sidebar_toggle,
            address,
            reload,
            zoom_level,
            downloads_button,
            tab_view,
            tab_bar,
            tab_list,
            split,
            bookmarks_bar,
            find_bar,
            toasts,
            extension_actions,
            update_banner,
        }
    }

    fn connect_signals(&self) {
        let ui = self.ui();
        let view = &ui.tab_view;
        view.connect_selected_page_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.selection_changed()
        ));
        view.connect_page_attached(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, page, _| {
                window.sync_page(page);
                window.browser().schedule_session_save();
            }
        ));
        view.connect_page_reordered(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _| window.browser().schedule_session_save()
        ));
        view.connect_close_page(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |view, page| {
                if let Ok(tab) = page.child().downcast::<Tab>() {
                    window.browser().tab_closed(&tab, view.page_position(page));
                }
                view.close_page_finish(page, true);
                glib::Propagation::Stop
            }
        ));
        view.connect_page_detached(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _| {
                window.browser().schedule_session_save();
                window.close_if_empty();
            }
        ));
        view.connect_is_transferring_page_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.close_if_empty()
        ));
        view.connect_create_window(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            None,
            move |_| {
                let target = BrowserWindow::new(window.browser());
                target.present();
                Some(target.ui().tab_view.clone())
            }
        ));

        ui.split.connect_collapsed_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |split| {
                let vertical = matches!(window.imp().layout.get(), Some(Layout::Sidebar(_)));
                split.set_show_sidebar(vertical && !split.is_collapsed());
            }
        ));

        ui.address.connect_edited(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, text| window.browser().omnibox_changed(&window, text)
        ));
        ui.address.connect_submitted(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, text| window.browser().omnibox_activated(&window, text)
        ));
        ui.address.connect_cancelled(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.focus_page()
        ));

        self.connect_fullscreened_notify(|window| {
            window
                .ui()
                .toolbar
                .set_reveal_top_bars(!window.is_fullscreen());
        });
    }

    /// The last window saves the session while its tabs still exist; every tab that goes
    /// with the window leaves the extension runtime.
    fn before_close(&self) {
        let browser = self.browser();
        if browser.windows().len() <= 1 {
            browser.save_session_now();
        }
        for tab in self.tabs() {
            browser.tab_discarded(&tab);
        }
    }

    // Layout.

    /// Places the tabs per the preference, live: the sidebar moves or the tab bar appears
    /// without recreating anything.
    pub(crate) fn apply_layout(&self, position: TabsPosition) {
        let ui = self.ui();
        let layout = Layout::for_position(position);
        self.imp().layout.set(Some(layout));
        match layout {
            Layout::Sidebar(pack) => {
                ui.tab_bar.set_visible(false);
                ui.split.set_sidebar_position(pack);
                ui.header.remove(&ui.sidebar_toggle);
                match pack {
                    gtk::PackType::Start => ui.header.pack_start(&ui.sidebar_toggle),
                    gtk::PackType::End => ui.header.pack_end(&ui.sidebar_toggle),
                    _ => ui.header.pack_start(&ui.sidebar_toggle),
                }
                ui.sidebar_toggle.set_visible(true);
                ui.split.set_show_sidebar(!ui.split.is_collapsed());
            }
            Layout::TopBar => {
                ui.split.set_show_sidebar(false);
                ui.sidebar_toggle.set_visible(false);
                ui.tab_bar.set_visible(true);
            }
        }
    }

    fn toggle_tab_sidebar(&self) {
        if matches!(self.imp().layout.get(), Some(Layout::Sidebar(_))) {
            let split = &self.ui().split;
            split.set_show_sidebar(!split.shows_sidebar());
        }
    }

    /// Where the tab widgets and the selected web view are, in window coordinates.
    pub(crate) fn layout_probe(&self) -> LayoutProbe {
        let ui = self.ui();
        let bounds = |widget: &gtk::Widget| -> Option<Rect> {
            if !widget.is_visible() || !widget.is_mapped() {
                return None;
            }
            let rect = widget.compute_bounds(self.upcast_ref::<gtk::Widget>())?;
            Some(Rect {
                x: rect.x(),
                y: rect.y(),
                width: rect.width(),
                height: rect.height(),
            })
        };
        let sidebar = ui
            .split
            .shows_sidebar()
            .then(|| bounds(ui.tab_list.widget().upcast_ref()))
            .flatten();
        LayoutProbe {
            width: self.width() as f32,
            sidebar,
            tab_bar: bounds(ui.tab_bar.upcast_ref()),
            web_view: self
                .selected_tab()
                .and_then(|tab| bounds(tab.web_view().upcast_ref())),
        }
    }

    pub(crate) fn set_bookmarks_bar_visible(&self, shown: bool) {
        if let Some(action) = self
            .lookup_action("show-bookmarks-bar")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_state(&shown.to_variant());
        }
        self.ui().bookmarks_bar.set_revealed(shown);
    }

    pub(crate) fn refresh_bookmarks_bar(&self) {
        self.ui().bookmarks_bar.refresh(self.browser().core());
    }

    /// The star follows the selected tab's committed URL.
    pub(crate) fn sync_star(&self) {
        let starred = self
            .selected_tab()
            .is_some_and(|tab| self.browser().is_bookmarked(tab.committed_uri().as_deref()));
        if let Some(action) = self
            .lookup_action("bookmark-page")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_state(&starred.to_variant());
        }
    }

    // Extension actions.

    pub(crate) fn refresh_extension_actions(&self) {
        let actions = self.browser().runtime().actions();
        self.ui().extension_actions.rebuild(
            &actions,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |id| window.activate_extension_action(id)
            ),
        );
    }

    /// What clicking an extension's toolbar button does: a popup page in a popover, or the
    /// extension's `action.onClicked`.
    pub(crate) fn activate_extension_action(&self, id: &ExtensionId) {
        let tab = self.selected_tab().map(|tab| tab.id());
        if let Some(view) = self.browser().runtime().activate_action(id, tab) {
            self.ui().extension_actions.show_popup(id, view);
        }
    }

    pub(crate) fn extension_action_button(&self, id: &ExtensionId) -> Option<gtk::Button> {
        self.ui().extension_actions.button_for(id)
    }

    /// The popup page currently shown, if any.
    pub(crate) fn extension_popup_view(&self) -> Option<webkit::WebView> {
        self.ui().extension_actions.popup_view()
    }

    pub(crate) fn close_extension_popup(&self) {
        self.ui().extension_actions.close_popup();
    }

    // Tabs.

    pub(crate) fn tabs(&self) -> Vec<Tab> {
        let view = &self.ui().tab_view;
        (0..view.n_pages())
            .filter_map(|i| view.nth_page(i).child().downcast().ok())
            .collect()
    }

    pub(crate) fn selected_tab(&self) -> Option<Tab> {
        self.ui()
            .tab_view
            .selected_page()
            .and_then(|page| page.child().downcast().ok())
    }

    pub(crate) fn select_tab(&self, tab: &Tab) {
        if let Some(page) = self.page_of(tab) {
            self.ui().tab_view.set_selected_page(&page);
        }
    }

    /// Opens a tab, next to `opener` if given, else at the end.
    pub(crate) fn open_tab(&self, uri: Option<&str>, opener: Option<&Tab>, focus: Focus) -> Tab {
        let tab = Tab::new(self.browser());
        if let Some(uri) = uri {
            tab.load(uri);
        }
        self.insert_tab(&tab, opener, focus);
        tab
    }

    /// A new tab page at the end; selecting it puts the focus in the address bar. The page
    /// has no base URI, so it is at `about:blank` and the tab still reads as blank.
    pub(crate) fn new_tab(&self) {
        let tab = self.open_tab(None, None, Focus::Foreground);
        match new_tab::page(&mut self.browser().core().borrow_mut()) {
            Ok(html) => tab.web_view().load_html(&html, None),
            Err(e) => log::warn!("new tab page: {e}"),
        }
    }

    pub(crate) fn close_tab(&self, tab: &Tab) {
        if let Some(page) = self.page_of(tab) {
            self.ui().tab_view.close_page(&page);
        }
    }

    pub(crate) fn close_selected_tab(&self) {
        if let Some(page) = self.ui().tab_view.selected_page() {
            self.ui().tab_view.close_page(&page);
        }
    }

    pub(crate) fn restore_closed(&self, closed: &ClosedTab) {
        let tab = Tab::new(self.browser());
        let view = &self.ui().tab_view;
        let page = view.insert(&tab, closed.position.clamp(0, view.n_pages()));
        view.set_selected_page(&page);
        tab.restore(closed.state.as_ref(), &closed.uri);
    }

    /// Loads `uri` in the selected tab, opening one if the window has none, and records
    /// how the page was reached.
    pub(crate) fn navigate_with(&self, uri: &str, transition: Transition) {
        let tab = match self.selected_tab() {
            Some(tab) => tab,
            None => self.open_tab(None, None, Focus::Foreground),
        };
        tab.set_pending_transition(transition);
        tab.load(uri);
        tab.web_view().grab_focus();
    }

    /// Keeps a `window.open` view until WebKit says it is ready to be shown.
    pub(crate) fn adopt_popup(&self, opener: &Tab, popup: &Tab) {
        self.imp().popups.borrow_mut().push(popup.clone());
        let opener = opener.downgrade();
        popup.web_view().connect_ready_to_show(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            popup,
            move |_| {
                if window.forget_popup(&popup) {
                    window.insert_tab(&popup, opener.upgrade().as_ref(), Focus::Foreground);
                }
            }
        ));
        popup.web_view().connect_close(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            popup,
            move |_| {
                window.forget_popup(&popup);
            }
        ));
    }

    fn forget_popup(&self, popup: &Tab) -> bool {
        let mut popups = self.imp().popups.borrow_mut();
        let before = popups.len();
        popups.retain(|p| p != popup);
        popups.len() != before
    }

    fn insert_tab(&self, tab: &Tab, opener: Option<&Tab>, focus: Focus) {
        let view = &self.ui().tab_view;
        let parent = opener.and_then(|opener| self.page_of(opener));
        let page = view.add_page(tab, parent.as_ref());
        if focus == Focus::Foreground {
            view.set_selected_page(&page);
        }
    }

    fn page_of(&self, tab: &Tab) -> Option<adw::TabPage> {
        let view = &self.ui().tab_view;
        (0..view.n_pages())
            .map(|i| view.nth_page(i))
            .find(|page| page.child() == *tab.upcast_ref::<gtk::Widget>())
    }

    fn close_if_empty(&self) {
        let view = &self.ui().tab_view;
        if view.n_pages() == 0 && !view.is_transferring_page() && !self.imp().closing.get() {
            self.close();
        }
    }

    pub(crate) fn focus_page(&self) {
        if let Some(tab) = self.selected_tab() {
            tab.web_view().grab_focus();
        }
    }

    // Keeping the header in step with the selected tab.

    /// Called by a tab of this window whenever something about it changes.
    pub(crate) fn tab_changed(&self, tab: &Tab, change: TabChange) {
        let Some(page) = self.page_of(tab) else {
            return;
        };
        let selected = page.is_selected();
        match change {
            TabChange::Title => {
                page.set_title(&tab.display_title());
                if selected {
                    self.set_title(Some(&tab.display_title()));
                }
                self.browser().title_changed(tab);
            }
            TabChange::Favicon => {
                page.set_icon(tab.web_view().favicon().as_ref());
                self.browser().favicon_changed(tab);
            }
            TabChange::Loading => {
                let loading = tab.web_view().is_loading();
                page.set_loading(loading);
                if selected {
                    self.sync_loading(tab);
                    // A load that ends without committing (a download, a stop) leaves the
                    // submitted text behind; show the page that is actually there again.
                    if !loading {
                        self.sync_location(tab);
                    }
                }
            }
            TabChange::Progress if selected => self.sync_loading(tab),
            TabChange::Committed(commit) => {
                page.set_title(&tab.display_title());
                if selected {
                    self.sync_location(tab);
                    self.sync_history(tab);
                }
                if let Some(uri) = tab.committed_uri() {
                    self.browser().navigation_committed(tab, &uri, commit);
                }
                if selected {
                    self.sync_star();
                }
            }
            TabChange::History if selected => self.sync_history(tab),
            TabChange::Zoom if selected => self.sync_zoom(tab),
            TabChange::Find(result) if selected => self.ui().find_bar.show_result(result),
            TabChange::Progress | TabChange::History | TabChange::Zoom | TabChange::Find(_) => {}
        }
    }

    fn selection_changed(&self) {
        let imp = self.imp();
        let ui = self.ui();
        if let Some(previous) = imp.chrome_tab.upgrade() {
            previous.set_typed(ui.address.take_edit());
        }
        let Some(tab) = self.selected_tab() else {
            imp.chrome_tab.set(None);
            return;
        };
        imp.chrome_tab.set(Some(&tab));
        let typed = tab.take_typed();
        let editing = typed.is_some();
        ui.address.restore(typed, tab.committed_uri().as_deref());
        ui.find_bar.retarget(tab.web_view());
        self.set_title(Some(&tab.display_title()));
        self.sync_location(&tab);
        self.sync_history(&tab);
        self.sync_loading(&tab);
        self.sync_zoom(&tab);
        self.sync_star();
        tab.mark_active(session::now_ms());
        self.browser().tab_activated(&tab);
        if editing || tab.is_blank() {
            ui.address.focus_for_typing();
        } else {
            tab.web_view().grab_focus();
        }
    }

    fn sync_page(&self, page: &adw::TabPage) {
        let Ok(tab) = page.child().downcast::<Tab>() else {
            return;
        };
        page.set_title(&tab.display_title());
        page.set_icon(tab.web_view().favicon().as_ref());
        page.set_loading(tab.web_view().is_loading());
    }

    fn sync_location(&self, tab: &Tab) {
        let address = &self.ui().address;
        address.show_uri(tab.committed_uri().as_deref());
        address.set_security(tab.security());
    }

    fn sync_history(&self, tab: &Tab) {
        let web_view = tab.web_view();
        self.set_action_enabled("back", web_view.can_go_back());
        self.set_action_enabled("forward", web_view.can_go_forward());
    }

    fn sync_loading(&self, tab: &Tab) {
        let web_view = tab.web_view();
        let loading = web_view.is_loading();
        let reload = &self.ui().reload;
        if loading {
            reload.set_icon_name("process-stop-symbolic");
            reload.set_action_name(Some("win.stop"));
            reload.set_tooltip_text(Some("Stop"));
        } else {
            reload.set_icon_name("view-refresh-symbolic");
            reload.set_action_name(Some("win.reload"));
            reload.set_tooltip_text(Some("Reload"));
        }
        self.ui().address.set_progress(if loading {
            web_view.estimated_load_progress()
        } else {
            0.0
        });
    }

    fn sync_zoom(&self, tab: &Tab) {
        self.ui()
            .zoom_level
            .set_label(&zoom::percent(tab.web_view().zoom_level()));
    }

    fn set_action_enabled(&self, name: &str, enabled: bool) {
        if let Some(action) = self.lookup_action(name).and_downcast::<gio::SimpleAction>() {
            action.set_enabled(enabled);
        }
    }
}

fn icon_button(icon: &str, action: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .action_name(action)
        .tooltip_text(tooltip)
        .build()
}
