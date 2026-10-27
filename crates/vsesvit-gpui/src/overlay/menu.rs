//! Owned by W2.1.
//! Menus (UA section 1.7): the main, profile, extension and tab menus and the page's context menu
//! (design section 1.5, built from the engine's `MenuModel` by W2.5). A [`Menu`] is plain data; the
//! host shows it with [`OverlayHost::open_menu`]. Each submenu level is a child overlay beside the
//! row that opened it, so the menu's holes cover its panels and nothing between them.
//!
//! Mouse: hovering a row highlights it and opens or closes submenus; a click runs a command. Keys:
//! Up, Down, Home and End move the highlight over the rows that can be chosen; Right or Enter open a
//! submenu and move into it; Left and Escape close a submenu level, Escape on the first level closes
//! the menu; Enter and Space run the highlighted command. A chosen command closes the whole menu
//! first, then runs, so the keyboard is already back with its owner when an action dispatches.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, canvas, div, px,
};
use gpui_component::{ActiveTheme as _, Icon};

use super::inline::WINDOW_MARGIN;
use super::{Anchor, DismissReason, OnClose, WindowAction, OverlayHost, OverlayId, OverlayKind, OverlaySpec, Placement};
use crate::theme::icons::VsIcon;
use crate::theme::tokens;

/// A menu: rows in order, separators between groups.
#[derive(Clone, Default)]
pub struct Menu {
    pub items: Vec<MenuItem>,
}

impl Menu {
    pub fn new() -> Menu {
        Menu::default()
    }

    pub fn item(mut self, item: impl Into<MenuItem>) -> Menu {
        self.items.push(item.into());
        self
    }

    pub fn separator(mut self) -> Menu {
        self.items.push(MenuItem::Separator);
        self
    }

    pub fn submenu(mut self, label: impl Into<SharedString>, menu: Menu) -> Menu {
        self.items.push(MenuItem::Submenu { label: label.into(), icon: None, enabled: true, menu });
        self
    }
}

/// Draws a custom row.
pub type DrawRow = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

#[derive(Clone)]
pub enum MenuItem {
    Command(Command),
    Submenu { label: SharedString, icon: Option<VsIcon>, enabled: bool, menu: Menu },
    Separator,
    /// A row the owner draws itself (the main menu's zoom row). The keyboard skips it.
    Custom(DrawRow),
}

impl From<Command> for MenuItem {
    fn from(command: Command) -> MenuItem {
        MenuItem::Command(command)
    }
}

/// A row that does something.
#[derive(Clone)]
pub struct Command {
    pub label: SharedString,
    pub icon: Option<VsIcon>,
    pub enabled: bool,
    pub check: Check,
    /// The accelerator shown at the row's end. `None` shows the key bound to the action, if any.
    pub hint: Option<SharedString>,
    pub run: MenuAction,
}

impl Command {
    /// A row that dispatches `action` to the window's focused element once the menu has closed.
    pub fn action(label: impl Into<SharedString>, action: impl Action) -> Command {
        Command::new(label, MenuAction::Action(Box::new(action)))
    }

    /// A row that calls `run` once the menu has closed.
    pub fn run(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Command {
        Command::new(label, MenuAction::Run(Rc::new(run)))
    }

    pub fn new(label: impl Into<SharedString>, run: MenuAction) -> Command {
        Command { label: label.into(), icon: None, enabled: true, check: Check::None, hint: None, run }
    }

    pub fn enabled(mut self, enabled: bool) -> Command {
        self.enabled = enabled;
        self
    }

    pub fn checked(mut self, checked: bool) -> Command {
        self.check = Check::Checkbox(checked);
        self
    }

    /// One of a group of which one is on (the engine's `radio_group`).
    pub fn radio(mut self, on: bool) -> Command {
        self.check = Check::Radio(on);
        self
    }

    pub fn icon(mut self, icon: VsIcon) -> Command {
        self.icon = Some(icon);
        self
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Command {
        self.hint = Some(hint.into());
        self
    }
}

/// The mark in a row's leading column.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Check {
    #[default]
    None,
    /// A check mark when on.
    Checkbox(bool),
    /// A dot when on.
    Radio(bool),
}

pub enum MenuAction {
    Action(Box<dyn Action>),
    Run(WindowAction),
}

impl Clone for MenuAction {
    fn clone(&self) -> MenuAction {
        match self {
            MenuAction::Action(action) => MenuAction::Action(action.boxed_clone()),
            MenuAction::Run(run) => MenuAction::Run(run.clone()),
        }
    }
}

fn choosable(item: &MenuItem) -> bool {
    match item {
        MenuItem::Command(command) => command.enabled,
        MenuItem::Submenu { enabled, .. } => *enabled,
        MenuItem::Separator | MenuItem::Custom(_) => false,
    }
}

/// The next row the keyboard can land on after `from`, forwards or backwards, wrapping around;
/// with no `from`, the first (or last) one.
pub(crate) fn step(items: &[MenuItem], from: Option<usize>, forward: bool) -> Option<usize> {
    let n = items.len();
    if n == 0 {
        return None;
    }
    let start = from.unwrap_or(if forward { n - 1 } else { 0 });
    (1..=n).map(|k| if forward { (start + k) % n } else { (start + n - k) % n }).find(|&i| choosable(&items[i]))
}

impl OverlayHost {
    /// Shows `menu` against `anchor`. It takes the keyboard, closes by [`super::Dismiss::TRANSIENT`],
    /// and `on_close` runs once when it goes, with `Chosen` when a command was chosen.
    pub fn open_menu(
        &mut self,
        menu: Menu,
        anchor: Anchor,
        placement: Placement,
        on_close: Option<OnClose>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> OverlayId {
        self.open_level(menu, anchor, placement, None, on_close, window, cx).0
    }

    #[allow(clippy::too_many_arguments)]
    fn open_level(
        &mut self,
        menu: Menu,
        anchor: Anchor,
        placement: Placement,
        parent: Option<OverlayId>,
        on_close: Option<OnClose>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (OverlayId, Entity<MenuView>) {
        let host = cx.entity().downgrade();
        let view = cx.new(|cx| MenuView { menu, host, me: None, highlighted: None, child: None, rows: Rc::default(), focus: cx.focus_handle() });
        let focus = view.read(cx).focus.clone();
        let mut spec = OverlaySpec::new(OverlayKind::Menu, anchor, placement, view.clone()).interactive(focus);
        spec.parent = parent;
        spec.on_close = on_close;
        let id = self.open(spec, window, cx);
        view.update(cx, |view, _| view.me = Some(id));
        (id, view)
    }
}

/// One level of an open menu.
pub struct MenuView {
    menu: Menu,
    host: WeakEntity<OverlayHost>,
    me: Option<OverlayId>,
    highlighted: Option<usize>,
    /// The submenu open from a row: the row, its overlay and its view.
    child: Option<(usize, OverlayId, Entity<MenuView>)>,
    /// Where each row was drawn, for anchoring its submenu.
    rows: Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>,
    focus: FocusHandle,
}

impl MenuView {
    /// The highlighted row, for tests and the self-test.
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// Where each row was drawn last, in window pixels, for tests, the self-test and the demo.
    pub fn row_bounds(&self) -> Vec<Option<Bounds<Pixels>>> {
        self.rows.borrow().clone()
    }

    /// The submenu open from a row, if any.
    pub fn open_submenu(&self, cx: &App) -> Option<(usize, OverlayId)> {
        self.open_child(cx).map(|(row, id, _)| (row, id))
    }

    fn open_child(&self, cx: &App) -> Option<(usize, OverlayId, Entity<MenuView>)> {
        let host = self.host.upgrade()?;
        self.child.clone().filter(|(_, id, _)| host.read(cx).is_open(*id))
    }

    fn hover(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.highlighted == Some(row) || !self.menu.items.get(row).is_some_and(choosable) {
            return;
        }
        self.highlighted = Some(row);
        match self.menu.items[row] {
            MenuItem::Submenu { .. } => self.show_submenu(row, false, window, cx),
            _ => self.close_child(window, cx),
        }
        cx.notify();
    }

    fn activate(&mut self, row: usize, by_keyboard: bool, window: &mut Window, cx: &mut Context<Self>) {
        match self.menu.items.get(row) {
            Some(MenuItem::Command(command)) if command.enabled => {
                let run = command.run.clone();
                self.choose(run, window, cx);
            }
            Some(MenuItem::Submenu { enabled: true, .. }) => {
                self.highlighted = Some(row);
                self.show_submenu(row, by_keyboard, window, cx);
            }
            _ => {}
        }
    }

    /// Closes the whole menu, then runs the command.
    fn choose(&mut self, run: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(host), Some(me)) = (self.host.upgrade(), self.me) {
            host.update(cx, |host, cx| {
                let root = host.root_of(me);
                host.close_with(root, DismissReason::Chosen, window, cx);
            });
        }
        match run {
            MenuAction::Action(action) => window.dispatch_action(action, cx),
            MenuAction::Run(run) => run(window, cx),
        }
    }

    /// Opens the row's submenu if it is not open. From the keyboard it takes the keyboard with its
    /// first row highlighted; from the mouse the keyboard stays on this level.
    fn show_submenu(&mut self, row: usize, by_keyboard: bool, window: &mut Window, cx: &mut Context<Self>) {
        let child = match self.open_child(cx) {
            Some((open_row, _, view)) if open_row == row => view,
            _ => {
                self.close_child(window, cx);
                let (Some(host), Some(me)) = (self.host.upgrade(), self.me) else { return };
                let Some(MenuItem::Submenu { menu, .. }) = self.menu.items.get(row) else { return };
                let Some(anchor) = self.rows.borrow().get(row).copied().flatten() else { return };
                let menu = menu.clone();
                let (id, view) = host.update(cx, |host, cx| host.open_level(menu, Anchor::Rect(anchor), Placement::Beside, Some(me), None, window, cx));
                self.child = Some((row, id, view.clone()));
                view
            }
        };
        if by_keyboard {
            let focus = child.read(cx).focus.clone();
            child.update(cx, |child, cx| {
                child.highlighted = step(&child.menu.items, None, true);
                cx.notify();
            });
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn close_child(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some((_, id, _)), Some(host)) = (self.child.take(), self.host.upgrade()) else { return };
        host.update(cx, |host, cx| host.close(id, window, cx));
    }

    /// Closes this level (and below); the first level closes the menu.
    fn close_level(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(host), Some(me)) = (self.host.upgrade(), self.me) else { return };
        host.update(cx, |host, cx| host.close_with(me, DismissReason::Escape, window, cx));
    }

    fn is_submenu(&self, cx: &App) -> bool {
        let (Some(host), Some(me)) = (self.host.upgrade(), self.me) else { return false };
        host.read(cx).root_of(me) != me
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.highlighted = step(&self.menu.items, self.highlighted, true),
            "up" => self.highlighted = step(&self.menu.items, self.highlighted, false),
            "home" => self.highlighted = step(&self.menu.items, None, true),
            "end" => self.highlighted = step(&self.menu.items, None, false),
            "right" => {
                if let Some(row) = self.highlighted
                    && matches!(self.menu.items.get(row), Some(MenuItem::Submenu { enabled: true, .. }))
                {
                    self.show_submenu(row, true, window, cx);
                }
            }
            "enter" | "space" => {
                if let Some(row) = self.highlighted {
                    self.activate(row, true, window, cx);
                }
            }
            "left" if self.is_submenu(cx) => self.close_level(window, cx),
            "escape" => self.close_level(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn row(&self, index: usize, item: &MenuItem, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (label, icon, check, hint, submenu) = match item {
            MenuItem::Separator => return div().my_1().h(px(1.)).bg(theme.border).into_any_element(),
            MenuItem::Custom(draw) => return draw(window, cx),
            MenuItem::Command(command) => {
                let hint = command.hint.clone().or_else(|| match &command.run {
                    MenuAction::Action(action) => window
                        .highest_precedence_binding_for_action(action.as_ref())
                        .map(|b| b.keystrokes().iter().map(ToString::to_string).collect::<Vec<_>>().join(" ").into()),
                    MenuAction::Run(_) => None,
                });
                (command.label.clone(), command.icon, command.check, hint, false)
            }
            MenuItem::Submenu { label, icon, .. } => (label.clone(), *icon, Check::None, None, true),
        };
        let enabled = choosable(item);
        let lit = self.highlighted == Some(index) || self.open_child(cx).is_some_and(|(row, _, _)| row == index);
        let fg = if !enabled {
            theme.muted_foreground
        } else if lit {
            theme.accent_foreground
        } else {
            theme.popover_foreground
        };
        let mark = match check {
            Check::Checkbox(true) => Some(Icon::new(VsIcon::Check).size(px(14.)).into_any_element()),
            Check::Radio(true) => Some(div().size(px(6.)).rounded_full().bg(fg).into_any_element()),
            Check::None | Check::Checkbox(false) | Check::Radio(false) => icon.map(|icon| Icon::new(icon).size(px(14.)).into_any_element()),
        };
        let rows = self.rows.clone();
        div()
            .id(("menu-row", index))
            .relative()
            .min_h(tokens::MENU_ROW_MIN_H)
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(4.))
            .text_color(fg)
            .when(lit && enabled, |d| d.bg(theme.accent))
            .child(div().w(px(16.)).flex().justify_center().items_center().children(mark))
            .child(div().flex_1().child(label))
            .children(hint.map(|hint| div().pl_4().text_size(tokens::TEXT_CAPTION).text_color(theme.muted_foreground).child(hint)))
            .when(submenu, |d| d.child(Icon::new(VsIcon::ChevronRight).size(px(14.))))
            .child(
                canvas(
                    move |bounds, _, _| {
                        if let Some(slot) = rows.borrow_mut().get_mut(index) {
                            *slot = Some(bounds);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .when(enabled, |d| {
                d.on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                    if *hovered {
                        this.hover(index, window, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.activate(index, false, window, cx)))
            })
            .into_any_element()
    }
}

impl Render for MenuView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        *self.rows.borrow_mut() = vec![None; self.menu.items.len()];
        let items = self.menu.items.clone();
        let rows: Vec<AnyElement> = items.iter().enumerate().map(|(i, item)| self.row(i, item, window, cx)).collect();
        let theme = cx.theme().clone();
        div()
            .id("menu")
            .track_focus(&self.focus)
            .key_context("Menu")
            .on_key_down(cx.listener(Self::key))
            .min_w(tokens::MENU_MIN_W)
            .max_h(window.viewport_size().height - WINDOW_MARGIN * 2.)
            .overflow_y_scroll()
            .p_1()
            .flex()
            .flex_col()
            .text_size(tokens::TEXT)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow_md()
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Vec<MenuItem> {
        Menu::new()
            .item(Command::run("Disabled", |_, _| {}).enabled(false))
            .item(Command::run("One", |_, _| {}))
            .separator()
            .item(MenuItem::Custom(Rc::new(|_, _| div().into_any_element())))
            .submenu("More", Menu::new().item(Command::run("Inner", |_, _| {})))
            .item(Command::run("Two", |_, _| {}).checked(true))
            .items
    }

    #[test]
    fn the_keyboard_lands_only_on_rows_that_can_be_chosen() {
        let items = menu();
        assert_eq!(step(&items, None, true), Some(1));
        assert_eq!(step(&items, Some(1), true), Some(4), "past the separator and the custom row");
        assert_eq!(step(&items, Some(4), true), Some(5));
        assert_eq!(step(&items, Some(5), true), Some(1), "wraps past the disabled row");
        assert_eq!(step(&items, Some(1), false), Some(5));
        assert_eq!(step(&items, None, false), Some(5));
        assert_eq!(step(&[], None, true), None);
        assert_eq!(step(&[MenuItem::Separator], None, true), None);
    }
}
