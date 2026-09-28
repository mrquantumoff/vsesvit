//! Extension toolbar actions: a button per pinned extension action, with its badge, in the
//! order of core's synced toolbar preference; the puzzle-piece Extensions menu listing every
//! action with a pin toggle; and the popover that hosts an action's popup page, under the
//! action's button or, for an unpinned one, under the puzzle piece.
//!
//! Pinned buttons can be dragged to reorder them and right-clicked to unpin them. Pins and
//! moves go through `win.extension-pin` and `win.extension-move`, which write the preference.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use vsesvit_core::extensions::ExtensionId;
use vsesvit_webext::ActionInfo;
use webkit::prelude::*;

/// Popup pages size themselves; these bound what a page can ask for, as Chrome does.
const POPUP_MIN: (i32, i32) = (200, 80);
const POPUP_MAX: (i32, i32) = (800, 600);
const POPUP_INITIAL: (i32, i32) = (320, 200);

pub(crate) struct ExtensionActions {
    container: gtk::Box,
    /// The pinned actions' buttons.
    pinned: gtk::Box,
    puzzle: gtk::Button,
    buttons: RefCell<Vec<Action>>,
    /// Every action, in install order, and whether it is pinned: what the menu lists.
    listed: RefCell<Vec<(ActionInfo, bool)>>,
    on_click: RefCell<Option<OnClick>>,
    menu: RefCell<Option<OpenMenu>>,
    popup: RefCell<Option<Popup>>,
}

/// What clicking an action does, given by the window.
type OnClick = Rc<dyn Fn(&ExtensionId)>;

/// The Extensions menu while it is open.
#[derive(Clone)]
struct OpenMenu {
    popover: gtk::Popover,
    rows: gtk::Box,
}

/// What a pinned button carries while it is dragged.
struct Dragged(ExtensionId);

/// Where a pinned action dragged from `from` lands when dropped on the one at `target`,
/// before or after it, counted among the pinned actions without the dragged one (as core's
/// `move_pinned` counts).
fn drop_index(from: usize, target: usize, after: bool) -> usize {
    let gap = target + usize::from(after);
    if from < gap { gap - 1 } else { gap }
}

/// A toolbar button and the action state it shows.
struct Action {
    info: ActionInfo,
    button: gtk::Button,
}

struct Popup {
    extension: ExtensionId,
    popover: gtk::Popover,
    view: webkit::WebView,
}

impl Popup {
    /// Takes the page out of the popover and the popover off its button, then drops both.
    fn discard(self) {
        // Unparenting from inside the popover's `closed` handler confuses GTK's popover
        // teardown, and `closed` may be what got us here.
        glib::idle_add_local_once(move || {
            self.popover.set_child(None::<&gtk::Widget>);
            if self.popover.parent().is_some() {
                self.popover.unparent();
            }
        });
    }
}

impl ExtensionActions {
    pub(crate) fn new() -> Rc<Self> {
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let pinned = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let puzzle = gtk::Button::builder()
            .icon_name("application-x-addon-symbolic")
            .tooltip_text("Extensions")
            .visible(false)
            .build();
        container.append(&pinned);
        container.append(&puzzle);
        let actions = Rc::new(ExtensionActions {
            container,
            pinned,
            puzzle,
            buttons: RefCell::new(Vec::new()),
            listed: RefCell::new(Vec::new()),
            on_click: RefCell::new(None),
            menu: RefCell::new(None),
            popup: RefCell::new(None),
        });
        let weak = Rc::downgrade(&actions);
        actions.puzzle.connect_clicked(move |_| {
            if let Some(actions) = weak.upgrade() {
                actions.open_menu();
            }
        });
        actions
    }

    pub(crate) fn widget(&self) -> &gtk::Box {
        &self.container
    }

    /// Shows the buttons of `pinned`, in its order, and lists every one of `actions` (all
    /// loaded actions, in install order) in the Extensions menu. The button of an extension
    /// that stays pinned is updated in place, so its open popup survives badge, title and
    /// icon changes; a popup closes only when its extension goes away. `on_click` receives
    /// the extension whose action was clicked.
    pub(crate) fn rebuild(self: &Rc<Self>, actions: &[ActionInfo], pinned: &[ExtensionId], on_click: impl Fn(&ExtensionId) + 'static) {
        let on_click: OnClick = Rc::new(on_click);
        self.on_click.replace(Some(on_click));
        let is_loaded = |id: &ExtensionId| actions.iter().any(|a| a.extension == *id);
        if self.popup.borrow().as_ref().is_some_and(|p| !is_loaded(&p.extension)) {
            self.close_popup();
        }
        self.listed.replace(actions.iter().map(|a| (a.clone(), pinned.contains(&a.extension))).collect());
        self.puzzle.set_visible(!actions.is_empty());
        self.refill_menu();
        let actions: Vec<&ActionInfo> = pinned.iter().filter_map(|id| actions.iter().find(|a| a.extension == *id)).collect();
        let loaded = |id: &ExtensionId| actions.iter().any(|a| a.extension == *id);
        let mut buttons = self.buttons.borrow_mut();
        let (mut kept, gone): (Vec<Action>, Vec<Action>) =
            buttons.drain(..).partition(|a| loaded(&a.info.extension));
        for action in gone {
            self.pinned.remove(&action.button);
        }
        let mut previous: Option<gtk::Button> = None;
        for info in actions.iter().copied() {
            let action = match kept.iter().position(|a| a.info.extension == info.extension) {
                Some(i) => {
                    let mut action = kept.swap_remove(i);
                    if action.info != *info {
                        show_action(&action.button, info);
                        action.info = info.clone();
                    }
                    action
                }
                None => {
                    let button = gtk::Button::builder().css_classes(["flat"]).build();
                    show_action(&button, info);
                    let id = info.extension.clone();
                    let weak = Rc::downgrade(self);
                    button.connect_clicked(move |_| {
                        if let Some(on_click) = weak.upgrade().and_then(|a| a.on_click.borrow().clone()) {
                            on_click(&id);
                        }
                    });
                    self.make_movable(&button, &info.extension);
                    self.pinned.append(&button);
                    Action { info: (*info).clone(), button }
                }
            };
            self.pinned.reorder_child_after(&action.button, previous.as_ref());
            previous = Some(action.button.clone());
            buttons.push(action);
        }
    }

    /// Dragging a pinned button onto another moves it there; right-clicking it offers
    /// Unpin and Manage Extensions.
    fn make_movable(self: &Rc<Self>, button: &gtk::Button, id: &ExtensionId) {
        let source = gtk::DragSource::builder().actions(gdk::DragAction::MOVE).build();
        let dragged = id.clone();
        source.connect_prepare(move |source, x, y| {
            if let Some(widget) = source.widget() {
                source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&widget))), x as i32, y as i32);
            }
            Some(gdk::ContentProvider::for_value(&glib::BoxedAnyObject::new(Dragged(dragged.clone())).to_value()))
        });
        button.add_controller(source);

        let drop = gtk::DropTarget::new(glib::BoxedAnyObject::static_type(), gdk::DragAction::MOVE);
        let (weak, target) = (Rc::downgrade(self), id.clone());
        drop.connect_drop(move |drop, value, x, _| {
            let (Some(actions), Ok(boxed)) = (weak.upgrade(), value.get::<glib::BoxedAnyObject>()) else { return false };
            let Ok(dragged) = boxed.try_borrow::<Dragged>() else { return false };
            let Some(widget) = drop.widget() else { return false };
            let order: Vec<ExtensionId> = actions.buttons.borrow().iter().map(|a| a.info.extension.clone()).collect();
            let (Some(from), Some(at)) = (order.iter().position(|e| *e == dragged.0), order.iter().position(|e| *e == target)) else {
                return false;
            };
            let past_middle = x > f64::from(widget.width()) / 2.0;
            let after = past_middle != (widget.direction() == gtk::TextDirection::Rtl);
            let to = u32::try_from(drop_index(from, at, after)).unwrap_or(u32::MAX);
            let _ = widget.activate_action("win.extension-move", Some(&(dragged.0.as_str(), to).to_variant()));
            true
        });
        button.add_controller(drop);

        let click = gtk::GestureClick::builder().button(gdk::BUTTON_SECONDARY).build();
        let id = id.clone();
        click.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(button) = gesture.widget() {
                unpin_menu(&button, &id, (x, y));
            }
        });
        button.add_controller(click);
    }

    /// The Extensions menu: every action with its icon and name, which opens it, and a pin
    /// toggle; then Manage Extensions.
    fn open_menu(self: &Rc<Self>) {
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let manage = gtk::Button::builder()
            .label("Manage Extensions")
            .action_name("win.show-extensions")
            .css_classes(["flat"])
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        content.append(&gtk::Label::builder().label("Extensions").xalign(0.0).margin_start(8).margin_top(4).css_classes(["heading"]).build());
        content.append(&rows);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&manage);
        let menu = gtk::Popover::builder().child(&content).css_classes(["extensions-menu"]).build();
        manage.connect_clicked(glib::clone!(
            #[weak]
            menu,
            move |_| menu.popdown()
        ));
        let weak = Rc::downgrade(self);
        menu.connect_closed(move |_| {
            if let Some(actions) = weak.upgrade() {
                actions.menu.take();
            }
        });
        self.menu.replace(Some(OpenMenu { popover: menu.clone(), rows }));
        self.refill_menu();
        crate::popup(&menu, &self.puzzle);
    }

    fn refill_menu(self: &Rc<Self>) {
        let Some(OpenMenu { popover: menu, rows }) = self.menu.borrow().clone() else { return };
        while let Some(row) = rows.first_child() {
            rows.remove(&row);
        }
        for (info, pinned) in self.listed.borrow().iter() {
            let label = gtk::Label::builder()
                .label(&info.title)
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(32)
                .build();
            let name = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            name.append(&action_icon(info));
            name.append(&label);
            let open = gtk::Button::builder().child(&name).hexpand(true).css_classes(["flat"]).build();
            let (weak, id, popover) = (Rc::downgrade(self), info.extension.clone(), menu.downgrade());
            open.connect_clicked(move |_| {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
                if let Some(on_click) = weak.upgrade().and_then(|a| a.on_click.borrow().clone()) {
                    on_click(&id);
                }
            });
            let pin = gtk::Button::builder()
                .icon_name("view-pin-symbolic")
                .tooltip_text(if *pinned { "Unpin from Toolbar" } else { "Pin to Toolbar" })
                .action_name("win.extension-pin")
                .action_target(&(info.extension.as_str(), !*pinned).to_variant())
                .css_classes(if *pinned { vec!["flat", "extension-pinned"] } else { vec!["flat", "dim-label"] })
                .build();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            row.append(&open);
            row.append(&pin);
            rows.append(&row);
        }
    }

    /// Clicks the puzzle piece and returns the menu it opened.
    #[cfg(feature = "self-test")]
    pub(crate) fn click_puzzle(&self) -> Option<gtk::Popover> {
        self.puzzle.emit_clicked();
        self.menu.borrow().as_ref().map(|open| open.popover.clone())
    }

    pub(crate) fn button_for(&self, id: &ExtensionId) -> Option<gtk::Button> {
        self.buttons
            .borrow()
            .iter()
            .find(|a| a.info.extension == *id)
            .map(|a| a.button.clone())
    }

    /// Shows `view` (the runtime's popup page, already loading) in a popover under the
    /// extension's button, or under the puzzle piece when it is not pinned. The view is
    /// dropped when the popover closes, whether the user dismissed it or the page called
    /// `window.close()`.
    pub(crate) fn show_popup(self: &Rc<Self>, id: &ExtensionId, view: webkit::WebView) {
        self.close_popup();
        let button = self.button_for(id).unwrap_or_else(|| self.puzzle.clone());
        view.set_size_request(POPUP_INITIAL.0, POPUP_INITIAL.1);
        view.connect_load_changed(|view, event| {
            if event == webkit::LoadEvent::Finished {
                let view = view.clone();
                glib::spawn_future_local(async move { fit_to_document(&view).await });
            }
        });
        let actions = Rc::downgrade(self);
        view.connect_close(move |view| {
            if let Some(actions) = actions.upgrade()
                && actions.popup_view().as_ref() == Some(view)
            {
                actions.close_popup();
            }
        });
        let popover = gtk::Popover::builder()
            .child(&view)
            .position(gtk::PositionType::Bottom)
            .build();
        let actions = Rc::downgrade(self);
        popover.connect_closed(move |popover| {
            if let Some(actions) = actions.upgrade() {
                actions.forget_popup(popover);
            }
        });
        crate::popup(&popover, &button);
        self.popup.replace(Some(Popup { extension: id.clone(), popover, view }));
    }

    pub(crate) fn popup_view(&self) -> Option<webkit::WebView> {
        self.popup.borrow().as_ref().map(|p| p.view.clone())
    }

    pub(crate) fn close_popup(&self) {
        let popup = self.popup.borrow_mut().take();
        if let Some(popup) = popup {
            popup.popover.popdown();
            popup.discard();
        }
    }

    /// The popover closed on its own (the user clicked elsewhere or pressed Escape).
    fn forget_popup(&self, popover: &gtk::Popover) {
        let popup = self.popup.borrow_mut().take_if(|p| p.popover == *popover);
        if let Some(popup) = popup {
            popup.discard();
        }
    }
}

/// A pinned button's context menu.
pub(crate) fn unpin_menu(button: &gtk::Widget, id: &ExtensionId, (x, y): (f64, f64)) -> gtk::PopoverMenu {
    let menu = gio::Menu::new();
    let unpin = gio::MenuItem::new(Some("_Unpin"), None);
    unpin.set_action_and_target_value(Some("win.extension-pin"), Some(&(id.as_str(), false).to_variant()));
    menu.append_item(&unpin);
    menu.append(Some("_Manage Extensions"), Some("win.show-extensions"));
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    crate::popup(&popover, button);
    popover
}

fn action_icon(action: &ActionInfo) -> gtk::Image {
    let icon = match action.icon.as_deref().filter(|p| p.is_file()) {
        Some(path) => icon_from_file(path),
        None => gtk::Image::from_icon_name("application-x-addon-symbolic"),
    };
    icon.set_pixel_size(16);
    icon
}

/// Puts the action's icon, badge and title on its button.
fn show_action(button: &gtk::Button, action: &ActionInfo) {
    let icon = action_icon(action);
    let badge = gtk::Label::builder()
        .label(&action.badge_text)
        .visible(!action.badge_text.is_empty())
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .can_target(false)
        .css_classes(["extension-badge"])
        .build();
    let overlay = gtk::Overlay::builder().child(&icon).build();
    overlay.add_overlay(&badge);
    button.set_child(Some(&overlay));
    button.set_tooltip_text(Some(&action.title));
}

fn icon_from_file(path: &Path) -> gtk::Image {
    match gtk::gdk::Texture::from_filename(path) {
        Ok(texture) => gtk::Image::from_paintable(Some(&texture)),
        Err(e) => {
            log::debug!("extension icon {}: {e}", path.display());
            gtk::Image::from_icon_name("application-x-addon-symbolic")
        }
    }
}

/// Resizes the popup to its document, within [`POPUP_MIN`]..[`POPUP_MAX`].
async fn fit_to_document(view: &webkit::WebView) {
    let script = "JSON.stringify([document.documentElement.scrollWidth, document.documentElement.scrollHeight])";
    let result = glib::future_with_timeout(
        Duration::from_secs(2),
        view.evaluate_javascript_future(script, None, None),
    )
    .await;
    let Ok(Ok(value)) = result else { return };
    let Some(size) = value
        .to_str()
        .as_str()
        .parse::<serde_json::Value>()
        .ok()
        .and_then(|v| Some((v.get(0)?.as_i64()?, v.get(1)?.as_i64()?)))
    else {
        return;
    };
    let width = i32::try_from(size.0).unwrap_or(POPUP_MAX.0).clamp(POPUP_MIN.0, POPUP_MAX.0);
    let height = i32::try_from(size.1).unwrap_or(POPUP_MAX.1).clamp(POPUP_MIN.1, POPUP_MAX.1);
    view.set_size_request(width, height);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::settle;

    fn action(extension: &ExtensionId, badge_text: &str) -> ActionInfo {
        ActionInfo {
            extension: extension.clone(),
            title: "Probe".to_owned(),
            icon: None,
            has_popup: true,
            badge_text: badge_text.to_owned(),
        }
    }

    struct Open {
        actions: Rc<ExtensionActions>,
        id: ExtensionId,
        view: glib::WeakRef<webkit::WebView>,
        _window: gtk::Window,
    }

    /// A toolbar with one action whose popup is open. The window is realized, which gives
    /// the popover a surface, and never shown.
    fn open_popup() -> Open {
        let actions = ExtensionActions::new();
        let window = gtk::Window::new();
        window.set_child(Some(actions.widget()));
        WidgetExt::realize(&window);
        let id = ExtensionId::parse(&"a".repeat(32)).expect("a valid id");
        actions.rebuild(&[action(&id, "")], std::slice::from_ref(&id), |_| {});
        let view = webkit::WebView::new();
        let weak = view.downgrade();
        actions.show_popup(&id, view);
        assert!(actions.popup_view().is_some(), "the popup did not open");
        Open { actions, id, view: weak, _window: window }
    }

    #[test]
    fn drops_land_where_core_counts_them() {
        // Pinned: [A, B, C].
        assert_eq!(drop_index(0, 2, true), 2, "A after C is last");
        assert_eq!(drop_index(2, 0, false), 0, "C before A is first");
        assert_eq!(drop_index(0, 1, false), 0, "A before B stays");
        assert_eq!(drop_index(0, 1, true), 1, "A after B");
        assert_eq!(drop_index(2, 1, false), 1, "C before B");
    }

    #[gtk::test]
    fn the_toolbar_shows_the_pinned_actions_in_order_and_unpinned_popups_open_from_the_puzzle_piece() {
        let actions = ExtensionActions::new();
        let window = gtk::Window::new();
        window.set_child(Some(actions.widget()));
        WidgetExt::realize(&window);
        let [a, b, c] = ["a", "b", "c"].map(|l| ExtensionId::parse(&l.repeat(32)).expect("a valid id"));
        let all = [action(&a, ""), action(&b, ""), action(&c, "")];
        actions.rebuild(&all, &[c.clone(), a.clone()], |_| {});
        let shown: Vec<gtk::Widget> = std::iter::successors(actions.pinned.first_child(), |w| w.next_sibling()).collect();
        let expected: Vec<gtk::Widget> = [&c, &a].iter().filter_map(|id| actions.button_for(id)).map(|b| b.upcast()).collect();
        let puzzle_shown = actions.puzzle.get_visible();
        let listed: Vec<(ExtensionId, bool)> = actions.listed.borrow().iter().map(|(info, pinned)| (info.extension.clone(), *pinned)).collect();

        actions.show_popup(&b, webkit::WebView::new());
        let anchor = actions.popup.borrow().as_ref().and_then(|p| p.popover.parent());
        actions.close_popup();
        actions.rebuild(&[], &[], |_| {});
        let puzzle_hidden = !actions.puzzle.get_visible();

        assert_eq!(shown, expected, "C then A, B not in the toolbar");
        assert!(puzzle_shown && puzzle_hidden, "the puzzle piece shows only while there are actions");
        assert_eq!(listed, [(a, true), (b, false), (c, true)], "the menu lists every action in install order");
        assert_eq!(anchor, Some(actions.puzzle.clone().upcast()), "an unpinned action's popup opens from the puzzle piece");
    }

    #[gtk::test]
    fn a_badge_update_leaves_the_open_popup_alone() {
        let open = open_popup();
        let button = open.actions.button_for(&open.id);
        open.actions.rebuild(&[action(&open.id, "3")], std::slice::from_ref(&open.id), |_| {});
        assert!(open.actions.popup_view().is_some(), "the popup was closed");
        assert_eq!(open.actions.button_for(&open.id), button, "the button was replaced");
    }

    #[gtk::test]
    fn unloading_the_extension_closes_its_popup() {
        let open = open_popup();
        open.actions.rebuild(&[], &[], |_| {});
        assert!(open.actions.popup_view().is_none());
        assert_eq!(open.actions.button_for(&open.id), None);
    }

    #[gtk::test]
    fn window_close_in_the_popup_page_closes_the_popup() {
        let open = open_popup();
        let view = open.view.upgrade().expect("the popup page");
        view.emit_by_name::<()>("close", &[]);
        drop(view);
        settle(Duration::from_millis(100));
        assert!(open.actions.popup_view().is_none(), "the popup is still open");
        assert!(open.view.upgrade().is_none(), "the popup page is still alive");
    }

    #[gtk::test]
    fn a_dismissed_popup_page_is_dropped() {
        let open = open_popup();
        let popover = open.actions.popup.borrow().as_ref().map(|p| p.popover.clone());
        popover.expect("the popover").popdown();
        settle(Duration::from_millis(100));
        assert!(open.actions.popup_view().is_none(), "the popup is still held");
        assert!(open.view.upgrade().is_none(), "the popup page is still alive");
    }
}
