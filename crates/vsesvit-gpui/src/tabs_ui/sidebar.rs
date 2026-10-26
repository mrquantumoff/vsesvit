//! Owned by W1.7.
//! The vertical tab list (UA section 1.5, left and right layouts): a row per tab, pinned ones first,
//! and New Tab at the bottom. Tab search, groups, the tab menu and drag reordering are W2's.

use gpui::prelude::FluentBuilder as _;
use gpui::{Action as _, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, StatefulInteractiveElement as _, Styled as _, div, px};
use gpui_component::{Icon, Theme};

use super::row::{self, RowActions, TabRow};
use crate::app::actions::NewTab;
use crate::theme::icons::VsIcon;
use crate::theme::tokens;

/// Which edge of the window the sidebar is on: its border faces the page.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
}

pub fn render(rows: Vec<TabRow>, width: Pixels, edge: Edge, actions: &RowActions, theme: &Theme) -> impl IntoElement {
    let hover = theme.list_hover;
    div()
        .id("tab-sidebar")
        .w(width)
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap_0p5()
        .py_1()
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .border_color(theme.border)
        .map(|d| match edge {
            Edge::Left => d.border_r_1(),
            Edge::Right => d.border_l_1(),
        })
        .child(
            div()
                .id("tab-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_0p5()
                .children(rows.iter().map(|r| tab_row(r, actions, theme))),
        )
        .child(
            div()
                .id("sidebar-new-tab")
                .m(tokens::SIDEBAR_BUTTON_MARGIN)
                .h(tokens::SIDEBAR_BUTTON_H)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .gap_2()
                .rounded(px(6.))
                .hover(move |b| b.bg(hover))
                .on_click(|_, window, cx| window.dispatch_action(NewTab.boxed_clone(), cx))
                .child(Icon::new(VsIcon::NewTab).size(tokens::FAVICON))
                .child("New Tab"),
        )
}

fn tab_row(row: &TabRow, actions: &RowActions, theme: &Theme) -> impl IntoElement {
    let hover = theme.list_hover;
    row::clickable(row, actions, "sidebar-tab")
        .relative()
        .min_h(tokens::TAB_ROW_MIN_H)
        .pt(tokens::TAB_ROW_PADDING.top)
        .pr(tokens::TAB_ROW_PADDING.right)
        .pb(tokens::TAB_ROW_PADDING.bottom)
        .pl(tokens::TAB_ROW_PADDING.left)
        .mx_1()
        .flex()
        .items_center()
        .gap(tokens::TAB_ROW_SPACING)
        .rounded(px(6.))
        .when(row.selected, |r| r.bg(theme.sidebar_accent))
        .when(!row.selected, |r| r.hover(move |r| r.bg(hover)))
        .child(row::icon(row))
        .child(div().flex_1().min_w_0().truncate().child(row.title.clone()))
        .when(row.pinned, |r| r.child(Icon::new(VsIcon::Pin).size(tokens::TAB_ROW_PIN).text_color(theme.muted_foreground)))
        .when(!row.pinned, |r| r.child(row::close_button(row, actions, hover)))
}

