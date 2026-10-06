//! The primary menu, with the zoom controls as a custom row.

use gtk::{gio, prelude::*};

/// The menu button and the zoom-level button inside it, whose label follows the selected tab.
pub(super) fn main_menu() -> (gtk::MenuButton, gtk::Button) {
    let windows = gio::Menu::new();
    windows.append(Some("New _Tab"), Some("win.new-tab"));
    windows.append(Some("New _Window"), Some("app.new-window"));
    windows.append(Some("New _Private Window"), Some("app.new-private-window"));

    let zoom = gio::Menu::new();
    let zoom_row = gio::MenuItem::new(None, None);
    zoom_row.set_attribute_value("custom", Some(&"zoom".to_variant()));
    zoom.append_item(&zoom_row);

    let tools = gio::Menu::new();
    tools.append(Some("_Developer Tools"), Some("win.developer-tools"));
    tools.append(Some("View Page _Source"), Some("win.view-source"));

    let page = gio::Menu::new();
    page.append(Some("_Print…"), Some("win.print"));
    page.append(Some("_Save Page As…"), Some("win.save-page"));
    page.append(Some("_Find…"), Some("win.find"));
    page.append(Some("_Fullscreen"), Some("win.fullscreen"));
    page.append_submenu(Some("More _Tools"), &tools);

    let library = gio::Menu::new();
    library.append(Some("_Bookmarks"), Some("win.show-bookmarks"));
    library.append(Some("_History"), Some("win.show-history"));
    library.append(Some("_Downloads"), Some("win.show-downloads"));
    library.append(Some("_Extensions"), Some("win.show-extensions"));
    library.append(Some("Show Bookmarks _Bar"), Some("win.show-bookmarks-bar"));

    let app = gio::Menu::new();
    app.append(Some("_Settings"), Some("win.show-settings"));
    app.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
    app.append(Some("_Welcome"), Some("app.welcome"));
    app.append(Some("_About Vsesvit"), Some("app.about"));

    let menu = gio::Menu::new();
    for section in [&windows, &zoom, &page, &library, &app] {
        menu.append_section(None, section);
    }

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    let (controls, level) = zoom_controls();
    popover.add_child(&controls, "zoom");
    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .primary(true)
        .popover(&popover)
        .build();
    (button, level)
}

fn zoom_controls() -> (gtk::Box, gtk::Button) {
    let zoom_out = gtk::Button::builder()
        .icon_name("zoom-out-symbolic")
        .action_name("win.zoom-out")
        .tooltip_text("Zoom Out")
        .css_classes(["flat", "circular"])
        .build();
    let level = gtk::Button::builder()
        .label("100%")
        .action_name("win.zoom-reset")
        .tooltip_text("Reset Zoom")
        .css_classes(["flat", "numeric"])
        .hexpand(true)
        .build();
    let zoom_in = gtk::Button::builder()
        .icon_name("zoom-in-symbolic")
        .action_name("win.zoom-in")
        .tooltip_text("Zoom In")
        .css_classes(["flat", "circular"])
        .build();
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    row.append(&zoom_out);
    row.append(&level);
    row.append(&zoom_in);
    (row, level)
}
