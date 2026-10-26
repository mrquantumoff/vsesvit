//! Owned by W1.7.
//! The horizontal tab strip of the top layout (UA section 1.5): pinned tabs as icons, then a tab per
//! page that shrinks from its widest to its narrowest as the strip fills, and New Tab after them.
//! Group chips, tab search, the tab menu and dragging are W2's.

use gpui::prelude::FluentBuilder as _;
use gpui::{Action as _, InteractiveElement as _, IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, div, px};
use gpui_component::{Icon, Theme};

use super::row::{self, RowActions, TabRow};
use crate::app::actions::NewTab;
use crate::theme::icons::VsIcon;
use crate::theme::tokens;

pub fn render(rows: Vec<TabRow>, actions: &RowActions, theme: &Theme) -> impl IntoElement {
    let hover = theme.list_hover;
    div()
        .id("tab-strip")
        .h(tokens::TAB_STRIP_H)
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap(tokens::TAB_GAP)
        .px_1()
        .bg(theme.tab_bar)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(tokens::TAB_GAP)
                .overflow_hidden()
                .children(rows.iter().map(|r| strip_tab(r, actions, theme))),
        )
        .child(
            div()
                .id("strip-new-tab")
                .size(tokens::TOOLBAR_BUTTON)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .hover(move |b| b.bg(hover))
                .on_click(|_, window, cx| window.dispatch_action(NewTab.boxed_clone(), cx))
                .child(Icon::new(VsIcon::NewTab).size(tokens::TOOLBAR_ICON)),
        )
}

fn strip_tab(row: &TabRow, actions: &RowActions, theme: &Theme) -> impl IntoElement {
    let hover = theme.list_hover;
    let tab = row::clickable(row, actions, "strip-tab")
        .h(tokens::TAB_H)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .rounded(px(6.))
        .text_color(if row.selected { theme.tab_active_foreground } else { theme.tab_foreground })
        .when(row.selected, |t| t.bg(theme.tab_active))
        .when(!row.selected, |t| t.hover(move |t| t.bg(hover)))
        .child(row::icon(row));
    if row.pinned {
        return tab.w(tokens::TAB_PINNED_W).flex_none().justify_center();
    }
    tab.flex_1()
        .min_w(tokens::TAB_MIN_W)
        .max_w(tokens::TAB_MAX_W)
        .child(div().flex_1().min_w_0().truncate().child(row.title.clone()))
        .child(row::close_button(row, actions, hover))
}
