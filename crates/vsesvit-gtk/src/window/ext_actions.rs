//! Extension toolbar actions: one button per loaded extension action, with its badge, and
//! the popover that hosts an action's popup page.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::extensions::ExtensionId;
use vsesvit_webext::ActionInfo;
use webkit::prelude::*;

/// Popup pages size themselves; these bound what a page can ask for, as Chrome does.
const POPUP_MIN: (i32, i32) = (200, 80);
const POPUP_MAX: (i32, i32) = (800, 600);
const POPUP_INITIAL: (i32, i32) = (320, 200);

pub(crate) struct ExtensionActions {
    container: gtk::Box,
    buttons: RefCell<Vec<Action>>,
    popup: RefCell<Option<Popup>>,
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
            self.popover.unparent();
        });
    }
}

impl ExtensionActions {
    pub(crate) fn new() -> Rc<Self> {
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        Rc::new(ExtensionActions {
            container,
            buttons: RefCell::new(Vec::new()),
            popup: RefCell::new(None),
        })
    }

    pub(crate) fn widget(&self) -> &gtk::Box {
        &self.container
    }

    /// Brings the buttons in line with `actions`, in their order. The button of an extension
    /// that is still loaded is updated in place, so its open popup survives badge, title and
    /// icon changes; a popup closes only when its extension goes away. `on_click` receives
    /// the extension whose action was clicked.
    pub(crate) fn rebuild(&self, actions: &[ActionInfo], on_click: impl Fn(&ExtensionId) + 'static) {
        let loaded = |id: &ExtensionId| actions.iter().any(|a| a.extension == *id);
        if self.popup.borrow().as_ref().is_some_and(|p| !loaded(&p.extension)) {
            self.close_popup();
        }
        let mut buttons = self.buttons.borrow_mut();
        let (mut kept, gone): (Vec<Action>, Vec<Action>) =
            buttons.drain(..).partition(|a| loaded(&a.info.extension));
        for action in gone {
            self.container.remove(&action.button);
        }
        let on_click = Rc::new(on_click);
        let mut previous: Option<gtk::Button> = None;
        for info in actions {
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
                    let on_click = on_click.clone();
                    button.connect_clicked(move |_| on_click(&id));
                    self.container.append(&button);
                    Action { info: info.clone(), button }
                }
            };
            self.container.reorder_child_after(&action.button, previous.as_ref());
            previous = Some(action.button.clone());
            buttons.push(action);
        }
    }

    pub(crate) fn button_for(&self, id: &ExtensionId) -> Option<gtk::Button> {
        self.buttons
            .borrow()
            .iter()
            .find(|a| a.info.extension == *id)
            .map(|a| a.button.clone())
    }

    /// Shows `view` (the runtime's popup page, already loading) in a popover under the
    /// extension's button. The view is dropped when the popover closes, whether the user
    /// dismissed it or the page called `window.close()`.
    pub(crate) fn show_popup(self: &Rc<Self>, id: &ExtensionId, view: webkit::WebView) {
        self.close_popup();
        let Some(button) = self.button_for(id) else { return };
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
        popover.set_parent(&button);
        let actions = Rc::downgrade(self);
        popover.connect_closed(move |popover| {
            if let Some(actions) = actions.upgrade() {
                actions.forget_popup(popover);
            }
        });
        popover.popup();
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

/// Puts the action's icon, badge and title on its button.
fn show_action(button: &gtk::Button, action: &ActionInfo) {
    let icon = match action.icon.as_deref().filter(|p| p.is_file()) {
        Some(path) => icon_from_file(path),
        None => gtk::Image::from_icon_name("application-x-addon-symbolic"),
    };
    icon.set_pixel_size(16);
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
        actions.rebuild(&[action(&id, "")], |_| {});
        let view = webkit::WebView::new();
        let weak = view.downgrade();
        actions.show_popup(&id, view);
        assert!(actions.popup_view().is_some(), "the popup did not open");
        Open { actions, id, view: weak, _window: window }
    }

    #[gtk::test]
    fn a_badge_update_leaves_the_open_popup_alone() {
        let open = open_popup();
        let button = open.actions.button_for(&open.id);
        open.actions.rebuild(&[action(&open.id, "3")], |_| {});
        assert!(open.actions.popup_view().is_some(), "the popup was closed");
        assert_eq!(open.actions.button_for(&open.id), button, "the button was replaced");
    }

    #[gtk::test]
    fn unloading_the_extension_closes_its_popup() {
        let open = open_popup();
        open.actions.rebuild(&[], |_| {});
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
