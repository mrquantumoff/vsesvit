//! Owned by W1.7.
//! A browser window (UA sections 1.4 to 1.6): the root view that lays out the toolbar, the tab list
//! and the page slot, hosts its tabs' pages in its platform surface, and reports its life to the
//! browser state and the focus policy. The state asks for a window (`BrowserStateEvent::OpenWindow`);
//! [`install`] opens it. Pages opened from pages (popups, middle clicks) and keys routed back from a
//! page arrive here from the router.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, WeakEntity, Window, canvas, div,
};
use gpui_component::ActiveTheme as _;
use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use vsesvit_core::profiles;
use vsesvit_engine::{CloseMode, InitialPage, NativeHandle, OpenDisposition, RoutedKey, TabId, Url};

use crate::app::shell::Shell;
use crate::engine_bridge::{self, SharedSurface, keymap};
use crate::platform::{PlatformError, SurfaceOps};
use crate::state::{BrowserState, BrowserStateEvent, WindowId};
use crate::tab::model::{Insert, TabListEvent, TabListModel};
use crate::tab::{Tab, TabChanged};
use crate::tabs_ui::{self, RowActions, TabRow};
use crate::toolbar::{SidebarSide, Toolbar, ToolbarEvent};

pub mod actions;
pub mod close;
pub mod frame;
pub mod fullscreen;
pub mod layout;
pub mod page_area;

#[cfg(test)]
mod tests;

use layout::{Layout, LayoutProbe};
use page_area::PageSlot;

/// Brings a window forward for a forwarded launch, with that launch's startup id.
pub type Raise = Rc<dyn Fn(&Window, Option<&str>)>;

/// What the windows need from the platform, so tests can give them fakes. Boot installs the real one.
pub struct WindowHost {
    /// Creates a window's content host: `platform::attach`, or `fake::attach` in tests.
    pub attach: fn(&Window) -> Result<Box<dyn SurfaceOps>, PlatformError>,
    /// The window's own native handle, as `Desktop::toplevel_at` names it.
    pub toplevel: fn(&Window) -> Option<NativeHandle>,
    pub raise: Raise,
    /// `platform::Surface::TRANSPARENT_CONTENT`: the window's background stays transparent and nothing
    /// paints the page's rectangle.
    pub transparent: bool,
}

impl Global for WindowHost {}

impl WindowHost {
    /// The real platform, with `desktop` for raising windows.
    #[cfg(any(target_os = "linux", windows))]
    pub fn native(desktop: Rc<crate::platform::Os>) -> WindowHost {
        use crate::platform::{Desktop as _, NativeSurface as _, Surface};
        WindowHost {
            attach: crate::platform::attach,
            toplevel: native_toplevel,
            raise: Rc::new(move |window, token| desktop.raise_for_invocation(window, token)),
            transparent: Surface::TRANSPARENT_CONTENT,
        }
    }
}

#[cfg(target_os = "linux")]
fn native_toplevel(window: &Window) -> Option<NativeHandle> {
    crate::platform::x11::window_xid(window).ok().map(NativeHandle::from_xid)
}

#[cfg(windows)]
fn native_toplevel(window: &Window) -> Option<NativeHandle> {
    crate::platform::windows::window_hwnd(window).ok().map(|hwnd| NativeHandle::from_hwnd(hwnd.cast()))
}

/// Opens a browser window whenever the state asks for one, raises one for a forwarded launch, and
/// keeps the popup-blocking pref in the hooks' snapshot.
pub fn install(state: &Entity<BrowserState>, host: WindowHost, cx: &mut App) {
    cx.set_global(host);
    publish_block_popups(cx);
    cx.subscribe(state, |_, event, cx| match event {
        BrowserStateEvent::OpenWindow(id) => open(*id, cx),
        BrowserStateEvent::RaiseWindow { window, startup_id } => raise(*window, startup_id.as_deref(), cx),
        BrowserStateEvent::PrefsChanged(changed) if changed.iter().any(|k| k == keys::BLOCK_POPUPS.key) => publish_block_popups(cx),
        _ => {}
    })
    .detach();
}

fn publish_block_popups(cx: &App) {
    if !engine_bridge::running(cx) {
        return;
    }
    let block = Shell::core(cx).borrow_mut().prefs().get(&keys::BLOCK_POPUPS);
    if let Ok(mut snapshot) = engine_bridge::hooks::snapshot(cx).try_borrow_mut() {
        snapshot.block_popups = block;
    }
}

/// Opens the GPUI window for the state's window `id`, with its content host. A window that cannot
/// host pages is useless: it closes its tabs and goes, and the browser quits if it was the only one.
fn open(id: WindowId, cx: &mut App) {
    let state = Shell::state(cx);
    let Some((browsing, tabs, geometry)) = state.read(cx).window(id).map(|w| (w.browsing, w.tabs.clone(), w.geometry)) else {
        return;
    };
    let host = cx.global::<WindowHost>();
    let (attach, transparent) = (host.attach, host.transparent);
    let options = frame::options(geometry, transparent, cx);
    let opened = cx.open_window(options, |window, cx| {
        crate::theme::follow_desktop(window);
        let surface = attach(window);
        cx.new(|cx| BrowserWindow::new(id, browsing, tabs, surface, window, cx))
    });
    match opened {
        Ok(handle) => state.update(cx, |state, _| state.window_attached(id, handle.into())),
        Err(e) => {
            log::error!("window {}: GPUI did not open it: {e}", id.0);
            state.update(cx, |state, cx| state.window_closed(id, cx));
        }
    }
}

fn raise(id: WindowId, token: Option<&str>, cx: &mut App) {
    let Some(handle) = Shell::state(cx).read(cx).window(id).and_then(|w| w.handle) else { return };
    let raise = cx.global::<WindowHost>().raise.clone();
    let _ = handle.update(cx, |_, window, _| raise(window, token));
}

pub struct BrowserWindow {
    id: WindowId,
    browsing: Browsing,
    state: WeakEntity<BrowserState>,
    tabs: Entity<TabListModel>,
    toolbar: Entity<Toolbar>,
    focus: FocusHandle,
    /// The content host every page of this window lives in; `None` once detached, or if it could not
    /// be made.
    surface: Option<SharedSurface>,
    gpui_id: gpui::WindowId,
    toplevel: Option<NativeHandle>,
    transparent: bool,
    layout: Layout,
    page: PageSlot,
    probe: LayoutProbe,
    title: String,
    /// Every page of the window closed and the window went.
    closed: bool,
    watched: HashMap<TabId, [Subscription; 2]>,
    _subscriptions: Vec<Subscription>,
}

impl BrowserWindow {
    fn new(
        id: WindowId,
        browsing: Browsing,
        tabs: Entity<TabListModel>,
        surface: Result<Box<dyn SurfaceOps>, PlatformError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> BrowserWindow {
        let state = Shell::state(cx);
        let host = cx.global::<WindowHost>();
        let (toplevel, transparent) = ((host.toplevel)(window), host.transparent);
        let gpui_id = window.window_handle().window_id();
        let surface = match surface {
            Ok(surface) => Some(Rc::new(std::cell::RefCell::new(surface))),
            Err(e) => {
                crate::platform::fatal_message(&format!("vsesvit: a window cannot show pages: {e}"));
                None
            }
        };
        if let Some(surface) = &surface {
            engine_bridge::focus(cx).add_window(gpui_id, surface.clone());
        }
        let position = Shell::core(cx).borrow_mut().prefs().get(&keys::TABS_POSITION);
        let toolbar = cx.new(|cx| Toolbar::new(browsing, window, cx));
        let this = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            this.update(cx, |this, cx| this.should_close(window, cx)).unwrap_or(true)
        });
        let subscriptions = vec![
            cx.subscribe_in(&tabs, window, Self::list_changed),
            cx.subscribe_in(&toolbar, window, Self::toolbar_event),
            cx.subscribe_in(&state, window, Self::state_event),
            cx.observe_window_activation(window, Self::activation_changed),
            cx.observe_window_bounds(window, Self::bounds_changed),
        ];
        let mut this = BrowserWindow {
            id,
            browsing,
            state: state.downgrade(),
            tabs: tabs.clone(),
            toolbar,
            focus: cx.focus_handle(),
            surface,
            gpui_id,
            toplevel,
            transparent,
            layout: Layout::new(position),
            page: PageSlot::default(),
            probe: LayoutProbe::default(),
            title: String::new(),
            closed: false,
            watched: HashMap::new(),
            _subscriptions: subscriptions,
        };
        let focus = engine_bridge::focus(cx);
        for tab in tabs.read(cx).tabs().to_vec() {
            this.watch(tab, cx);
            focus.join(gpui_id, tab);
        }
        this.sync_toolbar_toggle(cx);
        let selected = tabs.read(cx).selected();
        this.selection_changed(selected, window, cx);
        this.activation_changed(window, cx);
        this
    }

    pub fn id(&self) -> WindowId {
        self.id
    }

    pub fn tabs(&self) -> &Entity<TabListModel> {
        &self.tabs
    }

    /// Where the tab widgets and the page were last laid out, for the self-test's `tab_layout`.
    pub fn layout_probe(&self) -> LayoutProbe {
        self.probe
    }

    pub fn page_slot(&self) -> &PageSlot {
        &self.page
    }

    /// The window's surface, for the self-test's screenshot.
    pub fn surface(&self) -> Option<SharedSurface> {
        self.surface.clone()
    }

    fn selected_tab(&self, cx: &App) -> Option<Entity<Tab>> {
        let id = self.tabs.read(cx).selected()?;
        self.state.upgrade()?.read(cx).tab(id)
    }

    /// Redraws when the tab changes, and follows its focus requests.
    fn watch(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(tab) = self.state.upgrade().and_then(|s| s.read(cx).tab(id)) else { return };
        let observe = cx.observe(&tab, |_, _, cx| cx.notify());
        let changes = cx.subscribe(&tab, move |this, _, change, cx| {
            if let TabChanged::TakeFocus { next } = change
                && this.tabs.read(cx).selected() == Some(id)
            {
                this.page_gave_up_keyboard(*next, cx);
            }
        });
        self.watched.insert(id, [observe, changes]);
        self.page.tabs_changed();
    }

    fn list_changed(&mut self, _: &Entity<TabListModel>, event: &TabListEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            TabListEvent::Inserted { tab, .. } => {
                self.watch(*tab, cx);
                engine_bridge::focus(cx).join(self.gpui_id, *tab);
            }
            TabListEvent::Removed { tab } => {
                self.watched.remove(tab);
                page_area::forget_hook(*tab, cx);
                self.close_if_empty(window, cx);
            }
            TabListEvent::Selected { new, .. } => self.selection_changed(*new, window, cx),
            TabListEvent::OpenTabRequested => self.open_new_tab(window, cx),
            TabListEvent::Moved { .. } | TabListEvent::PinnedChanged { .. } | TabListEvent::GroupsChanged => {}
        }
        cx.notify();
    }

    /// Another tab shows: the focus policy follows, the toolbar shows the tab, and the keyboard goes
    /// to the address field for a blank tab or one with typed text, else to the page.
    fn selection_changed(&mut self, selected: Option<TabId>, window: &mut Window, cx: &mut Context<Self>) {
        engine_bridge::focus(cx).select_tab(self.gpui_id, selected);
        let tab = self.selected_tab(cx);
        self.toolbar.update(cx, |toolbar, cx| toolbar.show_tab(tab.clone(), window, cx));
        let Some(tab) = tab else { return };
        tab.update(cx, |tab, _| tab.mark_active(vsesvit_core::session::plan::now_ms()));
        let to_address = tab.read(cx).is_blank() || self.toolbar.read(cx).has_typed_text();
        if to_address {
            self.address_takes_keyboard(window, cx);
        } else {
            self.page_takes_keyboard(tab.read(cx).id(), window, cx);
        }
    }

    /// The address field gets the keyboard, whether or not the window is active yet.
    pub(super) fn address_takes_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        engine_bridge::focus(cx).chrome_wants_keyboard(self.gpui_id);
        self.toolbar.update(cx, |toolbar, cx| toolbar.focus_address(window, cx));
    }

    /// The selected page gets the keyboard; GPUI's own focus rests on the window, so the address field
    /// shows its resting text.
    fn page_takes_keyboard(&mut self, tab: TabId, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        engine_bridge::focus(cx).page_wants_keyboard(tab);
    }

    /// The page tabbed past its last element (`next`) or its first: the address field takes the keyboard,
    /// or the window's own focus does.
    fn page_gave_up_keyboard(&mut self, next: bool, cx: &mut Context<Self>) {
        let (toolbar, focus) = (self.toolbar.clone(), self.focus.clone());
        let Some(handle) = self.handle(cx) else { return };
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                if next {
                    toolbar.update(cx, |toolbar, cx| toolbar.focus_address(window, cx));
                } else {
                    window.focus(&focus, cx);
                }
            });
        });
    }

    fn handle(&self, cx: &App) -> Option<AnyWindowHandle> {
        self.state.upgrade()?.read(cx).window(self.id)?.handle
    }

    fn toolbar_event(&mut self, _: &Entity<Toolbar>, event: &ToolbarEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ToolbarEvent::AddressFocused => engine_bridge::focus(cx).chrome_wants_keyboard(self.gpui_id),
            ToolbarEvent::Submitted(text) => self.navigate_to_typed(text, window, cx),
        }
    }

    /// Enter in the address field: what was typed loads in the selected tab, which takes the keyboard.
    fn navigate_to_typed(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.selected_tab(cx) else { return };
        let target = Shell::core(cx).borrow_mut().omnibox().resolve(text);
        let url = match target {
            Ok(Some(target)) => target.url().clone(),
            Ok(None) => return,
            Err(e) => {
                log::warn!("the address field: {e}");
                return;
            }
        };
        tab.update(cx, |tab, cx| tab.load(url, Some(vsesvit_core::history::Transition::Typed), cx));
        self.toolbar.update(cx, |toolbar, cx| toolbar.reset_address(window, cx));
        self.page_takes_keyboard(tab.read(cx).id(), window, cx);
    }

    fn state_event(&mut self, _: &Entity<BrowserState>, event: &BrowserStateEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event {
            BrowserStateEvent::PrefsChanged(changed) if changed.iter().any(|k| k == keys::TABS_POSITION.key) => {
                self.layout.position = Shell::core(cx).borrow_mut().prefs().get(&keys::TABS_POSITION);
                self.sync_toolbar_toggle(cx);
                cx.notify();
            }
            BrowserStateEvent::ProfilesChanged => cx.notify(),
            _ => {}
        }
    }

    fn sync_toolbar_toggle(&mut self, cx: &mut Context<Self>) {
        let side = match self.layout.position {
            vsesvit_core::prefs::TabsPosition::Left => Some(SidebarSide::Left),
            vsesvit_core::prefs::TabsPosition::Right => Some(SidebarSide::Right),
            vsesvit_core::prefs::TabsPosition::Top => None,
        };
        self.toolbar.update(cx, |toolbar, cx| toolbar.set_sidebar_toggle(side, cx));
    }

    /// GPUI's activation changed: the focus policy follows, and an activated window goes first in the
    /// state's order.
    fn activation_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = window.is_window_active();
        engine_bridge::focus(cx).window_activated(self.gpui_id, active);
        if active && let Some(state) = self.state.upgrade() {
            let id = self.id;
            cx.defer(move |cx| state.update(cx, |state, cx| state.window_activated(id, cx)));
        }
    }

    /// The window moved or resized: the session keeps its frame, and the page's place on the screen is
    /// laid out again.
    fn bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bounds = window.bounds();
        let geometry = crate::state::WindowGeometry {
            bounds: Some((
                f32::from(bounds.origin.x).round() as i32,
                f32::from(bounds.origin.y).round() as i32,
                f32::from(bounds.size.width).round() as u32,
                f32::from(bounds.size.height).round() as u32,
            )),
            maximized: window.is_maximized(),
        };
        if let Some(state) = self.state.upgrade() {
            let id = self.id;
            cx.defer(move |cx| state.update(cx, |state, cx| state.set_window_geometry(id, geometry, cx)));
        }
        if let Some(tab) = self.tabs.read(cx).selected() {
            Shell::engine(cx).notify_moved(tab);
        }
        cx.notify();
    }

    /// The window's title, as Chrome forms it from the selected tab's.
    fn window_title(&self, cx: &App) -> String {
        let page = self.selected_tab(cx).map(|tab| tab.read(cx).display_title().to_string()).unwrap_or_default();
        let profile = self.state.upgrade().and_then(|s| s.read(cx).profile_title());
        profiles::window_title(&page, profile.as_deref(), self.browsing)
    }

    fn rows(&self, cx: &App) -> Vec<TabRow> {
        let Some(state) = self.state.upgrade() else { return Vec::new() };
        let model = self.tabs.read(cx);
        model
            .tabs()
            .iter()
            .enumerate()
            .filter_map(|(index, &id)| {
                let tab = state.read(cx).tab(id)?;
                let tab = tab.read(cx);
                Some(TabRow {
                    id,
                    title: tab.display_title(),
                    icon: tab.icon(),
                    loading: tab.loading(),
                    selected: model.selected() == Some(id),
                    pinned: index < model.pinned_count(),
                })
            })
            .collect()
    }

    fn row_actions(&self, cx: &Context<Self>) -> RowActions {
        let (select, close) = (cx.entity().downgrade(), cx.entity().downgrade());
        RowActions {
            select: Rc::new(move |tab, _, cx| {
                let _ = select.update(cx, |this, cx| this.tabs.update(cx, |model, cx| model.select(tab, cx)));
            }),
            close: Rc::new(move |tab, _, cx| {
                let _ = close.update(cx, |this, cx| this.close_tab(tab, cx));
            }),
        }
    }

    /// Records where a tab widget was laid out, for the layout probe.
    fn track(&self, region: fn(&mut LayoutProbe) -> &mut Option<gpui::Bounds<gpui::Pixels>>, cx: &Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        canvas(
            move |bounds, _, cx| {
                let _ = this.update(cx, |this, _| *region(&mut this.probe) = Some(bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

impl Render for BrowserWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.window_title(cx);
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        let width = window.viewport_size().width;
        self.probe = LayoutProbe { width: width.into(), ..LayoutProbe::default() };
        let sidebar = self.layout.sidebar_width(width);
        let top = self.layout.position == vsesvit_core::prefs::TabsPosition::Top;
        let right = self.layout.position == vsesvit_core::prefs::TabsPosition::Right;
        let rows = self.rows(cx);
        let actions = self.row_actions(cx);
        let theme = cx.theme().clone();
        let fill = (!self.transparent).then_some(theme.background);
        let page = page_area::element(cx.entity().downgrade(), fill);
        let edge = if right { tabs_ui::sidebar::Edge::Right } else { tabs_ui::sidebar::Edge::Left };
        let side = sidebar.map(|width| {
            div().relative().h_full().child(tabs_ui::sidebar::render(rows.clone(), width, edge, &actions, &theme)).child(self.track(|p| &mut p.sidebar, cx))
        });
        let strip = top.then(|| div().relative().child(tabs_ui::strip::render(rows, &actions, &theme)).child(self.track(|p| &mut p.tab_strip, cx)));
        let root = div()
            .id("browser-window")
            .size_full()
            .flex()
            .flex_col()
            .key_context(keymap::CONTEXT)
            .track_focus(&self.focus)
            .text_color(theme.foreground)
            .when(!self.transparent, |d| d.bg(theme.background))
            .child(self.toolbar.clone())
            .when_some(strip, |d, strip| d.child(strip))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .map(|d| if right { d.flex_row_reverse() } else { d.flex_row() })
                    .when_some(side, |d, side| d.child(side))
                    .child(page),
            );
        actions::register(root, cx)
    }
}

/// A page asked for a popup and the hooks allowed it as a tab: the engine created its browser, and
/// the popup becomes a selected tab next to its opener, in the opener's window (UA section 2.1).
pub fn adopt_popup(opener: TabId, tab: TabId, url: Url, native: Option<NativeHandle>, disposition: OpenDisposition, cx: &mut App) {
    let state = Shell::state(cx);
    let found = state.read(cx).tab(opener).map(|t| t.read(cx).browsing()).zip(state.read(cx).window_of(opener));
    let Some((browsing, window)) = found else {
        log::warn!("popup tab {} has no opener tab {}; closing it", tab.get(), opener.get());
        Shell::engine(cx).close_tab(tab, CloseMode::Force);
        return;
    };
    let select = disposition != OpenDisposition::NewBackgroundTab;
    state.update(cx, |state, cx| {
        let entity = state.register_tab(Tab::popup(tab, browsing, opener, url, native), cx);
        state.attach_tab(window, &entity, Insert::AfterOpener(opener), select, cx);
    });
}

/// A page asked for a URL elsewhere (a middle click, Ctrl+click): a tab next to the opener, selected
/// unless it was asked for in the background. A new window or a private one is asked for as such.
pub fn open_from_page(opener: TabId, url: Url, disposition: OpenDisposition, cx: &mut App) {
    let state = Shell::state(cx);
    let found = state.read(cx).tab(opener).map(|t| t.read(cx).browsing()).zip(state.read(cx).window_of(opener));
    let Some((browsing, window)) = found else { return };
    state.update(cx, |state, cx| match disposition {
        OpenDisposition::NewWindow | OpenDisposition::NewPopup => drop(state.open_window(browsing, vec![url], cx)),
        OpenDisposition::OffTheRecord => drop(state.open_window(Browsing::Private, vec![url], cx)),
        OpenDisposition::CurrentTab => {
            if let Some(tab) = state.tab(opener) {
                tab.update(cx, |tab, cx| tab.load(url, None, cx));
            }
        }
        OpenDisposition::NewForegroundTab | OpenDisposition::NewBackgroundTab | OpenDisposition::SaveToDisk => {
            let tab = state.new_tab_entity(browsing, InitialPage::Url(url), Some(opener), cx);
            let select = disposition == OpenDisposition::NewForegroundTab;
            state.attach_tab(window, &tab, Insert::AfterOpener(opener), select, cx);
        }
    });
}

/// A routed key arrived while a page had the keyboard: its action runs in the page's window, or in
/// the most recently active one.
pub fn run_shortcut(tab: Option<TabId>, key: RoutedKey, cx: &mut App) {
    let state = Shell::state(cx);
    let window = tab.and_then(|tab| state.read(cx).window_of(tab)).or_else(|| state.read(cx).windows().first().map(|w| w.id));
    let Some(handle) = window.and_then(|w| state.read(cx).window(w)).and_then(|w| w.handle) else { return };
    let action = keymap::action_for(key);
    let _ = handle.update(cx, |_, window, cx| window.dispatch_action(action, cx));
}
