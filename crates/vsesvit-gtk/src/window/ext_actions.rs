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
    buttons: RefCell<Vec<(ExtensionId, gtk::Button)>>,
    popup: RefCell<Option<Popup>>,
}

struct Popup {
    popover: gtk::Popover,
    view: webkit::WebView,
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

    /// Replaces the buttons. `on_click` receives the extension whose action was clicked.
    pub(crate) fn rebuild(&self, actions: &[ActionInfo], on_click: impl Fn(&ExtensionId) + 'static) {
        self.close_popup();
        let mut buttons = self.buttons.borrow_mut();
        for (_, button) in buttons.drain(..) {
            self.container.remove(&button);
        }
        let on_click = Rc::new(on_click);
        for action in actions {
            let button = action_button(action);
            let id = action.extension.clone();
            let on_click = on_click.clone();
            button.connect_clicked(move |_| on_click(&id));
            self.container.append(&button);
            buttons.push((action.extension.clone(), button));
        }
    }

    pub(crate) fn button_for(&self, id: &ExtensionId) -> Option<gtk::Button> {
        self.buttons
            .borrow()
            .iter()
            .find(|(ext, _)| ext == id)
            .map(|(_, button)| button.clone())
    }

    /// Shows `view` (the runtime's popup page, already loading) in a popover under the
    /// extension's button. The view is dropped when the popover closes.
    pub(crate) fn show_popup(&self, id: &ExtensionId, view: webkit::WebView) {
        self.close_popup();
        let Some(button) = self.button_for(id) else { return };
        view.set_size_request(POPUP_INITIAL.0, POPUP_INITIAL.1);
        view.connect_load_changed(|view, event| {
            if event == webkit::LoadEvent::Finished {
                let view = view.clone();
                glib::spawn_future_local(async move { fit_to_document(&view).await });
            }
        });
        let popover = gtk::Popover::builder()
            .child(&view)
            .position(gtk::PositionType::Bottom)
            .build();
        popover.set_parent(&button);
        popover.connect_closed(|popover| {
            popover.set_child(None::<&gtk::Widget>);
            // Unparenting from inside the signal handler confuses GTK's popover teardown.
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        popover.popup();
        self.popup.replace(Some(Popup { popover, view }));
    }

    pub(crate) fn popup_view(&self) -> Option<webkit::WebView> {
        self.popup.borrow().as_ref().map(|p| p.view.clone())
    }

    pub(crate) fn close_popup(&self) {
        if let Some(popup) = self.popup.borrow_mut().take() {
            popup.popover.popdown();
        }
    }
}

fn action_button(action: &ActionInfo) -> gtk::Button {
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
    gtk::Button::builder()
        .child(&overlay)
        .tooltip_text(&action.title)
        .css_classes(["flat"])
        .build()
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
