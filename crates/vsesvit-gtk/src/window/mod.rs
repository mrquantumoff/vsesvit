//! A browser window: the header bar with the navigation buttons at the start, the address
//! bar in the middle, and the extension actions and the menu at the end; the tabs as a
//! vertical list in an `AdwOverlaySplitView` sidebar (left or right) or as an `AdwTabBar` on
//! top; the bookmarks bar, the find bar, and the tab view holding one web view per tab.

mod actions;
mod ext_actions;
mod layout;
mod menu;
mod tab_list;

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::bookmarks::BookmarkNode;
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::history::Transition;
use vsesvit_core::new_tab;
use vsesvit_core::permissions::{Answer, Permission};
use vsesvit_core::prefs::TabsPosition;
use webkit::prelude::*;

use crate::address_bar::{AddressBar, Anchor};
use crate::bookmark_editor;
use crate::bookmarks_bar::BookmarksBar;
use crate::browser::{Browser, ClosedTab};
use crate::find_bar::FindBar;
use crate::permissions;
use crate::session;
use crate::site_info;
use crate::tab::{Tab, TabChange};
use crate::updates::Banner;
use crate::zoom;
use ext_actions::ExtensionActions;
use layout::{Layout, Rect};
use tab_list::TabList;

pub(crate) use layout::{LayoutProbe, classify as classify_layout};

/// The compact address bar's widest.
const COMPACT_ADDRESS_WIDTH: i32 = 720;

/// How long the notice naming a page that went full screen stays.
const FULLSCREEN_NOTICE_TIME: Duration = Duration::from_secs(4);

/// Whether a newly opened tab is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Focus {
    Foreground,
    Background,
}

struct Ui {
    toolbar: adw::ToolbarView,
    /// The header's buttons before and after the address bar. The sidebar toggle moves
    /// between them: after the navigation buttons, or last, next to the window controls.
    header_start: gtk::Box,
    header_end: gtk::Box,
    sidebar_toggle: gtk::ToggleButton,
    /// Holds the address bar; narrow and centered when the bar is compact.
    location: adw::Clamp,
    address: AddressBar,
    reload: gtk::Button,
    /// Shown only with the `toolbar.home_button` preference.
    home: gtk::Button,
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
    /// Over the page for a few seconds after it goes full screen.
    fullscreen_notice: gtk::Revealer,
    fullscreen_text: gtk::Label,
    extension_actions: Rc<ExtensionActions>,
    update_banner: adw::Banner,
}

/// The permission prompt on screen, for the selected tab's oldest waiting request.
struct ShownPrompt {
    popover: gtk::Popover,
    tab: glib::WeakRef<Tab>,
    request: webkit::PermissionRequest,
    asked: Vec<Permission>,
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
        pub(super) prompt: RefCell<Option<ShownPrompt>>,
        pub(super) fullscreen_notice_timeout: RefCell<Option<glib::SourceId>>,
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
        window.set_home_button_visible(browser.home_button_visible());
        window.set_compact_address_bar(browser.compact_address_bar());
        window.set_full_urls(browser.full_urls());
        window.refresh_bookmarks_bar();
        window.refresh_extension_actions();
        if browser.downloads().started_this_session() {
            window.show_downloads_button();
        }
        if let Some(updates) = browser.updates() {
            updates.window_opened(&window);
        }
        browser.sync().window_opened();
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
        let title = adw::Clamp::builder().hexpand(true).child(&address).build();

        let sidebar_toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .tooltip_text("Show Tabs")
            .build();
        let reload = icon_button("view-refresh-symbolic", "win.reload", "Reload");
        let home = icon_button("go-home-symbolic", "win.home", "Home");
        let extension_actions = ExtensionActions::new();
        let (menu_button, zoom_level) = menu::main_menu();
        let downloads_button = icon_button("folder-download-symbolic", "win.show-downloads", "Downloads");
        downloads_button.set_visible(false);

        let header_start = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header_start.append(&icon_button("go-previous-symbolic", "win.back", "Back"));
        header_start.append(&icon_button("go-next-symbolic", "win.forward", "Forward"));
        header_start.append(&reload);
        header_start.append(&home);
        header_start.append(&sidebar_toggle);
        let header_end = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header_end.append(extension_actions.widget());
        header_end.append(&icon_button("tab-new-symbolic", "win.new-tab", "New Tab"));
        header_end.append(&downloads_button);
        header_end.append(&menu_button);
        let header = adw::HeaderBar::new();
        header.pack_start(&header_start);
        header.set_title_widget(Some(&title));
        header.pack_end(&header_end);

        let update_banner = adw::Banner::builder().action_name("app.update").build();
        let bookmarks_bar = BookmarksBar::new();
        let find_bar = FindBar::new();
        let fullscreen_text = gtk::Label::builder()
            .justify(gtk::Justification::Center)
            .css_classes(["osd", "fullscreen-notice"])
            .build();
        let fullscreen_notice = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(24)
            .can_target(false)
            .child(&fullscreen_text)
            .build();
        let page = gtk::Overlay::new();
        page.set_child(Some(&tab_view));
        page.add_overlay(&fullscreen_notice);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&page));

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
            header_start,
            header_end,
            sidebar_toggle,
            location: title,
            address,
            reload,
            home,
            zoom_level,
            downloads_button,
            tab_view,
            tab_bar,
            tab_list,
            split,
            bookmarks_bar,
            find_bar,
            toasts,
            fullscreen_notice,
            fullscreen_text,
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
                // The last tab of the last window is saved while it still exists, so that,
                // as in Chrome, the next start restores it.
                if view.n_pages() == 1 && window.browser().windows().len() <= 1 {
                    window.browser().save_session_now();
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
            move |_, text, allow_inline| window.browser().omnibox_changed(&window, text, allow_inline)
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

        ui.address.connect_bubble_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.sync_permission_prompt()
        ));
        let clicks = gtk::GestureClick::builder()
            .button(0)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        clicks.connect_released(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _, _| window.clicked_while_prompting()
        ));
        self.add_controller(clicks);

        self.connect_fullscreened_notify(|window| {
            window
                .ui()
                .toolbar
                .set_reveal_top_bars(!window.is_fullscreen());
            if !window.is_fullscreen() {
                window.hide_fullscreen_notice();
            }
            window.sync_permission_prompt();
        });
    }

    /// Names the site whose page just went full screen, and how to leave, for a few seconds:
    /// with the header hidden, a page could otherwise draw a fake one (Chrome warns the same).
    pub(crate) fn show_fullscreen_notice(&self, site: &str) {
        let ui = self.ui();
        ui.fullscreen_text
            .set_label(&format!("{site} is now full screen\nPress Esc to exit full screen"));
        ui.fullscreen_notice.set_reveal_child(true);
        let timeout = glib::timeout_add_local_once(
            FULLSCREEN_NOTICE_TIME,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    // The source is gone once this runs; forget its id instead of removing it.
                    window.imp().fullscreen_notice_timeout.take();
                    window.hide_fullscreen_notice();
                }
            ),
        );
        if let Some(earlier) = self.imp().fullscreen_notice_timeout.replace(Some(timeout)) {
            earlier.remove();
        }
    }

    pub(crate) fn hide_fullscreen_notice(&self) {
        if let Some(timeout) = self.imp().fullscreen_notice_timeout.take() {
            timeout.remove();
        }
        self.ui().fullscreen_notice.set_reveal_child(false);
    }

    /// The last window saves the session while its tabs still exist (one closed with its last
    /// tab saved it then); every tab that goes with the window leaves the extension runtime.
    fn before_close(&self) {
        let browser = self.browser();
        if browser.windows().len() <= 1 && !self.tabs().is_empty() {
            browser.save_session_now();
        }
        let popups = self.imp().popups.take();
        for tab in self.tabs().into_iter().chain(popups) {
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
                let side = match pack {
                    gtk::PackType::End => &ui.header_end,
                    _ => &ui.header_start,
                };
                if let Some(parent) = ui.sidebar_toggle.parent().and_downcast::<gtk::Box>() {
                    parent.remove(&ui.sidebar_toggle);
                }
                side.append(&ui.sidebar_toggle);
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

    pub(crate) fn set_home_button_visible(&self, shown: bool) {
        self.ui().home.set_visible(shown);
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn home_button(&self) -> &gtk::Button {
        &self.ui().home
    }

    /// A compact bar is at most [`COMPACT_ADDRESS_WIDTH`] wide, centered in the header;
    /// otherwise it fills the space between the buttons.
    pub(crate) fn set_compact_address_bar(&self, compact: bool) {
        let width = if compact { COMPACT_ADDRESS_WIDTH } else { i32::MAX };
        let clamp = &self.ui().location;
        // The threshold at the maximum keeps the clamp from easing the width in below it.
        clamp.set_maximum_size(width);
        clamp.set_tightening_threshold(width);
    }

    pub(crate) fn set_full_urls(&self, full: bool) {
        self.ui().address.set_full_urls(full);
    }

    pub(crate) fn refresh_bookmarks_bar(&self) {
        self.ui().bookmarks_bar.refresh(self.browser().core());
    }

    /// The star in the address bar follows the selected tab's committed URL.
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
        self.ui().address.set_starred(starred);
    }

    /// The star's bubble on `node`: "Bookmark added" when the star just added it.
    pub(crate) fn show_bookmark_bubble(&self, node: BookmarkNode, added: bool) {
        let bubble = bookmark_editor::bubble(self, node, added);
        self.ui().address.show_popover(&bubble, Anchor::Star);
    }

    /// The connection popover of the selected tab's page, on the security icon.
    pub(crate) fn show_site_info(&self) {
        let Some(tab) = self.selected_tab() else { return };
        let permissions = permissions::site_info_section(self.browser(), &tab);
        let popover = site_info::popover(&site_info::Connection::of(&tab), permissions.as_ref());
        self.ui().address.show_popover(&popover, Anchor::Security);
    }

    /// Shows the selected tab's oldest waiting permission prompt, unless it is on screen.
    /// Other tabs' prompts wait until their tab is selected, so one shows at a time, and none
    /// shows while another bubble is open from the address bar or the window is fullscreen
    /// (the bar is hidden then). Called whenever any of that may have changed.
    pub(crate) fn sync_permission_prompt(&self) {
        let imp = self.imp();
        let tab = self.selected_tab();
        let hidden = self.ui().address.bubble().is_some() || self.is_fullscreen();
        let next = tab.as_ref().filter(|_| !hidden).and_then(|tab| permissions::next_prompt(self.browser(), tab));
        let on_screen = imp.prompt.borrow().as_ref().is_some_and(|shown| {
            next.as_ref().is_some_and(|next| shown.request == next.request && shown.asked == next.asked)
        });
        if on_screen {
            return;
        }
        if let Some(withdrawn) = imp.prompt.take() {
            withdrawn.popover.popdown();
        }
        let (Some(tab), Some(next)) = (tab, next) else { return };
        let request = next.request.clone();
        let popover = permissions::prompt_popover(
            &next.prompt,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                request,
                move |answer| window.answer_permission_prompt(&request, answer)
            ),
        );
        // A close the window did not make (Escape in the bubble, or a click elsewhere that
        // left the tab and the address bar as they were) is "Not now".
        popover.connect_closed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |popover| {
                let dismissed = {
                    let mut prompt = window.imp().prompt.borrow_mut();
                    if prompt.as_ref().is_some_and(|p| p.popover == *popover) { prompt.take() } else { None }
                };
                let Some(dismissed) = dismissed else { return };
                if let Some(tab) = dismissed.tab.upgrade() {
                    permissions::answer(window.browser(), &tab, &dismissed.request, &dismissed.asked, Answer::Dismiss);
                }
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    window,
                    move || permissions::enforce(window.browser())
                ));
            }
        ));
        imp.prompt.replace(Some(ShownPrompt { popover: popover.clone(), tab: tab.downgrade(), request, asked: next.asked }));
        self.ui().address.show_prompt(&popover);
    }

    /// Answers the prompt on screen, if it is still the one for `request`, and shows the next.
    fn answer_permission_prompt(&self, request: &webkit::PermissionRequest, answer: Answer) {
        let shown = {
            let mut prompt = self.imp().prompt.borrow_mut();
            if prompt.as_ref().is_some_and(|p| p.request == *request) { prompt.take() } else { None }
        };
        let Some(shown) = shown else { return };
        shown.popover.popdown();
        if let Some(tab) = shown.tab.upgrade() {
            permissions::answer(self.browser(), &tab, request, &shown.asked, answer);
        }
        permissions::enforce(self.browser());
    }

    /// A click in the window while a prompt shows. Once the click has done what it does,
    /// the prompt is dismissed if it is still on screen: a tab switch or another bubble
    /// from the address bar withdraws it instead, to come back later.
    fn clicked_while_prompting(&self) {
        let Some(popover) = self.imp().prompt.borrow().as_ref().map(|p| p.popover.clone()) else { return };
        let tab = self.selected_tab();
        glib::idle_add_local_once(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || {
                let still_shown = window.imp().prompt.borrow().as_ref().is_some_and(|p| p.popover == popover);
                if still_shown && window.selected_tab() == tab && window.ui().address.bubble().is_none() {
                    popover.popdown();
                }
            }
        ));
    }

    /// The in-use icon and tooltip on `tab`'s tab, if it shows one.
    #[cfg(feature = "self-test")]
    pub(crate) fn tab_indicator(&self, tab: &Tab) -> Option<(String, String)> {
        let page = self.page_of(tab)?;
        let icon = page.indicator_icon().and_downcast::<gio::ThemedIcon>()?;
        let name = icon.names().first().map(|n| n.to_string()).unwrap_or_default();
        Some((name, page.indicator_tooltip().to_string()))
    }

    // Extension actions.

    /// The toolbar shows the pinned actions in the synced order; the Extensions menu all.
    pub(crate) fn refresh_extension_actions(&self) {
        let actions = self.browser().runtime().actions();
        let available: Vec<String> = actions.iter().map(|a| a.extension.as_str().to_owned()).collect();
        let pinned: Vec<ExtensionId> = self
            .browser()
            .extension_toolbar(&available)
            .pinned
            .iter()
            .filter_map(|id| actions.iter().find(|a| a.extension.as_str() == id).map(|a| a.extension.clone()))
            .collect();
        self.ui().extension_actions.rebuild(
            &actions,
            &pinned,
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

    /// Opens the context menu of a pinned action's button, as a right-click does.
    #[cfg(feature = "self-test")]
    pub(crate) fn open_extension_context_menu(&self, id: &ExtensionId) -> Option<gtk::PopoverMenu> {
        let button = self.extension_action_button(id)?;
        Some(ext_actions::unpin_menu(button.upcast_ref(), id, (8.0, 8.0)))
    }

    /// Clicks the puzzle piece and returns the Extensions menu it opened.
    #[cfg(feature = "self-test")]
    pub(crate) fn open_extensions_menu(&self) -> Option<gtk::Popover> {
        self.ui().extension_actions.click_puzzle()
    }

    // Tabs.

    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn tab_list(&self) -> &TabList {
        &self.ui().tab_list
    }

    #[cfg(feature = "self-test")]
    pub(crate) fn tab_row_opacity(&self, tab: &Tab) -> Option<f64> {
        self.ui().tab_list.row_opacity(&self.page_of(tab)?)
    }

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
        self.load_new_tab_page(&tab);
    }

    fn load_new_tab_page(&self, tab: &Tab) {
        match new_tab::page(&mut self.browser().core().borrow_mut()) {
            Ok(html) => tab.web_view().load_html(&html, None),
            Err(e) => log::warn!("new tab page: {e}"),
        }
    }

    /// The home button: the homepage in the selected tab, or the new tab page there when
    /// the homepage is `about:home`. Like Chrome, the visit counts as a bookmark's.
    pub(crate) fn go_home(&self) {
        match self.browser().homepage() {
            Some(url) => self.navigate_with(url.as_str(), Transition::Bookmark),
            None => {
                let tab = match self.selected_tab() {
                    Some(tab) => tab,
                    None => self.open_tab(None, None, Focus::Foreground),
                };
                self.load_new_tab_page(&tab);
                self.ui().address.focus_for_typing();
            }
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
                if window.forget_popup(&popup) {
                    window.browser().tab_discarded(&popup);
                }
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
                    self.sync_permission_prompt();
                }
            }
            TabChange::Capture => {
                sync_indicator(&page, tab);
                if selected {
                    self.ui().address.set_in_use(permissions::indicator(tab.capturing()));
                }
            }
            TabChange::History if selected => self.sync_history(tab),
            TabChange::Zoom => {
                self.browser().zoom_changed(tab);
                if selected {
                    self.sync_zoom(tab);
                }
            }
            TabChange::Find(result) if selected => self.ui().find_bar.show_result(result),
            TabChange::Progress | TabChange::History | TabChange::Find(_) => {}
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
            self.sync_permission_prompt();
            return;
        };
        imp.chrome_tab.set(Some(&tab));
        ui.address.clear_progress();
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
        ui.address.set_in_use(permissions::indicator(tab.capturing()));
        tab.mark_active(session::now_ms());
        self.browser().tab_activated(&tab);
        if editing || tab.is_blank() {
            ui.address.focus_for_typing();
        } else {
            tab.web_view().grab_focus();
        }
        self.sync_permission_prompt();
    }

    fn sync_page(&self, page: &adw::TabPage) {
        let Ok(tab) = page.child().downcast::<Tab>() else {
            return;
        };
        page.set_title(&tab.display_title());
        page.set_icon(tab.web_view().favicon().as_ref());
        page.set_loading(tab.web_view().is_loading());
        sync_indicator(page, &tab);
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
        if loading {
            self.ui().address.set_progress(web_view.estimated_load_progress());
        } else {
            self.ui().address.finish_progress();
        }
    }

    fn sync_zoom(&self, tab: &Tab) {
        let level = tab.web_view().zoom_level();
        self.ui().zoom_level.set_label(&zoom::percent(level));
        self.ui().address.set_zoom(level);
    }

    fn set_action_enabled(&self, name: &str, enabled: bool) {
        if let Some(action) = self.lookup_action(name).and_downcast::<gio::SimpleAction>() {
            action.set_enabled(enabled);
        }
    }
}

/// The tab's in-use icon, while it captures.
fn sync_indicator(page: &adw::TabPage, tab: &Tab) {
    match permissions::indicator(tab.capturing()) {
        Some((icon, tooltip)) => {
            page.set_indicator_icon(Some(&gio::ThemedIcon::new(icon)));
            page.set_indicator_tooltip(&tooltip);
        }
        None => {
            page.set_indicator_icon(None::<&gio::Icon>);
            page.set_indicator_tooltip("");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Reply, Server, browser, wait_until};
    use vsesvit_core::prefs::keys;

    #[gtk::test]
    fn tab_rows_follow_the_pages_and_only_animate_on_screen() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        let view = window.ui().tab_view.clone();
        let list = window.tab_list();
        let pages = |view: &adw::TabView| (0..view.n_pages()).map(|i| view.nth_page(i)).collect::<Vec<_>>();
        for _ in 0..3 {
            window.open_tab(None, None, Focus::Foreground);
        }
        assert_eq!(list.row_counts(), (3, 0, true), "rows added before the window shows are simply there");
        window.close_tab(&window.tabs()[0]);
        assert_eq!(list.row_counts(), (2, 0, true), "an unshown row leaves at once");

        window.present();
        wait_until("the tab list on screen", || list.widget().is_mapped());
        window.open_tab(None, None, Focus::Foreground);
        let reordered = view.nth_page(2);
        view.reorder_page(&reordered, 0);
        assert_eq!(list.shown_pages(), pages(&view), "a reorder moves the row");
        let closing = view.nth_page(1);
        view.close_page(&closing);
        assert_eq!(view.n_pages(), 2, "the page goes at once");
        assert_eq!(list.row_counts().0, 2);
        wait_until("the rows to settle", || list.row_counts() == (2, 0, true));
        assert_eq!(list.shown_pages(), pages(&view));
        window.close();
    }

    #[gtk::test]
    fn the_home_button_follows_the_setting_and_opens_the_homepage_in_the_selected_tab() {
        let server = Server::start("127.0.0.1", |path| match path {
            "/home" => Reply::Page("Home"),
            _ => Reply::NotFound,
        });
        let browser = browser();
        let (window, other) = (BrowserWindow::new(&browser), BrowserWindow::new(&browser));
        window.present();
        other.present();
        let hidden = !window.ui().home.is_visible();
        browser.set_home_button_visible(true);
        let shown = window.ui().home.is_visible() && other.ui().home.is_visible();

        let url = server.url("/home");
        browser.core().borrow_mut().prefs().set(&keys::HOMEPAGE, &url).unwrap();
        let tab = window.open_tab(None, None, Focus::Foreground);
        gio::prelude::ActionGroupExt::activate_action(&window, "home", None);
        wait_until("the homepage", || tab.committed_uri().as_deref() == Some(url.as_str()));

        browser.core().borrow_mut().prefs().reset(&keys::HOMEPAGE).unwrap();
        gio::prelude::ActionGroupExt::activate_action(&window, "home", None);
        wait_until("the new tab page", || tab.committed_uri().as_deref() == Some("about:blank"));
        let tabs = window.tabs().len();

        browser.set_home_button_visible(false);
        let hidden_again = !window.ui().home.is_visible() && !other.ui().home.is_visible();
        window.destroy();
        other.destroy();
        assert!(hidden, "a fresh profile has no home button");
        assert!(shown, "the setting shows the button in every window");
        assert_eq!(tabs, 1, "home opens in the selected tab");
        assert!(hidden_again);
    }

    #[gtk::test]
    fn a_page_gone_full_screen_is_named_with_the_way_out() {
        let page = "<script>document.onfullscreenchange = () => \
                    document.title = document.fullscreenElement ? 'full' : 'windowed'</script>";
        let server = Server::start("127.0.0.1", |_| Reply::Body("text/html", page.into()));
        let window = BrowserWindow::new(&browser());
        window.present();
        let url = server.url("/");
        let tab = window.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the page", || tab.committed_uri().as_deref() == Some(url.as_str()));
        let run = |script: &str| {
            tab.web_view().evaluate_javascript(script, None, None, None::<&gio::Cancellable>, |_| {});
        };
        let title = || tab.web_view().title().unwrap_or_default();
        run("document.documentElement.requestFullscreen()");
        wait_until("full screen", || window.is_fullscreen() && title() == "full");
        let notice = &window.ui().fullscreen_notice;
        let shown = (notice.reveals_child(), window.ui().fullscreen_text.label());
        run("document.exitFullscreen()");
        wait_until("the window back", || !window.is_fullscreen() && title() == "windowed");
        let hidden = !notice.reveals_child();
        window.destroy();
        assert!(shown.0, "the notice shows");
        let site = url.trim_start_matches("http://").trim_end_matches('/');
        assert!(shown.1.contains(site) && shown.1.contains("Esc"), "{}", shown.1);
        assert!(hidden, "the notice goes with full screen");
    }

    #[gtk::test]
    fn a_popup_dropped_before_it_shows_leaves_the_extension_runtime() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.present();
        let opener = window.open_tab(None, None, Focus::Foreground);
        // The runtime hands back a tab's own content manager only while it knows the tab.
        let known = |tab: &Tab| {
            let ucm = browser.runtime().user_content_manager(tab.id());
            tab.web_view().user_content_manager().as_ref() == Some(&ucm)
        };
        let closed = Tab::new_related(&browser, &opener);
        window.adopt_popup(&opener, &closed);
        let adopted = known(&closed);
        closed.web_view().emit_by_name::<()>("close", &[]);
        let dropped = !known(&closed);
        let pending = Tab::new_related(&browser, &opener);
        window.adopt_popup(&opener, &pending);
        window.close();
        assert!(adopted, "a popup is known from the start");
        assert!(dropped, "a popup closed before it shows is discarded");
        assert!(!known(&pending), "a popup still pending when its window closes is discarded");
    }

    #[gtk::test]
    fn the_homepage_setting_reads_like_an_address() {
        let browser = browser();
        let homepage = |text: &str| {
            browser.core().borrow_mut().prefs().set(&keys::HOMEPAGE, &text.to_owned()).unwrap();
            browser.homepage().map(String::from)
        };
        let resolved = [
            homepage("example.com"),
            homepage("localhost:3000"),
            homepage("https://start.example/x"),
            homepage(" about:home "),
            homepage("about:blank"),
            homepage("javascript:alert(1)"),
            homepage("mailto:someone@example.com"),
        ];
        browser.core().borrow_mut().prefs().reset(&keys::HOMEPAGE).unwrap();
        assert_eq!(
            resolved.each_ref().map(Option::as_deref),
            [
                Some("https://example.com/"),
                Some("http://localhost:3000/"),
                Some("https://start.example/x"),
                None,
                None,
                None,
                None,
            ]
        );
    }

    #[gtk::test]
    fn closing_the_last_tab_of_the_last_window_keeps_it_in_the_session() {
        let server = Server::start("127.0.0.1", |_| Reply::Page("Kept"));
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.present();
        let url = server.url("/kept");
        let tab = window.open_tab(Some(&url), None, Focus::Foreground);
        wait_until("the page", || tab.committed_uri().as_deref() == Some(url.as_str()));
        let alone = browser.windows().len() == 1;
        window.close_tab(&tab);
        wait_until("the window to close", || browser.windows().is_empty());
        let saved = browser.core().borrow_mut().session().restore().unwrap();
        let urls: Vec<String> = saved
            .into_iter()
            .flat_map(|s| s.windows)
            .flat_map(|w| w.tabs)
            .map(|t| t.url.to_string())
            .collect();
        assert!(alone, "no other test's window is open");
        assert_eq!(urls, [url]);
    }

    #[gtk::test]
    fn a_compact_address_bar_is_narrow_and_a_full_one_fills_the_header() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.set_default_size(1600, 900);
        window.present();
        let address = window.address_bar().clone();
        let header = address.ancestor(adw::HeaderBar::static_type()).expect("the bar is in the header");

        browser.set_compact_address_bar(true);
        wait_until("a compact address bar", || (1..=COMPACT_ADDRESS_WIDTH).contains(&address.width()));
        let compact = address.width();
        let compact_centered = {
            let bounds = address.compute_bounds(&header).expect("the bar is in the header");
            (bounds.x() + bounds.width() / 2.0 - header.width() as f32 / 2.0).abs()
        };

        browser.set_compact_address_bar(false);
        wait_until("a full-width address bar", || address.width() > COMPACT_ADDRESS_WIDTH);
        let full = address.width();

        browser.set_compact_address_bar(true);
        window.destroy();
        assert!(compact <= COMPACT_ADDRESS_WIDTH, "compact bar is {compact} px");
        assert!(compact_centered < 40.0, "compact bar is {compact_centered} px off center");
        assert!(full > COMPACT_ADDRESS_WIDTH, "full bar is {full} px");
    }

    #[gtk::test]
    fn navigation_comes_first_and_the_right_sidebar_toggle_last() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.set_default_size(1600, 900);
        window.present();
        let ui = window.ui();
        let span = |widget: &gtk::Widget| {
            widget
                .compute_bounds(&window)
                .map_or((f32::NAN, f32::NAN), |b| (b.x(), b.x() + b.width()))
        };
        let (reload, toggle, address) = (
            ui.reload.clone().upcast::<gtk::Widget>(),
            ui.sidebar_toggle.clone().upcast::<gtk::Widget>(),
            ui.address.clone().upcast::<gtk::Widget>(),
        );

        window.apply_layout(TabsPosition::Left);
        wait_until("the toggle between reload and the address bar", || {
            let (toggle, address) = (span(&toggle), span(&address));
            span(&reload).1 <= toggle.0 && toggle.1 <= address.0
        });

        window.apply_layout(TabsPosition::Right);
        wait_until("the toggle after the address bar", || span(&toggle).0 >= span(&address).1);
        let toggle_is_last = ui.header_end.last_child().as_ref() == Some(&toggle);
        let reload_before_address = span(&reload).1 <= span(&address).0;

        window.apply_layout(browser.tabs_position());
        window.destroy();
        assert!(toggle_is_last, "the right sidebar's toggle sits after the menu");
        assert!(reload_before_address, "the navigation buttons stay before the address bar");
    }
}
