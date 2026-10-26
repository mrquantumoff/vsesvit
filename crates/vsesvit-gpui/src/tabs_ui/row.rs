//! Owned by W1.7.
//! One tab as the sidebar and the strip show it: its icon (the page's, or a globe), its title and a
//! close button. A click selects the tab, a middle click or the close button closes it. Indicators,
//! groups, the tab menu and dragging are W2's.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, Hsla, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, img, px};
use gpui_component::Icon;
use vsesvit_engine::TabId;

use crate::tab::TabIcon;
use crate::theme::icons::VsIcon;
use crate::theme::tokens;

/// What a row shows, read from its tab and the window's list.
#[derive(Clone, Debug, PartialEq)]
pub struct TabRow {
    pub id: TabId,
    pub title: SharedString,
    pub icon: TabIcon,
    pub loading: bool,
    pub selected: bool,
    pub pinned: bool,
}

/// What a click does to a row's tab.
pub type OnTab = Rc<dyn Fn(TabId, &mut Window, &mut App)>;

/// What the rows' clicks do; the window builds them.
#[derive(Clone)]
pub struct RowActions {
    pub select: OnTab,
    pub close: OnTab,
}

/// The row's icon: the page's favicon, else a globe, faded while the page loads.
pub fn icon(row: &TabRow) -> AnyElement {
    let icon = match &row.icon {
        TabIcon::Page(image) => img(image.clone()).size(tokens::FAVICON).flex_none().into_any_element(),
        TabIcon::Default => Icon::new(VsIcon::Globe).size(tokens::FAVICON).into_any_element(),
    };
    div().flex_none().when(row.loading, |d| d.opacity(0.5)).child(icon).into_any_element()
}

/// The close button, which closes the tab without selecting it.
pub fn close_button(row: &TabRow, actions: &RowActions, hover: Hsla) -> impl IntoElement {
    let (id, close) = (row.id, actions.close.clone());
    div()
        .id(("tab-close", u64::from(id.get())))
        .size(tokens::TAB_CLOSE)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .opacity(tokens::TAB_ROW_CLOSE_OPACITY)
        .hover(move |b| b.opacity(1.).bg(hover))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            close(id, window, cx);
        })
        .child(Icon::new(VsIcon::Close).size(tokens::FAVICON))
}

/// What every row does with the mouse: a press selects, a middle press closes.
pub fn clickable(row: &TabRow, actions: &RowActions, name: &'static str) -> gpui::Stateful<gpui::Div> {
    let (id, select, close) = (row.id, actions.select.clone(), actions.close.clone());
    div()
        .id((name, u64::from(id.get())))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| select(id, window, cx))
        .on_mouse_down(MouseButton::Middle, move |_, window, cx| close(id, window, cx))
}
