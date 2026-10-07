//! Tab groups in a window, by core's rules ([`TabGroups`]): the window keeps one beside its
//! `AdwTabView`, settles it after every change to its tabs, and draws it in both tab lists.
//! The vertical list heads each group with its chip, which collapses or expands it, and hides
//! a collapsed group's rows. `AdwTabBar` can neither hide pages nor head them, so on top each
//! group is a chip before the tabs that opens its editor, a grouped tab shows a dot in the
//! group's colour where its speaker would be, and a collapsed group's tabs stay in the bar.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::tab_groups::{GroupColor, GroupId, Row, Step, TabGroup, TabGroups, WindowTabs};
use vsesvit_webext::TabId;

use super::{BrowserWindow, Layout};
use crate::tab::Tab;

/// The group editor's width.
const EDITOR_WIDTH: i32 = 300;

/// The `tab-group` actions, each with a group's id as its target.
pub(super) fn install(window: &BrowserWindow) {
    let actions = gio::SimpleActionGroup::new();
    let entries = [
        ("toggle", BrowserWindow::toggle_group as fn(&BrowserWindow, GroupId)),
        ("edit", BrowserWindow::edit_group),
        ("new-tab", BrowserWindow::new_tab_in_group),
        ("ungroup", BrowserWindow::ungroup),
        ("close", BrowserWindow::close_group),
    ];
    for (name, run) in entries {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        action.connect_activate(glib::clone!(
            #[weak]
            window,
            move |_, target| {
                if let Some(id) = parse_target(target) {
                    run(&window, id);
                }
            }
        ));
        actions.add_action(&action);
    }
    window.insert_action_group("tab-group", Some(&actions));
}

/// A group as an action's target.
pub(super) fn target(id: GroupId) -> glib::Variant {
    id.to_string().to_variant()
}

pub(super) fn parse_target(target: Option<&glib::Variant>) -> Option<GroupId> {
    target?.str()?.parse().ok()
}

/// The group colours for the current theme as style classes, `tab-group-<colour>`: a chip's or
/// a swatch's fill and a tab row's bar. Rebuilt when the theme turns light or dark, when every
/// window redraws its groups too, for the dots on the tab bar.
pub(crate) fn install_style(display: &gdk::Display) {
    let provider = gtk::CssProvider::new();
    let style = adw::StyleManager::default();
    provider.load_from_string(&colors_css(style.is_dark()));
    gtk::style_context_add_provider_for_display(display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    style.connect_dark_notify(move |style| {
        provider.load_from_string(&colors_css(style.is_dark()));
        for window in gtk::Window::list_toplevels() {
            if let Ok(window) = window.downcast::<BrowserWindow>() {
                window.redraw_groups();
            }
        }
    });
}

fn colors_css(dark: bool) -> String {
    // Chrome's chips have white text in a light theme and dark text in a dark one.
    let text = if dark { "#202124" } else { "#ffffff" };
    let mut css = String::new();
    for color in GroupColor::ALL {
        let (class, fill) = (class_of(color), hex(color, dark));
        let _ = writeln!(
            css,
            ".{class}.tab-group-chip, .{class}.tab-group-swatch {{ background-color: {fill}; color: {text}; }}\n\
             .{class}.tab-row {{ box-shadow: inset 3px 0 {fill}; }}"
        );
    }
    css
}

fn hex(color: GroupColor, dark: bool) -> String {
    let [r, g, b] = color.rgb(dark);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn class_of(color: GroupColor) -> String {
    format!("tab-group-{}", color.label().to_lowercase())
}

/// Gives `widget` the style class of `color`, or of no colour.
pub(super) fn set_color_class(widget: &impl IsA<gtk::Widget>, color: Option<GroupColor>) {
    for other in GroupColor::ALL {
        widget.remove_css_class(&class_of(other));
    }
    if let Some(color) = color {
        widget.add_css_class(&class_of(color));
    }
}

/// Shows `group` on `chip`: its title on its colour, or the colour alone while it has none.
pub(super) fn paint_chip(chip: &gtk::Label, group: &TabGroup) {
    chip.set_label(&group.title);
    set_color_class(chip, Some(group.color));
    if group.title.is_empty() {
        chip.add_css_class("untitled");
    } else {
        chip.remove_css_class("untitled");
    }
}

thread_local! {
    static DOTS: RefCell<HashMap<[u8; 3], gdk::Texture>> = RefCell::default();
}

/// A dot in `rgb` half as wide as a tab's 16 px indicator, drawn at twice that size for scaled
/// displays.
fn dot(rgb: [u8; 3]) -> gdk::Texture {
    const SIZE: usize = 32;
    const RADIUS: f32 = 8.0;
    DOTS.with_borrow_mut(|dots| {
        dots.entry(rgb)
            .or_insert_with(|| {
                let center = SIZE as f32 / 2.0;
                let mut pixels = Vec::with_capacity(SIZE * SIZE * 4);
                for y in 0..SIZE {
                    for x in 0..SIZE {
                        let distance = (x as f32 + 0.5 - center).hypot(y as f32 + 0.5 - center);
                        let alpha = (RADIUS + 0.5 - distance).clamp(0.0, 1.0);
                        pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], (alpha * 255.0).round() as u8]);
                    }
                }
                let bytes = glib::Bytes::from_owned(pixels);
                gdk::MemoryTexture::new(SIZE as i32, SIZE as i32, gdk::MemoryFormat::R8g8b8a8, &bytes, SIZE * 4).upcast()
            })
            .clone()
    })
}

impl BrowserWindow {
    /// The window's tabs as its groups see them.
    pub(super) fn window_tabs(&self) -> WindowTabs<TabId> {
        let view = &self.ui().tab_view;
        WindowTabs {
            order: self.tabs().iter().map(Tab::id).collect(),
            pinned: usize::try_from(view.n_pinned_pages()).unwrap_or(0),
            active: self.selected_tab().map(|tab| tab.id()),
        }
    }

    fn page_by_id(&self, id: TabId) -> Option<adw::TabPage> {
        let tab = self.tabs().into_iter().find(|tab| tab.id() == id)?;
        self.page_of(&tab)
    }

    pub(crate) fn group_of(&self, tab: &Tab) -> Option<TabGroup> {
        self.imp().groups.borrow().group_of(&tab.id()).cloned()
    }

    /// A restored window's groups, from its tabs' records, once all of them are open.
    pub(crate) fn restore_groups(&self, tabs: Vec<(TabId, Option<TabGroup>)>) {
        self.imp().groups.replace(TabGroups::restore(tabs));
        self.settle_groups(None);
    }

    /// Whether the window has a group other than `tab`'s, and which ones, for the tab menu.
    pub(super) fn groups_for_menu(&self, tab: &Tab) -> (Option<GroupId>, Vec<TabGroup>) {
        let tabs = self.window_tabs();
        let groups = self.imp().groups.borrow();
        let own = groups.group_of(&tab.id()).map(|g| g.id);
        (own, groups.in_order(&tabs).into_iter().filter(|g| Some(g.id) != own).collect())
    }

    /// A tab opened from `opener` starts in its group, as in Chrome.
    pub(super) fn adopt_into_group(&self, tab: &Tab, opener: &Tab) {
        self.imp().groups.borrow_mut().adopt(tab.id(), &opener.id());
    }

    /// After anything changed the window's tabs: the groups follow them, and both tab lists
    /// show them. `moved` is the tab that just moved or opened, when known.
    pub(super) fn settle_groups(&self, moved: Option<&Tab>) {
        if self.imp().groups_held.get() {
            return;
        }
        let tabs = self.window_tabs();
        self.imp().groups.borrow_mut().settle(&tabs, moved.map(Tab::id).as_ref());
        self.redraw_groups();
    }

    /// A group action: `change` edits the groups and says how the tabs move to match, which
    /// happens with settling held off until the last step.
    fn change_groups(&self, change: impl FnOnce(&mut TabGroups<TabId>, &WindowTabs<TabId>) -> Vec<Step<TabId>>) {
        let tabs = self.window_tabs();
        let steps = change(&mut self.imp().groups.borrow_mut(), &tabs);
        self.imp().groups_held.set(true);
        for step in steps {
            self.apply_step(step);
        }
        self.imp().groups_held.set(false);
        self.settle_groups(None);
        self.browser().schedule_session_save();
    }

    fn apply_step(&self, step: Step<TabId>) {
        let view = &self.ui().tab_view;
        match step {
            Step::Unpin(id) => {
                if let Some(page) = self.page_by_id(id) {
                    view.set_page_pinned(&page, false);
                }
            }
            Step::Move(id, to) => {
                if let Some(page) = self.page_by_id(id) {
                    view.reorder_page(&page, i32::try_from(to).unwrap_or(i32::MAX));
                }
            }
            Step::Activate(id) => {
                if let Some(page) = self.page_by_id(id) {
                    view.set_selected_page(&page);
                }
            }
            Step::OpenTab => self.new_tab(),
        }
    }

    /// "Add Tab to New Group": the tab alone in a new group, whose editor opens to name it.
    pub(super) fn group_tab(&self, tab: &Tab) {
        let mut created = None;
        self.change_groups(|groups, tabs| {
            let (id, steps) = groups.new_group(tabs, &tab.id());
            created = Some(id);
            steps
        });
        if let Some(id) = created {
            self.edit_group(id);
        }
    }

    pub(super) fn add_tab_to_group(&self, tab: &Tab, group: GroupId) {
        self.change_groups(|groups, tabs| groups.join(tabs, &tab.id(), group));
    }

    pub(super) fn remove_tab_from_group(&self, tab: &Tab) {
        self.change_groups(|groups, tabs| groups.leave(tabs, &tab.id()));
    }

    /// A click on a group's header.
    fn toggle_group(&self, group: GroupId) {
        self.change_groups(|groups, tabs| {
            let collapsed = groups.get(group).is_some_and(|g| g.collapsed);
            groups.set_collapsed(tabs, group, !collapsed)
        });
    }

    fn rename_group(&self, group: GroupId, title: &str) {
        self.imp().groups.borrow_mut().set_title(group, title);
        self.redraw_groups();
        self.browser().schedule_session_save();
    }

    fn recolor_group(&self, group: GroupId, color: GroupColor) {
        self.imp().groups.borrow_mut().set_color(group, color);
        self.redraw_groups();
        self.browser().schedule_session_save();
    }

    /// A new tab right after the group's last, in the group.
    fn new_tab_in_group(&self, group: GroupId) {
        self.close_group_editor();
        let tabs = self.window_tabs();
        let Some(last) = self.imp().groups.borrow().members(&tabs, group).last().copied() else { return };
        let Some(at) = tabs.order.iter().position(|t| *t == last) else { return };
        let tab = Tab::new(self.browser(), self.browsing());
        self.imp().groups.borrow_mut().adopt(tab.id(), &last);
        let view = &self.ui().tab_view;
        view.set_selected_page(&view.insert(&tab, i32::try_from(at + 1).unwrap_or(i32::MAX)));
        self.load_new_tab_page(&tab);
    }

    fn ungroup(&self, group: GroupId) {
        self.close_group_editor();
        self.change_groups(|groups, _| {
            groups.ungroup(group);
            Vec::new()
        });
    }

    /// Closes the group's tabs the way each would close by itself.
    fn close_group(&self, group: GroupId) {
        self.close_group_editor();
        let members = self.imp().groups.borrow().members(&self.window_tabs(), group);
        let view = &self.ui().tab_view;
        for page in members.into_iter().filter_map(|id| self.page_by_id(id)) {
            view.close_page(&page);
        }
    }

    /// Draws the groups as they are now in both tab lists and the tab bar's dots.
    pub(crate) fn redraw_groups(&self) {
        let tabs = self.window_tabs();
        let (rows, order) = {
            let groups = self.imp().groups.borrow();
            (groups.rows(&tabs), groups.in_order(&tabs))
        };
        let ui = self.ui();
        let view = &ui.tab_view;
        let pages: Vec<adw::TabPage> = (0..view.n_pages()).map(|i| view.nth_page(i)).collect();
        let page_of = |id: TabId| tabs.order.iter().position(|t| *t == id).and_then(|i| pages.get(i)).cloned();
        let rows: Vec<Row<adw::TabPage>> = rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Header(group) => Some(Row::Header(group)),
                Row::Tab { tab, group, hidden } => Some(Row::Tab { tab: page_of(tab)?, group, hidden }),
            })
            .collect();
        ui.tab_list.show_groups(&rows);
        self.show_group_chips(&order);
        for page in &pages {
            if let Ok(tab) = page.child().downcast::<Tab>() {
                self.sync_indicator(page, &tab);
            }
        }
    }

    /// The dot and the name `tab`'s group shows on the tab bar, which has no other way to.
    pub(super) fn group_dot(&self, tab: &Tab) -> Option<(gio::Icon, String)> {
        if self.imp().layout.get() != Some(Layout::TopBar) {
            return None;
        }
        let group = self.group_of(tab)?;
        let dark = adw::StyleManager::default().is_dark();
        Some((dot(group.color.rgb(dark)).upcast(), group.name()))
    }

    /// Each group's chip at the start of the tab bar, in the order of the groups.
    fn show_group_chips(&self, groups: &[TabGroup]) {
        let bar = &self.ui().group_chips;
        let mut old = self.imp().chips.take();
        let mut shown = Vec::with_capacity(groups.len());
        for group in groups {
            let chip = match old.iter().position(|(id, _)| *id == group.id) {
                Some(at) => old.remove(at).1,
                None => {
                    let id = group.id;
                    let chip = gtk::Button::builder()
                        .child(&gtk::Label::builder().css_classes(["tab-group-chip"]).valign(gtk::Align::Center).build())
                        .css_classes(["flat", "tab-group-button"])
                        .valign(gtk::Align::Center)
                        .build();
                    chip.connect_clicked(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        move |_| window.edit_group(id)
                    ));
                    chip
                }
            };
            if let Some(label) = chip.child().and_downcast::<gtk::Label>() {
                paint_chip(&label, group);
            }
            chip.set_tooltip_text(Some(&group.name()));
            chip.update_property(&[gtk::accessible::Property::Label(&group.name())]);
            shown.push((group.id, chip));
        }
        for (_, gone) in old {
            bar.remove(&gone);
        }
        let mut previous: Option<gtk::Widget> = None;
        for (_, chip) in &shown {
            if chip.parent().is_none() {
                bar.append(chip);
            }
            bar.reorder_child_after(chip, previous.as_ref());
            previous = Some(chip.clone().upcast());
        }
        bar.set_visible(!shown.is_empty());
        self.imp().chips.replace(shown);
    }

    /// The group editor, at the group's header in the tab list, else its chip on the tab bar,
    /// else the top of the page.
    fn edit_group(&self, group: GroupId) {
        let Some(shown) = self.imp().groups.borrow().get(group).cloned() else { return };
        self.close_group_editor();
        let editor = self.group_editor(&shown);
        let ui = self.ui();
        let chip = self.imp().chips.borrow().iter().find(|(id, _)| *id == group).map(|(_, chip)| chip.clone().upcast());
        match [ui.tab_list.group_header(group), chip].into_iter().flatten().find(WidgetExt::is_mapped) {
            Some(anchor) => crate::popup(&editor, &anchor),
            None => {
                let page: &gtk::Widget = ui.toasts.upcast_ref();
                editor.set_pointing_to(Some(&gdk::Rectangle::new(page.width() / 2, 0, 1, 1)));
                crate::popup(&editor, page);
            }
        }
        self.imp().group_editor.replace(Some(editor));
    }

    pub(super) fn close_group_editor(&self) {
        if let Some(editor) = self.imp().group_editor.take() {
            editor.popdown();
        }
    }

    /// The editor of `group`: its name, its colour, and New Tab in Group, Ungroup and Close
    /// Group. The name and the colour apply as they change.
    fn group_editor(&self, group: &TabGroup) -> gtk::Popover {
        let id = group.id;
        let name = gtk::Entry::builder().text(&group.title).placeholder_text("Name this group").build();
        name.update_property(&[gtk::accessible::Property::Label("Group name")]);
        name.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |name| window.rename_group(id, &name.text())
        ));

        let swatches = gtk::Box::builder().spacing(6).halign(gtk::Align::Center).build();
        let mut first: Option<gtk::ToggleButton> = None;
        for color in GroupColor::ALL {
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            let swatch = gtk::ToggleButton::builder()
                .child(&check)
                .tooltip_text(color.label())
                .css_classes(["circular", "tab-group-swatch"])
                .active(color == group.color)
                .build();
            set_color_class(&swatch, Some(color));
            swatch.update_property(&[gtk::accessible::Property::Label(color.label())]);
            swatch.bind_property("active", &check, "visible").sync_create().build();
            swatch.set_group(first.as_ref());
            swatch.connect_toggled(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |swatch| {
                    if swatch.is_active() {
                        window.recolor_group(id, color);
                    }
                }
            ));
            swatches.append(&swatch);
            first.get_or_insert(swatch);
        }

        let actions = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for (label, action) in [("New Tab in Group", "new-tab"), ("Ungroup", "ungroup"), ("Close Group", "close")] {
            actions.append(
                &gtk::Button::builder()
                    .child(&gtk::Label::builder().label(label).xalign(0.0).build())
                    .action_name(format!("tab-group.{action}"))
                    .action_target(&target(id))
                    .css_classes(["flat", "tab-group-action"])
                    .build(),
            );
        }

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .width_request(EDITOR_WIDTH)
            .build();
        content.append(&name);
        content.append(&swatches);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&actions);
        let popover = gtk::Popover::builder().child(&content).position(gtk::PositionType::Bottom).build();
        popover.add_css_class("tab-group-editor");
        name.connect_activate(glib::clone!(
            #[weak]
            popover,
            move |_| popover.popdown()
        ));
        // After the popover has taken the focus, as Chrome puts it in the name.
        popover.connect_map(move |_| {
            let name = name.clone();
            glib::idle_add_local_once(move || {
                name.grab_focus();
            });
        });
        popover
    }

    /// The group editor while it is open.
    #[cfg(feature = "self-test")]
    pub(crate) fn group_editor_open(&self) -> Option<gtk::Popover> {
        self.imp().group_editor.borrow().clone().filter(|editor| editor.is_visible())
    }

    /// The header of `group` in the tab list, while it has one.
    #[cfg(feature = "self-test")]
    pub(crate) fn group_header(&self, group: GroupId) -> Option<gtk::Widget> {
        self.ui().tab_list.group_header(group)
    }

    /// Whether `tab`'s row in the tab list is hidden in a collapsed group.
    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn tab_row_hidden(&self, tab: &Tab) -> Option<bool> {
        self.ui().tab_list.row_hidden(&self.page_of(tab)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::browser;
    use crate::window::Focus;

    #[test]
    fn a_group_is_its_actions_target() {
        let id = GroupId::new();
        assert_eq!(parse_target(Some(&target(id))), Some(id));
        assert_eq!(parse_target(Some(&"not a group".to_variant())), None);
        assert_eq!(parse_target(None), None);
    }

    #[gtk::test]
    fn a_collapsed_group_hides_its_rows_but_keeps_its_header_and_a_shown_tab_selected() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.present();
        let first = window.open_tab(None, None, Focus::Foreground);
        let grouped = window.open_tab(None, None, Focus::Foreground);
        window.group_tab(&grouped);
        window.close_group_editor();
        let id = window.group_of(&grouped).expect("the tab is in its new group").id;
        let header = window.ui().tab_list.group_header(id).expect("the group has a header");
        let shown = (header.is_mapped(), window.tab_row_hidden(&grouped));

        let last = window.open_tab(None, None, Focus::Background);
        window.toggle_group(id);
        let collapsed = (header.is_mapped(), window.tab_row_hidden(&grouped), window.selected_tab() == Some(last.clone()));
        let list = &window.ui().tab_list;
        list.step_from(&window.page_of(&last).expect("the last tab's page"), -1);
        let stepped_up = window.selected_tab() == Some(first.clone());
        list.step_from(&window.page_of(&first).expect("the first tab's page"), 1);
        let stepped_down = window.selected_tab() == Some(last.clone());
        window.select_tab(&grouped);
        let reselected = (window.group_of(&grouped).map(|g| g.collapsed), window.tab_row_hidden(&grouped));
        window.ungroup(id);
        let ungrouped = (window.group_of(&grouped), window.ui().tab_list.group_header(id));
        window.destroy();
        assert_eq!(shown, (true, Some(false)));
        assert_eq!(collapsed, (true, Some(true), true), "the header stays, the row hides, the next shown tab is selected");
        assert!(stepped_down && stepped_up, "the arrow keys step over the hidden row");
        assert_eq!(reselected, (Some(false), Some(false)), "selecting a hidden tab expands its group");
        assert_eq!(ungrouped, (None, None));
    }
}
