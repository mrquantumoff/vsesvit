//! Owned by W1.7.
//! The toolbar row (UA sections 1.5 and 2.1): the tab list's toggle, Back, Forward, Reload or Stop, a
//! plain address field and New Tab. The omnibox proper (suggestions, chips, the site info) is W2.2's;
//! until then the field shows the selected tab's address and hands back what was typed on Enter.
//! What was typed and not submitted stays with its tab while another tab is shown.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Icon};
use vsesvit_core::address::readable_url;
use vsesvit_core::private::Browsing;

use crate::app::actions::{Back, Forward, NewTab, Reload, Stop, ToggleTabList};
use crate::tab::Tab;
use crate::theme::icons::VsIcon;
use crate::theme::{private_chrome, tokens};

pub mod downloads_button;
pub mod main_menu;
pub mod profile_button;
pub mod update_banner;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarEvent {
    /// Enter in the address field, with its text.
    Submitted(String),
    /// The address field took the keyboard.
    AddressFocused,
}

pub struct Toolbar {
    browsing: Browsing,
    tab: Option<Entity<Tab>>,
    address: Entity<InputState>,
    /// The user typed since the field last showed the tab's address.
    edited: bool,
    /// The tab list's toggle shows where the list is: before the field, or after it on the right.
    sidebar_toggle: Option<SidebarSide>,
    _tab: Option<Subscription>,
    _address: Subscription,
}

/// Where the sidebar is, for its toggle's icon.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SidebarSide {
    Left,
    Right,
}

impl EventEmitter<ToolbarEvent> for Toolbar {}

impl Toolbar {
    pub fn new(browsing: Browsing, window: &mut Window, cx: &mut Context<Self>) -> Toolbar {
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter address"));
        let watch = cx.subscribe_in(&address, window, |this, address, event: &InputEvent, _, cx| match event {
            InputEvent::Change => this.edited = true,
            InputEvent::PressEnter { .. } => {
                let text = address.read(cx).value().to_string();
                this.edited = false;
                cx.emit(ToolbarEvent::Submitted(text));
            }
            InputEvent::Focus => cx.emit(ToolbarEvent::AddressFocused),
            InputEvent::Blur => {}
        });
        Toolbar { browsing, tab: None, address, edited: false, sidebar_toggle: None, _tab: None, _address: watch }
    }

    /// The window's selected tab changed. The old tab keeps what was typed for it; the field shows the
    /// new tab's typed text, else its address.
    pub fn show_tab(&mut self, tab: Option<Entity<Tab>>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(old) = self.tab.take()
            && self.edited
        {
            let typed = self.address.read(cx).value().to_string();
            old.update(cx, |old, _| old.set_typed(Some(typed)));
        }
        self.edited = false;
        self._tab = tab.as_ref().map(|tab| cx.observe_in(tab, window, |this, _, window, cx| this.tab_changed(window, cx)));
        self.tab = tab;
        let typed = self.tab.as_ref().and_then(|tab| tab.update(cx, |tab, _| tab.take_typed()));
        match typed {
            Some(text) => {
                self.set_text(text, window, cx);
                self.edited = true;
            }
            None => self.show_address(window, cx),
        }
        cx.notify();
    }

    /// Whether the field holds text typed for the shown tab and not submitted.
    pub fn has_typed_text(&self) -> bool {
        self.edited
    }

    /// Gives the field the keyboard with its text selected (Ctrl+L, a blank tab).
    pub fn focus_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.address.update(cx, |address, cx| {
            address.focus(window, cx);
            address.select_all(window, cx);
        });
    }

    /// The field shows the tab's address again, dropping what was typed (after a navigation started).
    pub fn reset_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edited = false;
        self.show_address(window, cx);
    }

    pub fn set_sidebar_toggle(&mut self, side: Option<SidebarSide>, cx: &mut Context<Self>) {
        if self.sidebar_toggle != side {
            self.sidebar_toggle = side;
            cx.notify();
        }
    }

    fn tab_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.edited {
            self.show_address(window, cx);
        }
        cx.notify();
    }

    fn show_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.tab.as_ref().and_then(|tab| tab.read(cx).link().map(|url| readable_url(url.as_str()))).unwrap_or_default();
        self.set_text(text, window, cx);
    }

    fn set_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.address.update(cx, |address, cx| {
            if address.value() != text.as_str() {
                address.set_value(text, window, cx);
            }
        });
    }
}

impl Render for Toolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let private = self.browsing == Browsing::Private;
        let (background, border, ink) = if private {
            let p = private_chrome();
            (p.background, p.border, p.foreground)
        } else {
            (theme.title_bar, theme.title_bar_border, theme.foreground)
        };
        let (can_back, can_forward, loading) =
            self.tab.as_ref().map(|t| t.read(cx)).map_or((false, false, false), |t| (t.can_go_back(), t.can_go_forward(), t.loading()));
        let hover = theme.list_hover;
        let field = div()
            .flex_1()
            .max_w(tokens::OMNIBOX_COMPACT_MAX_W)
            .h(tokens::OMNIBOX_H)
            .child(Input::new(&self.address).h(tokens::OMNIBOX_H).appearance(true).cleanable(false));
        let toggle = |side: SidebarSide| {
            let icon = match side {
                SidebarSide::Left => VsIcon::SidebarLeft,
                SidebarSide::Right => VsIcon::SidebarRight,
            };
            button("toggle-tab-list", icon, true, hover, ToggleTabList.boxed_clone())
        };
        div()
            .id("toolbar")
            .h(tokens::TOOLBAR_H)
            .flex_none()
            .flex()
            .items_center()
            .gap(tokens::TOOLBAR_SPACING)
            .px(tokens::TOOLBAR_EDGE)
            .bg(background)
            .text_color(ink)
            .border_b_1()
            .border_color(border)
            .when(self.sidebar_toggle == Some(SidebarSide::Left), |bar| bar.child(toggle(SidebarSide::Left)))
            .child(button("back", VsIcon::Back, can_back, hover, Back.boxed_clone()))
            .child(button("forward", VsIcon::Forward, can_forward, hover, Forward.boxed_clone()))
            .child(if loading {
                button("stop", VsIcon::Stop, true, hover, Stop.boxed_clone())
            } else {
                button("reload", VsIcon::Reload, self.tab.is_some(), hover, Reload.boxed_clone())
            })
            .child(div().flex_1().flex().justify_center().child(field))
            .child(button("new-tab", VsIcon::NewTab, true, hover, NewTab.boxed_clone()))
            .when(self.sidebar_toggle == Some(SidebarSide::Right), |bar| bar.child(toggle(SidebarSide::Right)))
    }
}

/// A toolbar button that dispatches `action` to the window; dimmed and inert when not `enabled`.
fn button(id: &'static str, icon: VsIcon, enabled: bool, hover: gpui::Hsla, action: Box<dyn Action>) -> impl IntoElement {
    div()
        .id(SharedString::from(id))
        .size(tokens::TOOLBAR_BUTTON)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .when(!enabled, |b| b.opacity(0.4))
        .when(enabled, |b| {
            b.hover(move |b| b.bg(hover)).on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        })
        .child(Icon::new(icon).size(tokens::TOOLBAR_ICON))
}
