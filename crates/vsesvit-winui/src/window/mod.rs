//! One browser window: a tab list (the vertical pane on the left or right of the pages, or the
//! `TabView` strip in the title bar), the toolbar, the bookmarks bar and the page grid that hosts
//! every tab's web view.
//!
//! With the vertical pane the toolbar sits in the title bar and its empty stretches drag the
//! window (see `update_drag_regions`); with the top strip the strip's footer does. `tabs` only
//! owns the `Tab` values; their order and the selection live in the live tab list. No `RefCell`
//! borrow is held across a XAML call, because XAML raises events such as `SelectionChanged`
//! synchronously from inside them.

mod address;
mod chrome;
mod media;
mod permissions;
mod progress;
mod tab_actions;
mod tab_layout;
mod tab_menu;
mod trackers;
mod wiring;

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use vsesvit_core::address::{readable_url, simplified_url};
use vsesvit_core::bookmarks::BookmarkId;
use vsesvit_core::extensions::toolbar::Layout;
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::{TabsPosition, Theme};
use vsesvit_core::suggest::Queries;
use vsesvit_core::view_source;
use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::bookmark_editor::{Editor, Target};
use crate::bookmarks_bar::{Bar, BarCommand, BarItem, Disposition};
use crate::browser::{Browser, ClosedTab};
use crate::connection::Headline;
use crate::dialogs::{self, Dialog};
use crate::downloads::Indicator;
use crate::extension_toolbar as toolbar;
use crate::layout::StripKind;
use crate::omnibox::{self, Address};
use crate::popup::{self, Activation, ExtensionAction, OpenerTab, Popup};
use crate::session::{TabPlan, WindowPlan};
use crate::shortcuts::{self, Command};
use crate::player::Player;
use crate::strip::{SidePane, TopStrip};
use crate::tab::{Initial, Tab, TabId};
use crate::updates::{Action, Banner, Severity};
use crate::{capture, connection, exec, platform, xaml, zoom};

use chrome::Chrome;
use tab_actions::Split;
pub(crate) use tab_menu::TabAction;
use wiring::{strip_events, with};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Show {
    Activate,
    /// Shown without taking focus from the user's current window (scripted runs).
    NoActivate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuAction {
    Run(Command),
    Show(Dialog),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    End,
    After(TabId),
    /// Opened by this tab's page.
    FromPage(TabId),
}

/// The material behind the window's chrome, which is transparent over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Backdrop {
    /// Mica Alt, tinted by the desktop background: the Windows 11 default for tabbed apps.
    Mica,
    /// Acrylic, a blur of the windows behind this one.
    Acrylic,
}

impl Backdrop {
    fn markup(self) -> &'static str {
        match self {
            Backdrop::Mica => r#"<MicaBackdrop {ns} Kind="BaseAlt"/>"#,
            Backdrop::Acrylic => r#"<DesktopAcrylicBackdrop {ns}/>"#,
        }
    }
}

/// Puts `backdrop` behind `window`'s content.
pub(crate) fn set_backdrop(window: &Window, backdrop: Backdrop) {
    let set = xaml::load::<SystemBackdrop>(backdrop.markup()).and_then(|b| {
        window
            .cast::<IWindow2>()
            .and_then(|w| w.SetSystemBackdrop(&b))
    });
    if let Err(e) = set {
        log::warn!("window backdrop {backdrop:?}: {e}");
    }
}

/// Gives `window`'s content, `root`, and its title bar buttons `theme`.
pub(crate) fn set_theme(window: &Window, root: &FrameworkElement, theme: Theme) {
    let (element, title_bar) = match theme {
        Theme::System => (ElementTheme::Default, TitleBarTheme::UseDefaultAppMode),
        Theme::Light => (ElementTheme::Light, TitleBarTheme::Light),
        Theme::Dark => (ElementTheme::Dark, TitleBarTheme::Dark),
    };
    let _ = root.SetRequestedTheme(element);
    let preferred = window
        .cast::<IWindow2>()
        .and_then(|w| w.AppWindow())
        .and_then(|w| w.TitleBar())
        .and_then(|t| t.cast::<IAppWindowTitleBar3>())
        .and_then(|t| t.SetPreferredTheme(title_bar));
    if let Err(e) = preferred {
        log::debug!("title bar theme: {e}");
    }
}

/// The window's look, from preferences.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowPrefs {
    pub tabs: TabsPosition,
    pub pane_collapsed: bool,
    pub pane_width: u32,
    pub theme: Theme,
    pub bookmarks_bar: bool,
    /// The Home button next to Reload.
    pub home_button: bool,
    pub backdrop: Backdrop,
    /// A narrow address bar centered in the toolbar.
    pub compact_address: bool,
    /// Whole URLs in the address bar, instead of simplified ones while it is not focused.
    pub full_urls: bool,
    /// The media player at the foot of the vertical tab pane.
    pub media_player: bool,
    /// Picture-in-picture in the media player, for the sites that allowed it.
    pub pip: bool,
}

/// The compact address bar's widest.
const COMPACT_ADDRESS_WIDTH: f64 = 720.0;

/// Whether the star shows the page at `url` bookmarked: never a blank tab or the new tab page,
/// which have no address to bookmark.
pub(crate) fn starred(url: &str, is_bookmarked: impl FnOnce(&str) -> bool) -> bool {
    omnibox::has_link(url) && is_bookmarked(url)
}

/// The site icon's glyph and tooltip for a page, from the verdict of its security popup on the
/// engine's `report`: a lock for a secure page, a warning for one that is not, a search glyph
/// for a blank page and a page glyph for anything else.
fn site_look(url: &str, report: Option<&connection::Report>) -> (&'static str, &'static str) {
    match Headline::of(url, report) {
        Headline::Secure => ("\u{E72E}", "Connection is secure"),
        Headline::NotSecure => ("\u{E7BA}", "Not secure"),
        Headline::Local if !omnibox::has_link(url) => {
            ("\u{E721}", "Search or enter web address")
        }
        Headline::Local => (
            "\u{E8A5}",
            "This page is on your device or inside the browser",
        ),
    }
}

/// A window's position and size in screen pixels, and whether it is maximized.
type Bounds = ((i32, i32, u32, u32), bool);

pub(crate) struct BrowserWindow {
    browser: Weak<Browser>,
    window: Window,
    ui: Chrome,
    top: Rc<TopStrip>,
    side: Rc<SidePane>,
    player: Player,
    media: media::MediaState,
    progress: progress::Progress,
    tabs_position: Cell<TabsPosition>,
    tabs: RefCell<Vec<Rc<Tab>>>,
    /// The tab whose page opened each tab, while that still places the tabs it opens next.
    openers: RefCell<HashMap<TabId, TabId>>,
    /// The page's URL in the address box, or the user's edit and its suggestions.
    address: RefCell<Address>,
    /// The last key pressed in the address box deletes text (see `omnibox::deletes`).
    address_deleting: Cell<bool>,
    /// The address box's queries for the default engine's suggestions.
    search_queries: Queries,
    /// Keeps the suggestion list highlighting the edit's row (see `watch_suggestion_list`).
    suggestion_list_watch: RefCell<Option<windows_core::EventRevoker>>,
    /// The tab the toolbar currently shows.
    shown_tab: Cell<Option<TabId>>,
    /// A tab's row is being moved in the strip: its selection changes are not the user's.
    reordering: Cell<bool>,
    split: Cell<Option<Split>>,
    fullscreen: Cell<bool>,
    /// While fullscreen, the bounds the window had before, which a saved session keeps.
    windowed_bounds: Cell<Option<Bounds>>,
    /// The toolbar's drag regions last sent to the window; `None` since the layout changed.
    drag_regions: RefCell<Option<Vec<RectInt32>>>,
    /// The address box has the keyboard focus, and so shows the whole URL.
    address_focused: Cell<bool>,
    full_urls: Cell<bool>,
    bookmarks_bar_wanted: Cell<bool>,
    bar: Rc<Bar>,
    toolbar: Rc<toolbar::Toolbar>,
    /// The bookmark editor opened last, from the star or the bookmarks bar.
    editor: RefCell<Option<Rc<Editor>>>,
    /// The security icon's popup opened last.
    connection: RefCell<Option<Flyout>>,
    permissions: permissions::PermissionUi,
    dialog_open: Cell<bool>,
    /// What a scripted run's `show_dialog` shows over the window instead of the modal dialog.
    scripted_dialog: RefCell<Option<dialogs::Preview>>,
    /// A shortcut is being captured in Settings: the window's accelerators are off.
    shortcuts_suspended: Cell<bool>,
    /// What the update bar shows; the user may have closed it since.
    update_banner: RefCell<Option<Banner>>,
    closed: Cell<bool>,
    me: Weak<BrowserWindow>,
}

impl BrowserWindow {
    pub fn create(browser: &Rc<Browser>, show: Show, prefs: WindowPrefs) -> Result<Rc<Self>> {
        let ui = Chrome::load()?;
        let window = Window::new()?;
        window.SetTitle("Vsesvit")?;
        window.SetContent(&ui.root)?;
        window.SetExtendsContentIntoTitleBar(true)?;
        platform::set_window_icon(platform::window_handle(&window)?);
        let window2 = window.cast::<IWindow2>()?;

        let slot = Rc::new(OnceCell::new());
        let bar = Bar::new(
            ui.bookmarks_bar.clone(),
            ui.bookmark_items.clone(),
            ui.bookmarks_overflow.clone(),
            // The bar and toolbar are created before the window, so their commands reach it
            // through `slot`.
            Rc::new(wiring::later(&slot, BrowserWindow::bar_command)),
        )?;
        let toolbar = toolbar::Toolbar::new(
            ui.pinned_extensions.clone(),
            ui.extensions_menu.clone(),
            Rc::new(wiring::later(&slot, BrowserWindow::toolbar_command)),
        )?;
        let events = Rc::new(strip_events(&slot));
        let top = TopStrip::new(ui.tab_view.clone(), &events)?;
        let side = SidePane::new(&events)?;
        let player = Player::new(wiring::player_events(&slot))?;
        side.set_media(player.element())?;
        let this = Rc::new_cyclic(|me: &Weak<BrowserWindow>| Self {
            browser: Rc::downgrade(browser),
            window,
            ui,
            top,
            side,
            player,
            media: media::MediaState::default(),
            progress: progress::Progress::default(),
            tabs_position: Cell::new(prefs.tabs),
            tabs: RefCell::new(Vec::new()),
            openers: RefCell::new(HashMap::new()),
            address: RefCell::new(Address::Page(String::new())),
            address_deleting: Cell::new(false),
            search_queries: Queries::default(),
            suggestion_list_watch: RefCell::new(None),
            shown_tab: Cell::new(None),
            reordering: Cell::new(false),
            split: Cell::new(None),
            fullscreen: Cell::new(false),
            windowed_bounds: Cell::new(None),
            drag_regions: RefCell::new(None),
            address_focused: Cell::new(false),
            full_urls: Cell::new(prefs.full_urls),
            bookmarks_bar_wanted: Cell::new(prefs.bookmarks_bar),
            bar,
            toolbar,
            editor: RefCell::new(None),
            connection: RefCell::new(None),
            permissions: permissions::PermissionUi::default(),
            dialog_open: Cell::new(false),
            scripted_dialog: RefCell::new(None),
            shortcuts_suspended: Cell::new(false),
            update_banner: RefCell::new(None),
            closed: Cell::new(false),
            me: me.clone(),
        });
        let _ = slot.set(this.me.clone());
        this.side.set_width(f64::from(prefs.pane_width));
        this.set_pane_collapsed(prefs.pane_collapsed);
        this.show_layout(prefs.tabs)?;
        this.apply_theme(prefs.theme);
        this.apply_backdrop(prefs.backdrop);
        this.set_compact_address(prefs.compact_address);
        this.set_bookmarks_bar_visible(prefs.bookmarks_bar);
        this.set_home_button_visible(prefs.home_button);
        this.set_media_switches(prefs.media_player, prefs.pip);
        this.wire()?;
        this.wire_permissions()?;
        this.install_accelerators()?;
        this.show_shortcuts();
        this.size_for_screen()?;
        match show {
            Show::Activate => this.window.Activate()?,
            Show::NoActivate => {
                // Behind the user's windows, where a stray click cannot reach it; the in-app
                // capture reads the window's own surface, so being covered does not matter.
                let app = window2.AppWindow()?;
                app.ShowWithActivation(false)?;
                if let Err(e) = app
                    .cast::<IAppWindow2>()
                    .and_then(|a| a.MoveInZOrderAtBottom())
                {
                    log::warn!("moving the window to the back: {e}");
                }
            }
        }
        Ok(this)
    }

    fn app_window(&self) -> Result<AppWindow> {
        self.window.cast::<IWindow2>()?.AppWindow()
    }

    fn size_for_screen(&self) -> Result<()> {
        let hwnd = platform::window_handle(&self.window)?;
        let scale = f64::from(unsafe { GetDpiForWindow(hwnd) }.max(96)) / 96.0;
        let size = SizeInt32 {
            width: (1280.0 * scale) as i32,
            height: (860.0 * scale) as i32,
        };
        self.app_window()?.Resize(size)
    }

    /// Resizes the window to `width` by `height` screen pixels.
    pub fn resize(&self, width: i32, height: i32) -> Result<()> {
        self.app_window()?.Resize(SizeInt32 { width, height })
    }

    pub fn browser(&self) -> Option<Rc<Browser>> {
        self.browser.upgrade()
    }

    /// The XAML window, which owns the pickers and flyouts opened over it.
    pub fn xaml_window(&self) -> &Window {
        &self.window
    }

    pub fn xaml_root(&self) -> Result<XamlRoot> {
        self.ui.root.cast::<UIElement>()?.XamlRoot()
    }

    /// Physical pixels per XAML pixel; 1.0 before the window has a XamlRoot.
    pub(crate) fn scale(&self) -> f64 {
        self.xaml_root()
            .and_then(|r| r.RasterizationScale())
            .unwrap_or(1.0)
    }

    fn me(&self) -> Rc<Self> {
        self.me
            .upgrade()
            .expect("a window method runs while the window is alive")
    }

    /// Brings the window forward for a launch the user started (a forwarded command line).
    pub fn activate(&self) {
        if let Err(e) = self.window.Activate() {
            log::warn!("activate window: {e}");
        }
    }

    // ---- tabs ----

    /// Opens a window's tabs from a startup plan and selects its active tab.
    pub fn open_planned(&self, plan: &WindowPlan) -> Result<()> {
        for (index, tab) in plan.tabs.iter().enumerate() {
            let initial = tab.url.clone().map_or(Initial::Blank, Initial::Url);
            self.open_tab(initial, Placement::End, index == plan.active, Some(tab))?;
        }
        if let Some(bounds) = plan.bounds {
            self.apply_bounds(bounds, plan.maximized);
        }
        Ok(())
    }

    pub fn open_url_tab(&self, url: &str, foreground: bool) -> Result<Rc<Tab>> {
        self.open_tab(
            Initial::Url(url.to_owned()),
            Placement::End,
            foreground,
            None,
        )
    }

    pub fn open_blank_tab(&self) -> Result<Rc<Tab>> {
        let tab = self.open_tab(Initial::Blank, Placement::End, true, None)?;
        self.focus_address();
        Ok(tab)
    }

    /// A page's new-window request: a tab right after its opener, or in the background after
    /// the tabs the page opened before.
    pub fn open_tab_from(&self, opener: TabId, initial: Initial, background: bool) {
        if let Err(e) = self.open_tab(initial, Placement::FromPage(opener), !background, None) {
            log::error!("open tab: {e}");
        }
    }

    fn open_tab(
        &self,
        initial: Initial,
        placement: Placement,
        foreground: bool,
        restored: Option<&TabPlan>,
    ) -> Result<Rc<Tab>> {
        let browser = self.browser().ok_or_else(windows_core::Error::empty)?;
        let tab = Tab::new(
            browser.next_tab_id(),
            restored.and_then(|p| p.id),
            self.me.clone(),
        )?;
        if let Initial::Url(url) = &initial {
            tab.set_planned(url, restored.map_or("", |p| p.title.as_str()));
        }
        let pinned = restored.is_some_and(|p| p.pinned);
        if pinned {
            tab.set_pinned(true);
        }
        xaml::set_visible(tab.view(), false)?;
        let me = self.me.clone();
        let id = tab.id;
        tab.view()
            .cast::<UIElement>()?
            .GotFocus(move |_, _| wiring::with(&me, |w| w.page_focused(id)))?
            .forget();
        self.ui
            .pages
            .Children()?
            .Append(&tab.view().cast::<UIElement>()?)?;
        self.tabs.borrow_mut().push(tab.clone());

        let count = u32::try_from(self.tab_count().saturating_sub(1)).unwrap_or(u32::MAX);
        let index = match placement {
            Placement::End => count,
            Placement::After(opener) => self.index_of(opener).map_or(count, |i| i + 1),
            Placement::FromPage(opener) if foreground => {
                self.index_of(opener).map_or(count, |i| i + 1)
            }
            Placement::FromPage(opener) => {
                let openers = self.openers.borrow();
                tab_layout::from_page_index(&self.strip().order(), opener, |t| {
                    openers.get(&t).copied()
                })
                .and_then(|i| u32::try_from(i).ok())
                .unwrap_or(count)
            }
        };
        if let Placement::FromPage(opener) = placement {
            self.openers.borrow_mut().insert(tab.id, opener);
        }
        // Only pinned tabs go among the pinned ones.
        let pinned_before = u32::try_from(self.pinned_count(Some(tab.id))).unwrap_or(u32::MAX);
        let index = if pinned {
            index.min(pinned_before)
        } else {
            index.max(pinned_before)
        };
        let placed = self
            .strip()
            .insert(index, tab.id, &tab.look())
            .and_then(|()| {
                if foreground || self.strip().selected().is_none() {
                    self.strip().select(tab.id)?;
                }
                Ok(())
            });
        if let Err(e) = placed {
            // Leaves nothing of a tab that never starts; dropping `initial` cancels a request.
            let _ = self.remove_tab(&tab);
            return Err(e);
        }
        self.sync_selection();
        exec::spawn(tab.clone().start(
            browser.engine().environment().clone(),
            browser.page_script(),
            initial,
        ));
        browser.session_changed();
        Ok(tab)
    }

    pub fn close_tab(&self, id: TabId) {
        let Some(tab) = self.tab(id) else { return };
        if let Some(browser) = self.browser() {
            browser.tab_closing(self.tab_count() - 1);
        }
        if let Err(e) = self.remove_tab(&tab) {
            log::warn!("close tab {id}: {e}");
        }
        if let Some(browser) = self.browser() {
            let state = tab.state();
            browser.remember_closed(ClosedTab {
                url: state.url,
                title: state.title,
            });
            browser.session_changed();
        }
        let last = self.tabs.borrow().is_empty();
        if last {
            let _ = self.window.Close();
        }
    }

    fn remove_tab(&self, tab: &Rc<Tab>) -> Result<()> {
        let strip = self.strip();
        let order = strip.order();
        if let Some(index) = order.iter().position(|id| *id == tab.id) {
            if strip.selected() == Some(tab.id) && order.len() > 1 {
                let next = if index + 1 < order.len() {
                    index + 1
                } else {
                    index - 1
                };
                strip.select(order[next])?;
            }
            strip.remove(tab.id)?;
        }
        self.media_tab_closing(tab.id);
        xaml::remove_child(&self.ui.pages, &tab.view().cast::<UIElement>()?)?;
        tab.close();
        self.tabs.borrow_mut().retain(|t| t.id != tab.id);
        self.openers
            .borrow_mut()
            .retain(|child, opener| *child != tab.id && *opener != tab.id);
        self.forget_split_of(tab.id);
        self.sync_selection();
        self.refresh_media();
        Ok(())
    }

    pub fn tab(&self, id: TabId) -> Option<Rc<Tab>> {
        self.tabs.borrow().iter().find(|t| t.id == id).cloned()
    }

    /// Tabs in the tab list's order.
    pub fn tabs_in_order(&self) -> Vec<Rc<Tab>> {
        let order = self.strip().order();
        order.into_iter().filter_map(|id| self.tab(id)).collect()
    }

    fn index_of(&self, id: TabId) -> Option<u32> {
        let index = self.strip().order().iter().position(|t| *t == id)?;
        u32::try_from(index).ok()
    }

    pub fn active_tab(&self) -> Option<Rc<Tab>> {
        self.strip().selected().and_then(|id| self.tab(id))
    }

    fn select_index(&self, index: usize) {
        if let Some(&id) = self.strip().order().get(index) {
            let _ = self.strip().select(id);
        }
        self.sync_selection();
    }

    fn select_relative(&self, step: isize) {
        let count = self.tab_count() as isize;
        if count == 0 {
            return;
        }
        let current = self
            .strip()
            .selected()
            .and_then(|id| self.index_of(id))
            .unwrap_or(0) as isize;
        self.select_index((current + step).rem_euclid(count) as usize);
    }

    /// Shows the selected tab's web view, hides the rest, and refreshes the toolbar.
    fn sync_selection(&self) {
        let active = self.active_tab();
        if self.shown_tab.get() != active.as_ref().map(|t| t.id) {
            self.media_selection_moved();
        }
        self.update_pip();
        self.place_views(active.as_ref().map(|t| t.id));
        if self.fullscreen.get() && !active.as_ref().is_some_and(|t| t.state().fullscreen) {
            self.set_fullscreen(false);
        }
        let active_id = active.as_ref().map(|t| t.id);
        if self.shown_tab.replace(active_id) != active_id {
            self.forget_openers_unless(active_id);
            self.address.replace(Address::Page(String::new()));
            if let Some(tab) = &active {
                tab.mark_active();
                self.take_to_site_zoom(tab);
            }
            if let Some(browser) = self.browser() {
                browser.session_changed();
            }
        }
        self.refresh_chrome();
        self.show_permission_prompt();
    }

    /// Selecting a tab that is neither a page that opened tabs nor one they opened ends where
    /// those pages put their next tabs, as in Chrome.
    fn forget_openers_unless(&self, selected: Option<TabId>) {
        let mut openers = self.openers.borrow_mut();
        let related = selected.is_some_and(|id| {
            openers.contains_key(&id) || openers.values().any(|opener| *opener == id)
        });
        if !related {
            openers.clear();
        }
    }

    /// A typed or bookmarked navigation in any tab: new tabs from pages go right after them
    /// again, as in Chrome.
    pub fn forget_openers(&self) {
        self.openers.borrow_mut().clear();
    }

    fn strip_selection_changed(&self, kind: StripKind) {
        if kind == StripKind::of(self.tabs_position.get()) && !self.reordering.get() {
            self.sync_selection();
        }
    }

    /// Called by a tab whenever its state changed.
    pub fn tab_updated(&self, tab: &Tab) {
        self.strip().update(tab.id, &tab.look());
        if self.media_tab() == Some(tab.id) {
            self.refresh_media();
        }
        if self.active_tab().is_some_and(|a| a.id == tab.id) {
            self.refresh_chrome();
        }
    }

    pub fn tab_fullscreen_changed(&self, tab: &Tab, fullscreen: bool) {
        if self.active_tab().is_some_and(|a| a.id == tab.id) {
            self.set_fullscreen(fullscreen);
        }
    }

    fn refresh_chrome(&self) {
        let state = self.active_tab().map(|t| t.state()).unwrap_or_default();
        let _ = self.ui.back.SetIsEnabled(state.can_go_back);
        let _ = self.ui.forward.SetIsEnabled(state.can_go_forward);
        let (glyph, tip) = if state.loading() {
            ("\u{E711}", "Stop")
        } else {
            ("\u{E72C}", "Refresh")
        };
        let tip = shortcuts::current().tip(tip, Command::Reload);
        let _ = self.ui.reload_glyph.SetGlyph(glyph);
        let _ = xaml::set_tip(&self.ui.reload, &tip);
        let shown = self.address_shown(&state.url);
        let showing_page = match &mut *self.address.borrow_mut() {
            Address::Page(written) => {
                written.clone_from(&shown);
                true
            }
            Address::Editing(_) => false,
        };
        if showing_page && self.ui.address.Text().is_ok_and(|t| t != shown) {
            let _ = self.ui.address.SetText(&shown);
        }
        self.show_progress();
        self.show_star(state.starred);
        let report = self.active_tab().and_then(|t| t.security_report());
        let report = report.and_then(|json| connection::parse_report(&json));
        self.show_site(&state.url, report.as_ref());
        self.show_zoom(state.zoom);
        self.show_pip_button();
        self.show_permissions_state();
        self.show_tracking_status();
        let _ = xaml::set_visible(&self.ui.copy_link, omnibox::has_link(&state.url));
        let title = if state.title.is_empty() || state.url.is_empty() {
            "Vsesvit".to_owned()
        } else {
            format!("{} - Vsesvit", state.title)
        };
        let _ = self.window.SetTitle(&title);
    }

    /// The URL in readable form: whole while the user works in the address box or asked for
    /// full URLs, otherwise simplified.
    fn address_shown(&self, url: &str) -> String {
        let url = readable_url(omnibox::display_url(url));
        if self.address_focused.get() || self.full_urls.get() {
            url
        } else {
            simplified_url(&url)
        }
    }

    pub fn set_compact_address(&self, compact: bool) {
        let width = if compact {
            COMPACT_ADDRESS_WIDTH
        } else {
            f64::INFINITY
        };
        let _ = self.ui.address_pill.SetMaxWidth(width);
    }

    pub fn set_full_urls(&self, full: bool) {
        self.full_urls.set(full);
        self.refresh_chrome();
    }

    /// Clicking into the address box shows the whole URL, selected; leaving it shows the
    /// simplified one again, unless the user typed something.
    pub(super) fn address_focus_changed(&self, focused: bool) {
        // The box's list opening can raise `LostFocus` while the focus stays in the box.
        if !focused && self.focus_in_address() {
            return;
        }
        self.address_focused.set(focused);
        if !focused {
            // Focus moving into the box's own suggestion list leaves and returns within a turn.
            let me = self.me.clone();
            exec::spawn(async move {
                with(&me, |w| {
                    if !w.address_focused.get() {
                        w.close_suggestions();
                    }
                });
            });
        }
        self.refresh_chrome();
        let _ = xaml::set_visible(&self.ui.address_focus_ring, focused);
        if let Some(text_box) = self.address_text_box() {
            let alignment = if focused {
                TextAlignment::Left
            } else {
                TextAlignment::Center
            };
            let _ = text_box.SetTextAlignment(alignment);
            // The box's own list takes the focus from it and gives it back while the user
            // edits, which must leave the edit's selection alone.
            if focused && matches!(*self.address.borrow(), Address::Page(_)) {
                let _ = text_box.SelectAll();
            }
        }
    }

    /// The icon at the start of the address pill: the page's security, or a search glyph for
    /// the new tab page.
    fn show_site(&self, url: &str, report: Option<&connection::Report>) {
        let (glyph, tip) = site_look(url, report);
        let _ = self.ui.site_icon.SetGlyph(glyph);
        let _ = xaml::set_tip(&self.ui.site_icon, tip);
    }

    /// The zoom chip, shown while the page is not at 100%, and the zoom bubble's level.
    fn show_zoom(&self, level: zoom::Level) {
        let label = level.label();
        let _ = self.ui.zoom_chip_text.SetText(&label);
        let _ = self.ui.zoom_level.SetText(&label);
        let _ = xaml::set_visible(&self.ui.zoom_chip, !level.is_default());
        if level.is_default() {
            let _ = self
                .ui
                .zoom_bubble
                .cast::<FlyoutBase>()
                .and_then(|b| b.Hide());
        }
    }

    /// A button of the zoom bubble.
    pub(super) fn zoom_clicked(&self, step: zoom::Step) {
        match self.active_tab() {
            Some(tab) if self.is_foreground() => tab.zoom(vec![step], false),
            _ => log::info!("zoom {step:?}: the window is not in the foreground"),
        }
    }

    /// Takes the selected tab's page to the zoom remembered for its site
    /// ([`Tab::wanted_zoom`]) with the key presses a person would use: only in the foreground
    /// window, never in scripted runs, which send no OS input, and never while the user types
    /// in the address box, which the presses would take the focus from. [`Tab::zoom`] checks
    /// again just before sending them, and leaves the level wanted if it cannot.
    pub(crate) fn take_to_site_zoom(&self, tab: &Tab) {
        let interactive = self
            .browser()
            .is_some_and(|b| b.config().mode.is_interactive());
        let selected = self.active_tab().is_some_and(|a| a.id == tab.id);
        if !interactive
            || !selected
            || tab.wanted_zoom().is_none()
            || self.in_pip(tab.id)
            || self.address_focused.get()
            || !self.is_foreground()
        {
            return;
        }
        if let Some(level) = tab.take_wanted_zoom() {
            let steps = zoom::steps(tab.state().zoom, level);
            log::debug!("tab {}: to its site's zoom {}", tab.id, level.label());
            if !steps.is_empty() {
                tab.zoom(steps, true);
            }
        }
    }

    pub fn zoom_chip_shown(&self) -> Option<String> {
        xaml::is_visible(&self.ui.zoom_chip)
            .then(|| self.ui.zoom_chip_text.Text().ok())
            .flatten()
            .map(|t| t.to_string())
    }

    /// Opens the zoom bubble, as a click on the chip does.
    pub fn show_zoom_bubble(&self) -> Result<()> {
        self.ui
            .zoom_bubble
            .cast::<FlyoutBase>()?
            .ShowAt(&self.ui.zoom_chip.cast::<FrameworkElement>()?)
    }

    /// The security icon: a popup on the page's connection and certificate.
    pub fn show_connection(&self) -> Result<()> {
        let Some(tab) = self.active_tab() else {
            return Ok(());
        };
        let url = tab.state().url;
        if !omnibox::has_link(&url) {
            return Ok(());
        }
        self.close_suggestions();
        let report = tab
            .security_report()
            .and_then(|json| connection::parse_report(&json));
        let host = connection::host_of(&url);
        let content =
            connection::content(&url, &host, report.as_ref(), self.tracking_status(&tab))?;
        self.wire_tracking_switch(&content, &tab)?;
        if let (Ok(button), Some(report)) =
            (xaml::find::<Button>(&content, "ShowCertificate"), report)
        {
            let window = self.me.clone();
            dialogs::on_click(&button, move || {
                with(&window, |w| {
                    if let Ok(owner) = platform::window_handle(&w.window) {
                        connection::show_native(owner, report.chain.clone());
                    }
                });
            })?;
        }
        // Before it shows, not once it is open: a closing prompt can hold the opening back.
        self.fill_permissions_in(&content, &tab)?;
        let flyout = connection::flyout(&content)?;
        self.prompt_yields_to(&flyout)?;
        self.show_at_site_button(&flyout.cast()?)?;
        *self.connection.borrow_mut() = Some(flyout);
        Ok(())
    }

    pub fn hide_connection(&self) {
        if let Some(flyout) = self.connection.borrow().as_ref() {
            hide(flyout);
        }
    }

    /// The security icon's popup while it is open.
    pub fn connection_popup(&self) -> Option<FrameworkElement> {
        open_content(self.connection.borrow().as_ref()?)
    }

    /// Shows the site-info popup or the permission prompt under the site-info button; behind
    /// another window it must not take the focus.
    fn show_at_site_button(&self, flyout: &FlyoutBase) -> Result<()> {
        let options = FlyoutShowOptions::new()?;
        options.SetShowMode(if self.is_foreground() {
            FlyoutShowMode::Standard
        } else {
            FlyoutShowMode::Transient
        })?;
        flyout.ShowAtWithOptions(&self.ui.site_button.cast::<FrameworkElement>()?, &options)
    }

    fn show_star(&self, starred: bool) {
        let _ = self.ui.star.SetIsChecked(Some(starred));
        let _ = self
            .ui
            .star_glyph
            .SetGlyph(if starred { "\u{E735}" } else { "\u{E734}" });
    }

    /// Re-reads each tab's bookmarked state (after a bookmark changed anywhere).
    pub fn refresh_starred(&self, is_bookmarked: &dyn Fn(&str) -> bool) {
        let tabs = self.tabs.borrow().clone();
        for tab in tabs {
            let url = tab.state().url;
            tab.set_starred(starred(&url, is_bookmarked));
        }
        self.refresh_chrome();
    }

    pub fn address_width(&self) -> f64 {
        self.ui
            .address
            .cast::<FrameworkElement>()
            .and_then(|a| a.ActualWidth())
            .unwrap_or(0.0)
    }

    pub fn address_text(&self) -> String {
        self.ui.address.Text().unwrap_or_default()
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.borrow().len()
    }

    // ---- commands ----

    pub fn run(&self, command: Command) {
        let active = self.active_tab();
        match command {
            Command::NewTab => {
                if let Err(e) = self.open_blank_tab() {
                    log::error!("new tab: {e}");
                }
            }
            Command::NewWindow => {
                if let Some(browser) = self.browser()
                    && let Err(e) = browser.open_blank_window(Show::Activate)
                {
                    log::error!("new window: {e}");
                }
            }
            Command::CloseTab => {
                if let Some(tab) = active {
                    self.close_tab(tab.id);
                }
            }
            Command::ReopenClosedTab => {
                if let Some(closed) = self.browser().and_then(|b| b.take_closed()) {
                    log::info!("reopening {} ({})", closed.url, closed.title);
                    if let Err(e) = self.open_url_tab(&closed.url, true) {
                        log::error!("reopen tab: {e}");
                    }
                }
            }
            Command::FocusAddress => self.focus_address(),
            Command::Reload => {
                if let Some(tab) = active {
                    tab.reload();
                }
            }
            Command::Back => {
                if let Some(tab) = active {
                    tab.go_back();
                }
            }
            Command::Forward => {
                if let Some(tab) = active {
                    tab.go_forward();
                }
            }
            Command::NextTab => self.select_relative(1),
            Command::PreviousTab => self.select_relative(-1),
            Command::SelectTab(index) => {
                if usize::from(index) < self.tab_count() {
                    self.select_index(usize::from(index));
                }
            }
            Command::SelectLastTab => {
                if let Some(last) = self.tab_count().checked_sub(1) {
                    self.select_index(last);
                }
            }
            Command::Bookmark => self.star_clicked(),
            Command::Find => {
                if let (Some(tab), Some(browser)) = (active, self.browser()) {
                    match browser.engine().find_options("") {
                        Ok(options) => exec::spawn(async move {
                            if let Err(e) = tab.find(options).await {
                                log::warn!("find: {e}");
                            }
                        }),
                        Err(e) => log::warn!("find: {e}"),
                    }
                }
            }
            Command::ToggleBookmarksBar => {
                if let Some(browser) = self.browser() {
                    browser.set_bookmarks_bar_visible(!browser.bookmarks_bar_visible());
                }
            }
            Command::ShowBookmarks => self.show_dialog(Dialog::Bookmarks),
            Command::ShowHistory => self.show_dialog(Dialog::History),
            Command::ShowDownloads => self.show_dialog(Dialog::Downloads),
            Command::ToggleTabPane => {
                if self.tabs_position.get() != TabsPosition::Top
                    && let Some(browser) = self.browser()
                {
                    browser.set_tab_pane_collapsed(!self.is_pane_collapsed());
                }
            }
            Command::CopyCleanLink | Command::CopyLink => {
                if let Some(tab) = active {
                    self.copy_link(&tab, command == Command::CopyCleanLink);
                }
            }
            Command::SavePage => {
                if let Some(tab) = active {
                    exec::spawn(save_page(self.me.clone(), tab));
                }
            }
            Command::Print => {
                if let Some(tab) = active
                    && let Err(e) = tab.print()
                {
                    log::warn!("tab {}: print: {e}", tab.id);
                }
            }
            Command::ViewSource => {
                if let Some(tab) = active
                    && let Some(url) = view_source::source_url(&tab.state().url)
                {
                    self.open_tab_from(tab.id, Initial::Url(url), false);
                }
            }
            Command::DeveloperTools | Command::JavaScriptConsole => {
                if let Some(tab) = active
                    && let Err(e) = tab.open_devtools()
                {
                    log::warn!("tab {}: developer tools: {e}", tab.id);
                }
            }
        }
    }

    /// Applies the bindings in effect: the accelerators, the shortcuts that menus and tooltips
    /// name, and the pages' key sets.
    pub fn shortcuts_changed(&self) {
        if let Err(e) = self.set_accelerators() {
            log::warn!("keyboard accelerators: {e}");
        }
        self.show_shortcuts();
        let tabs = self.tabs.borrow().clone();
        for tab in tabs {
            tab.shortcuts_changed();
        }
    }

    /// Turns the window's accelerators off while Settings captures a shortcut, and on again.
    pub fn suspend_shortcuts(&self, suspended: bool) {
        self.shortcuts_suspended.set(suspended);
        if let Err(e) = self.set_accelerators() {
            log::warn!("keyboard accelerators: {e}");
        }
    }

    /// The key and modifiers of each of the window's accelerators.
    pub fn accelerator_keys(&self) -> Vec<(i32, u32)> {
        let Ok(accelerators) = self
            .ui
            .root
            .cast::<UIElement>()
            .and_then(|r| r.KeyboardAccelerators())
        else {
            return Vec::new();
        };
        accelerators
            .into_iter()
            .filter_map(|a| Some((a.Key().ok()?.0, a.Modifiers().ok()?.0)))
            .collect()
    }

    fn show_shortcuts(&self) {
        let bindings = shortcuts::current();
        let ui = &self.ui;
        let tips = [
            (ui.back.cast::<DependencyObject>(), "Back", Command::Back),
            (ui.forward.cast(), "Forward", Command::Forward),
            (
                ui.copy_link.cast(),
                "Copy link without trackers",
                Command::CopyCleanLink,
            ),
            (ui.star.cast(), "Bookmark this page", Command::Bookmark),
            (ui.downloads.cast(), "Downloads", Command::ShowDownloads),
        ];
        for (element, text, command) in tips {
            let tip = bindings.tip(text, command);
            let _ = element.and_then(|e| xaml::set_tip(&e, &tip));
        }
        let menu = [
            ("MenuNewTab", Command::NewTab),
            ("MenuNewWindow", Command::NewWindow),
            ("MenuBookmarks", Command::ShowBookmarks),
            ("MenuHistory", Command::ShowHistory),
            ("MenuDownloads", Command::ShowDownloads),
            ("MenuSavePage", Command::SavePage),
            ("MenuPrint", Command::Print),
            ("MenuDeveloperTools", Command::DeveloperTools),
            ("MenuViewSource", Command::ViewSource),
        ];
        for (name, command) in menu {
            let label = bindings.label(command).unwrap_or_default();
            let _ = xaml::find::<MenuFlyoutItem>(&ui.root, name)
                .and_then(|item| item.SetKeyboardAcceleratorTextOverride(&label));
        }
        self.refresh_chrome();
        self.side.show_shortcuts();
    }

    /// View page source is only for pages with a source, as in Chrome.
    pub(super) fn menu_opening(&self) {
        let url = self.active_tab().map(|t| t.state().url).unwrap_or_default();
        let _ = xaml::find::<Control>(&self.ui.root, "MenuViewSource")
            .and_then(|item| item.SetIsEnabled(view_source::source_url(&url).is_some()));
    }

    /// Says in the window that something the user asked for failed.
    pub fn show_failure(&self, title: &str, message: &str) {
        let bar = &self.ui.notice_bar;
        let _ = bar.SetTitle(title);
        let _ = bar.SetMessage(message);
        let _ = bar.SetIsOpen(true);
    }

    pub fn show_dialog(&self, dialog: Dialog) {
        let Some(browser) = self.browser() else {
            return;
        };
        if !browser.config().mode.is_interactive() {
            self.show_scripted_dialog(dialog);
            return;
        }
        if dialog.windowed() {
            browser.show_dialog_window(dialog, &self.me());
            return;
        }
        if self.dialog_open.replace(true) {
            return;
        }
        self.close_suggestions();
        let me = self.me();
        exec::spawn(async move {
            if let Err(e) = dialogs::show(&me, dialog).await {
                log::error!("{dialog:?} dialog: {e}");
            }
            me.dialog_open.set(false);
        });
    }

    /// A scripted run never shows the modal dialog (see `dialogs`): its content goes over the
    /// window until the next one or `close_scripted_dialog`.
    fn show_scripted_dialog(&self, dialog: Dialog) {
        self.close_scripted_dialog();
        match dialogs::preview(&self.me(), dialog) {
            Ok(preview) => *self.scripted_dialog.borrow_mut() = Some(preview),
            Err(e) => log::error!("{dialog:?} dialog: {e}"),
        }
    }

    /// The dialog a scripted run's `show_dialog` put over the window.
    pub fn scripted_dialog(&self) -> Option<Dialog> {
        self.scripted_dialog
            .borrow()
            .as_ref()
            .map(dialogs::Preview::kind)
    }

    pub fn close_scripted_dialog(&self) {
        drop(self.scripted_dialog.take());
    }

    /// Shows `body` over the window like a dialog, without the modal dialog's focus handling,
    /// so a scripted run can capture it without activating anything. `None` removes it.
    pub fn set_overlay(&self, content: Option<(&str, &UIElement)>) -> Result<()> {
        let children = self.ui.overlay_body.Children()?;
        children.Clear()?;
        if let Some((title, body)) = content {
            self.close_suggestions();
            self.ui.overlay_title.SetText(title)?;
            children.Append(body)?;
        }
        xaml::set_visible(&self.ui.overlay, content.is_some())
    }

    /// Programmatic focus in an inactive window would activate it, which scripted runs and
    /// background events must never do.
    pub fn is_foreground(&self) -> bool {
        platform::window_handle(&self.window)
            .is_ok_and(|hwnd| unsafe { GetForegroundWindow() } == hwnd)
    }

    fn focus_address(&self) {
        if !self.is_foreground() {
            return;
        }
        let _ = self
            .ui
            .address
            .cast::<UIElement>()
            .and_then(|a| a.Focus(FocusState::Programmatic));
        if let Some(text_box) = self.address_text_box() {
            let _ = text_box.SelectAll();
        }
    }

    /// The star button and Ctrl+D: bookmarks the page at the end of the bookmarks bar and opens
    /// the editor as "Bookmark added", or opens it as "Edit bookmark" on a bookmarked page.
    pub fn star_clicked(&self) {
        let (Some(tab), Some(browser)) = (self.active_tab(), self.browser()) else {
            return;
        };
        let state = tab.state();
        if !omnibox::has_link(&state.url) {
            self.show_star(false);
            return;
        }
        let target = match browser.bookmark_of(&state.url) {
            Some(id) => Target::Existing(id),
            None => {
                let Some(id) = browser.bookmark_page(&state.url, &state.title) else {
                    return;
                };
                if let Some(png) = tab.favicon_png() {
                    browser.record_favicon(&state.url, &png);
                }
                Target::Added(id)
            }
        };
        self.refresh_chrome();
        if let Ok(star) = self.ui.star.cast::<FrameworkElement>() {
            self.open_editor(&star, target);
        }
    }

    fn open_editor(&self, anchor: &FrameworkElement, target: Target) {
        if let Some(open) = self.editor.take() {
            open.close();
        }
        match Editor::open(&self.me(), anchor, target, self.is_foreground()) {
            Ok(editor) => *self.editor.borrow_mut() = Some(editor),
            Err(e) => log::error!("bookmark editor: {e}"),
        }
    }

    /// The bookmark editor while it is open.
    pub fn bookmark_editor(&self) -> Option<Rc<Editor>> {
        self.editor.borrow().clone().filter(|e| e.is_open())
    }

    /// The Home button: the home page in the current tab, or the new tab page by default.
    pub fn go_home(&self) {
        let (Some(browser), Some(tab)) = (self.browser(), self.active_tab()) else {
            return;
        };
        // Chrome counts the Home button as a bookmark (AUTO_BOOKMARK).
        match browser.home_page() {
            Some(url) => tab.navigate_as(&url, Transition::Bookmark),
            None => tab.go_to_new_tab_page(),
        }
    }

    // ---- bars ----

    pub fn set_bookmarks_bar_visible(&self, visible: bool) {
        self.bookmarks_bar_wanted.set(visible);
        let _ = xaml::set_visible(&self.ui.bookmarks_bar, visible && !self.fullscreen.get());
    }

    pub fn set_home_button_visible(&self, visible: bool) {
        let _ = xaml::set_visible(&self.ui.home, visible);
    }

    pub fn home_button_shown(&self) -> bool {
        xaml::is_visible(&self.ui.home)
    }

    pub fn bookmarks_bar_shown(&self) -> bool {
        xaml::is_visible(&self.ui.bookmarks_bar)
    }

    /// Replaces the bookmarks bar's items.
    pub fn set_bookmarks_bar(&self, items: &[BarItem]) {
        self.bar.set(items);
        let _ = xaml::set_visible(&self.ui.bookmarks_hint, items.is_empty());
    }

    pub fn bookmarks_bar_items(&self) -> Vec<BarItem> {
        self.bar.items()
    }

    pub fn bookmarks_bar_list(&self) -> ListView {
        self.bar.list().clone()
    }

    /// The number of entries the bookmarks bar holds, shown or behind the chevron.
    pub fn bookmarks_bar_buttons(&self) -> u32 {
        self.bar.len()
    }

    /// The bar's items shown whole, and those in the chevron's menu.
    pub fn bookmarks_bar_split(&self) -> (Vec<BarItem>, Vec<BarItem>) {
        (self.bar.shown_items(), self.bar.overflow_items())
    }

    pub fn bookmarks_overflow_shown(&self) -> bool {
        xaml::is_visible(&self.ui.bookmarks_overflow)
    }

    pub(crate) fn fit_bookmarks_bar(&self) {
        self.bar.fit();
    }

    pub fn open_bookmarks_folder(&self, id: BookmarkId) -> Result<MenuFlyout> {
        self.bar.open_folder(id)
    }

    /// The chevron's menu of the bookmarks that do not fit.
    pub fn show_bookmarks_overflow(&self) -> Result<MenuFlyout> {
        self.bar.show_overflow()
    }

    pub(super) fn bar_item_clicked(&self, clicked: &IInspectable) {
        self.bar.clicked(clicked);
    }

    /// A bar item was dragged to a new place: move its bookmark there in core.
    pub(crate) fn bar_item_dropped(&self) {
        let Some(browser) = self.browser() else {
            return;
        };
        if let Some((id, before)) = self.bar.dropped()
            && let Err(e) = browser.move_bookmark_before(id, BookmarkId::TOOLBAR, before)
        {
            log::warn!("move bookmark: {e}");
        }
        browser.bookmarks_changed();
    }

    /// Opens the context menu of a bookmark the bar or one of its menus shows, or of the bar
    /// itself for `None`.
    pub fn show_bookmark_context_menu(&self, id: Option<BookmarkId>) -> Result<MenuFlyout> {
        self.bar.show_context_menu(id)
    }

    /// Runs a command of the bookmarks bar or of its menus.
    pub(crate) fn bar_command(&self, command: BarCommand) {
        let Some(browser) = self.browser() else {
            return;
        };
        match command {
            BarCommand::Open(url, disposition) => self.open_link(&url, disposition),
            BarCommand::OpenAll(urls) => {
                for url in urls {
                    self.open_link(&url, Disposition::BackgroundTab);
                }
            }
            BarCommand::Edit(id) => {
                let anchor = self.bar.element_of(id).unwrap_or_else(|| self.bar_anchor());
                self.open_editor(&anchor, Target::Existing(id));
            }
            BarCommand::CopyLink(url) => {
                if let Err(e) = platform::copy_text(&url) {
                    log::warn!("copy link: {e}");
                }
            }
            BarCommand::Delete {
                id,
                title,
                contents: 0,
            } => {
                log::info!("deleting the bookmark {title:?}");
                browser.remove_bookmark(id);
            }
            BarCommand::Delete {
                id,
                title,
                contents,
            } => {
                let me = self.me();
                exec::spawn(async move {
                    let text = format!(
                        "\u{201C}{title}\u{201D} and the {contents} item(s) in it will be deleted."
                    );
                    match dialogs::confirm(&me, "Delete this folder?", &text, "Delete").await {
                        Ok(true) => browser.remove_bookmark(id),
                        Ok(false) => {}
                        Err(e) => log::warn!("confirm folder delete: {e}"),
                    }
                });
            }
            BarCommand::AddPage => {
                let state = self.active_tab().map(|t| t.state()).unwrap_or_default();
                let target = Target::NewPage {
                    title: state.title,
                    url: readable_url(omnibox::display_url(&state.url)),
                };
                self.open_editor(&self.bar_anchor(), target);
            }
            BarCommand::AddFolder => self.open_editor(&self.bar_anchor(), Target::NewFolder),
            BarCommand::ToggleBar => self.run(Command::ToggleBookmarksBar),
            BarCommand::Manager => self.show_dialog(Dialog::Bookmarks),
        }
    }

    /// Where the editor opens for what the bar does not show whole: under the chevron, or at
    /// the bar's end.
    fn bar_anchor(&self) -> FrameworkElement {
        if xaml::is_visible(&self.ui.bookmarks_overflow)
            && let Ok(chevron) = self.ui.bookmarks_overflow.cast()
        {
            return chevron;
        }
        self.ui.bookmarks_bar.clone()
    }

    pub fn open_link(&self, url: &str, disposition: Disposition) {
        let result = match (disposition, self.active_tab()) {
            (Disposition::CurrentTab, Some(tab)) => {
                tab.navigate_as(url, Transition::Bookmark);
                Ok(())
            }
            (Disposition::CurrentTab, None) => self.open_url_tab(url, true).map(drop),
            (Disposition::BackgroundTab, _) => self.open_url_tab(url, false).map(drop),
            (Disposition::NewWindow, _) => match self.browser() {
                Some(browser) => browser
                    .open_window(
                        &WindowPlan::with_tabs(vec![TabPlan::url(url.to_owned())]),
                        Show::Activate,
                    )
                    .map(drop),
                None => Ok(()),
            },
        };
        if let Err(e) = result {
            log::error!("open {url}: {e}");
        }
    }

    /// Shows `banner` in the update bar, or hides the bar. A bar the user closed opens again
    /// only for news (a new title), not for progress.
    pub fn show_update(&self, banner: Option<&Banner>) {
        let news =
            self.update_banner.borrow().as_ref().map(|b| &b.title) != banner.map(|b| &b.title);
        *self.update_banner.borrow_mut() = banner.cloned();
        let bar = &self.ui.update_bar;
        let Some(banner) = banner else {
            let _ = bar.SetIsOpen(false);
            return;
        };
        let severity = match banner.severity {
            Severity::Informational => InfoBarSeverity::Informational,
            Severity::Error => InfoBarSeverity::Error,
        };
        let _ = bar.SetSeverity(severity);
        let _ = bar.SetTitle(&banner.title);
        let _ = bar.SetMessage(&banner.message);
        let action = &self.ui.update_action;
        if let Some(label) = banner.action.map(Action::label) {
            let _ = xaml::boxed(label)
                .and_then(|label| action.cast::<IContentControl>()?.SetContent(&label));
        }
        let _ = xaml::set_visible(action, banner.action.is_some());
        if news {
            let _ = bar.SetIsOpen(true);
        }
    }

    /// The toolbar's downloads button: shown once a download started, busy while one runs.
    pub fn show_downloads(&self, indicator: Indicator) {
        let _ = xaml::set_visible(&self.ui.downloads, indicator != Indicator::Hidden);
        let _ = self
            .ui
            .downloads_busy
            .SetIsActive(indicator == Indicator::Busy);
    }

    pub fn downloads_button_shown(&self) -> bool {
        xaml::is_visible(&self.ui.downloads)
    }

    fn update_clicked(&self) {
        let action = self.update_banner.borrow().as_ref().and_then(|b| b.action);
        if let (Some(action), Some(browser)) = (action, self.browser()) {
            browser.update_action(action);
        }
    }

    /// Shows the pinned extension actions in the toolbar, in `layout`'s order.
    pub fn set_extension_actions(&self, actions: &[ExtensionAction], layout: &Layout) {
        self.toolbar.set(actions, layout);
    }

    /// Opens the popup of the extension action for `engine_id`, as a click would: under its
    /// pinned button, or under the Extensions button.
    pub fn open_extension_popup(&self, engine_id: &str, activation: Activation) -> Result<Popup> {
        let browser = self.browser().ok_or_else(windows_core::Error::empty)?;
        let action = browser
            .extension_actions()
            .into_iter()
            .find(|a| a.extension_id == engine_id)
            .ok_or_else(|| windows_core::Error::new(E_FAIL, "no action for that extension"))?;
        let anchor = self
            .toolbar
            .anchor_of(engine_id)
            .ok_or_else(windows_core::Error::empty)?;
        self.show_popup(&anchor, &action, activation)
    }

    /// Runs a command of the extension toolbar or its menus.
    pub(crate) fn toolbar_command(&self, command: toolbar::Command) {
        let Some(browser) = self.browser() else {
            return;
        };
        match command {
            toolbar::Command::Open { id, anchor } => {
                let action = browser.extension_actions().into_iter().find(|a| a.id == id);
                if let Some(action) = action
                    && let Err(e) = self.show_popup(&anchor, &action, Activation::Focus)
                {
                    log::error!("popup of {id}: {e}");
                }
            }
            toolbar::Command::Pin(id, pinned) => browser.set_extension_pinned(&id, pinned),
            toolbar::Command::Move(id, to) => browser.move_extension(&id, to),
            toolbar::Command::Manage => self.show_dialog(Dialog::Extensions),
        }
    }

    pub fn extension_toolbar(&self) -> &toolbar::Toolbar {
        &self.toolbar
    }

    fn show_popup(
        &self,
        anchor: &FrameworkElement,
        action: &ExtensionAction,
        activation: Activation,
    ) -> Result<Popup> {
        let browser = self.browser().ok_or_else(windows_core::Error::empty)?;
        let active = self.active_tab().map(|t| t.id);
        let opener: Vec<OpenerTab> = self
            .tabs_in_order()
            .iter()
            .map(|t| OpenerTab {
                url: t.session_url(),
                active: Some(t.id) == active,
            })
            .collect();
        popup::open(
            anchor,
            browser.engine().environment().clone(),
            action,
            activation,
            &opener,
        )
    }

    fn set_fullscreen(&self, on: bool) {
        if self.fullscreen.get() == on {
            return;
        }
        self.windowed_bounds
            .set(if on { self.bounds() } else { None });
        self.fullscreen.set(on);
        let chrome_visible = !on;
        let _ = xaml::set_visible(&self.ui.toolbar, chrome_visible);
        let _ = xaml::set_visible(
            &self.ui.bookmarks_bar,
            chrome_visible && self.bookmarks_bar_wanted.get(),
        );
        let _ = xaml::set_visible(&self.ui.update_bar, chrome_visible);
        if let Err(e) = self.show_layout(self.tabs_position.get()) {
            log::warn!("fullscreen layout: {e}");
        }
        let kind = if on {
            AppWindowPresenterKind::FullScreen
        } else {
            AppWindowPresenterKind::Overlapped
        };
        if let Err(e) = self.app_window().and_then(|w| w.SetPresenterByKind(kind)) {
            log::warn!("fullscreen: {e}");
        }
    }

    pub fn apply_backdrop(&self, backdrop: Backdrop) {
        set_backdrop(&self.window, backdrop);
    }

    pub fn apply_theme(&self, theme: Theme) {
        set_theme(&self.window, &self.ui.root, theme);
    }

    // ---- placement ----

    /// The window's bounds; one that is not restored gives the bounds it returns to.
    pub fn bounds(&self) -> Option<Bounds> {
        if let Some(windowed) = self.windowed_bounds.get() {
            return Some(windowed);
        }
        let app = self.app_window().ok()?;
        let state = app
            .Presenter()
            .and_then(|p| p.cast::<OverlappedPresenter>())
            .and_then(|p| p.State())
            .ok();
        let maximized = state == Some(OverlappedPresenterState::Maximized);
        // A maximized or minimized window is not where Restore Down puts it.
        let rect = if state.is_some_and(|s| s != OverlappedPresenterState::Restored) {
            platform::normal_bounds(platform::window_handle(&self.window).ok()?)?
        } else {
            let (position, size) = (app.Position().ok()?, app.Size().ok()?);
            RectInt32 {
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
            }
        };
        let width = u32::try_from(rect.width).ok()?;
        let height = u32::try_from(rect.height).ok()?;
        Some(((rect.x, rect.y, width, height), maximized))
    }

    fn apply_bounds(&self, (x, y, width, height): (i32, i32, u32, u32), maximized: bool) {
        let rect = RectInt32 {
            x,
            y,
            width: i32::try_from(width).unwrap_or(i32::MAX),
            height: i32::try_from(height).unwrap_or(i32::MAX),
        };
        let Ok(app) = self.app_window() else { return };
        let work_area = |rect, fallback| {
            DisplayArea::GetFromRect(rect, fallback)
                .and_then(|d| d.WorkArea())
                .ok()
        };
        let rect = match work_area(rect, DisplayAreaFallback::Nearest) {
            Some(nearest) => {
                let title_bar = RectInt32 {
                    height: rect.height.min(TITLE_BAR_HEIGHT),
                    ..rect
                };
                restored_rect(
                    rect,
                    work_area(title_bar, DisplayAreaFallback::None),
                    nearest,
                )
            }
            None => rect,
        };
        if rect.width >= 200
            && rect.height >= 150
            && let Err(e) = app.MoveAndResize(rect)
        {
            log::warn!("restore window bounds: {e}");
        }
        if maximized {
            let _ = app
                .Presenter()
                .and_then(|p| p.cast::<OverlappedPresenter>())
                .and_then(|p| p.Maximize());
        }
    }

    pub fn window_id(&self) -> Result<WindowId> {
        self.app_window()?.Id()
    }

    // ---- engine access and capture ----

    /// The WebView2 profile, once any tab's engine view exists.
    pub async fn engine_profile(&self) -> Option<CoreWebView2Profile> {
        let ready = exec::wait_for(
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(50),
            || self.tabs.borrow().iter().find_map(|t| t.core().cloned()),
        )
        .await?;
        ready
            .cast::<ICoreWebView2_13>()
            .and_then(|c| c.Profile())
            .ok()
    }

    /// The whole window, captured without activating it.
    pub async fn capture(&self) -> Result<capture::WindowShot> {
        capture::window_png(platform::window_handle(&self.window)?).await
    }
}

/// WebView2's Save As dialog for `tab`'s page. A cancel is the user's; anything else that is not
/// a save says so in the window.
async fn save_page(window: Weak<BrowserWindow>, tab: Rc<Tab>) {
    let failure = match tab.save_as().await {
        Ok(CoreWebView2SaveAsUIResult::Success) => {
            log::info!("tab {}: page saved", tab.id);
            return;
        }
        Ok(CoreWebView2SaveAsUIResult::Cancelled) => {
            log::info!("tab {}: saving the page was cancelled", tab.id);
            return;
        }
        Ok(CoreWebView2SaveAsUIResult::FileAlreadyExists) => "The file already exists.".to_owned(),
        Ok(CoreWebView2SaveAsUIResult::InvalidPath) => {
            "The file name or folder is not valid.".to_owned()
        }
        Ok(CoreWebView2SaveAsUIResult::KindNotSupported) => {
            "This page cannot be saved in that format.".to_owned()
        }
        Ok(other) => format!("WebView2 gave the unknown result {}.", other.0),
        Err(e) => e.message(),
    };
    log::warn!("tab {}: saving the page failed: {failure}", tab.id);
    if let Some(window) = window.upgrade() {
        window.show_failure("Could not save the page", &failure);
    }
}

fn hide(flyout: &Flyout) {
    let _ = flyout.cast::<FlyoutBase>().and_then(|f| f.Hide());
}

/// What `flyout` shows, while it is open.
fn open_content(flyout: &Flyout) -> Option<FrameworkElement> {
    let open = flyout
        .cast::<FlyoutBase>()
        .and_then(|f| f.IsOpen())
        .unwrap_or(false);
    open.then(|| flyout.Content().ok()?.cast().ok()).flatten()
}

/// How much of a restored window's title bar must be on a display for it to stay where it was
/// saved: enough to grab it with the mouse, in screen pixels.
const GRAB_WIDTH: i32 = 100;
const GRAB_HEIGHT: i32 = 16;
/// The part of a window's top edge treated as its title bar.
const TITLE_BAR_HEIGHT: i32 = 32;

/// Where a window saved at `saved` reopens. `title_work_area` is the work area of the display
/// under its title bar, if any display is; `nearest_work_area` that of the display nearest to it.
fn restored_rect(
    saved: RectInt32,
    title_work_area: Option<RectInt32>,
    nearest_work_area: RectInt32,
) -> RectInt32 {
    let grabbable = title_work_area.is_some_and(|area| {
        let left = saved.x.max(area.x);
        let right = saved.x.saturating_add(saved.width).min(area.x + area.width);
        let top = saved.y.max(area.y);
        let bottom = saved
            .y
            .saturating_add(TITLE_BAR_HEIGHT.min(saved.height))
            .min(area.y + area.height);
        right - left >= GRAB_WIDTH && bottom - top >= GRAB_HEIGHT
    });
    if grabbable {
        return saved;
    }
    let area = nearest_work_area;
    let width = saved.width.min(area.width);
    let height = saved.height.min(area.height);
    RectInt32 {
        x: saved.x.clamp(area.x, area.x + area.width - width),
        y: saved.y.clamp(area.y, area.y + area.height - height),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: i32, y: i32, width: i32, height: i32) -> RectInt32 {
        RectInt32 {
            x,
            y,
            width,
            height,
        }
    }

    const SCREEN: RectInt32 = rect(0, 0, 1920, 1040);

    #[test]
    fn the_site_icon_follows_the_engines_verdict() {
        let report = |state: &str| connection::Report {
            state: state.to_owned(),
            ..Default::default()
        };
        let broken = report("insecure-broken");
        assert_eq!(site_look("https://a.test/", Some(&broken)).1, "Not secure");
        let secure = report("secure");
        assert_eq!(
            site_look("https://a.test/", Some(&secure)).1,
            "Connection is secure"
        );
        assert_eq!(site_look("https://a.test/", None).1, "Not secure");
        assert_eq!(site_look("http://a.test/", None).1, "Not secure");
        assert_eq!(site_look("", None).1, "Search or enter web address");
        assert_eq!(
            site_look("file:///C:/a.html", None).1,
            "This page is on your device or inside the browser"
        );
    }

    #[test]
    fn blank_pages_are_never_starred() {
        let bookmarked = |_: &str| true;
        assert!(starred("https://e.test/", bookmarked));
        assert!(!starred("https://e.test/", |_| false));
        assert!(!starred("about:blank", bookmarked));
        assert!(!starred("", bookmarked));
    }

    #[test]
    fn a_window_on_a_display_stays_where_it_was() {
        let saved = rect(100, 100, 1280, 860);
        assert_eq!(restored_rect(saved, Some(SCREEN), SCREEN), saved);
    }

    #[test]
    fn a_window_whose_title_bar_can_still_be_grabbed_stays() {
        let saved = rect(1700, 200, 1280, 860);
        assert_eq!(restored_rect(saved, Some(SCREEN), SCREEN), saved);
    }

    #[test]
    fn a_window_from_a_disconnected_display_moves_onto_the_nearest() {
        let saved = rect(2600, 100, 1280, 860);
        assert_eq!(
            restored_rect(saved, None, SCREEN),
            rect(640, 100, 1280, 860)
        );
    }

    #[test]
    fn a_title_bar_barely_on_a_display_is_brought_onto_it() {
        let saved = rect(1880, 100, 1280, 860);
        assert_eq!(
            restored_rect(saved, Some(SCREEN), SCREEN),
            rect(640, 100, 1280, 860)
        );
        let above = rect(100, -600, 1280, 860);
        assert_eq!(restored_rect(above, None, SCREEN), rect(100, 0, 1280, 860));
    }

    #[test]
    fn a_window_larger_than_the_display_shrinks_to_its_work_area() {
        let saved = rect(-3000, -50, 2560, 1400);
        assert_eq!(restored_rect(saved, None, SCREEN), SCREEN);
    }

    #[test]
    fn work_areas_away_from_the_origin_are_respected() {
        let left = rect(-1280, 200, 1280, 984);
        let saved = rect(-5000, 0, 800, 600);
        assert_eq!(restored_rect(saved, None, left), rect(-1280, 200, 800, 600));
    }
}
