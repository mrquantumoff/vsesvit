//! Profiles in the Linux shell: the avatar every list of profiles shows, the dialog that names
//! and colours a profile, Manage Profiles, and the picker a launch shows before any profile
//! opens. Core's `profiles` keeps the list; `profile::launch` starts a profile's process.

use std::cell::Cell;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::profiles::{self, NAME_MAX, ProfileColor, ProfileId, ProfilesDir};

use crate::dialogs::{confirm, plain_toast, row_button};
use crate::window::BrowserWindow;
use crate::{APP_ID, profile};

/// The avatars' colours, one class per colour.
pub(crate) fn avatar_css() -> String {
    let mut css = String::from(
        ".profile-avatar { border-radius: 9999px; color: white; font-weight: bold; min-width: 22px; min-height: 22px; }\n\
         .profile-avatar.large { min-width: 72px; min-height: 72px; font-size: 2em; }\n\
         .profile-card { padding: 12px; }\n",
    );
    for color in ProfileColor::ALL {
        css += &format!(".profile-avatar.{} {{ background-color: {}; }}\n", color_class(color), color.css());
    }
    css
}

fn color_class(color: ProfileColor) -> String {
    format!("profile-{}", color.label().to_lowercase())
}

/// A coloured circle with the profile's initial.
pub(crate) fn avatar(name: &str, color: ProfileColor, large: bool) -> gtk::Label {
    let label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    if large {
        label.add_css_class("large");
    }
    set_avatar(&label, name, color);
    label
}

pub(crate) fn set_avatar(label: &gtk::Label, name: &str, color: ProfileColor) {
    let large = label.has_css_class("large");
    label.set_css_classes(&["profile-avatar", &color_class(color)]);
    if large {
        label.add_css_class("large");
    }
    label.set_label(&profiles::avatar_initial(name));
}

/// Asks for a profile's name and colour. `None` when cancelled.
async fn ask(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    accept: &str,
    name: &str,
    color: ProfileColor,
) -> Option<(String, ProfileColor)> {
    let entry = gtk::Entry::builder()
        .text(name)
        .activates_default(true)
        .max_length(NAME_MAX as i32)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label("Name")]);
    let chosen = Rc::new(Cell::new(color));
    let colors = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .max_children_per_line(5)
        .halign(gtk::Align::Center)
        .build();
    let mut group: Option<gtk::ToggleButton> = None;
    for choice in ProfileColor::ALL {
        let button = gtk::ToggleButton::builder()
            .child(&avatar("", choice, false))
            .tooltip_text(choice.label())
            .active(choice == color)
            .css_classes(["flat", "circular"])
            .build();
        button.set_group(group.as_ref());
        group.get_or_insert_with(|| button.clone());
        button.connect_toggled(glib::clone!(
            #[strong]
            chosen,
            move |button| {
                if button.is_active() {
                    chosen.set(choice);
                }
            }
        ));
        colors.append(&button);
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&entry);
    body.append(&colors);

    let dialog = adw::AlertDialog::new(Some(heading), None);
    dialog.set_extra_child(Some(&body));
    dialog.add_responses(&[("cancel", "_Cancel"), ("accept", accept)]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("accept", !name.trim().is_empty());
    entry.connect_changed(glib::clone!(
        #[weak]
        dialog,
        move |entry| dialog.set_response_enabled("accept", !entry.text().trim().is_empty())
    ));
    entry.grab_focus();
    let response = dialog.choose_future(Some(parent)).await;
    (response == "accept").then(|| (entry.text().trim().to_owned(), chosen.get()))
}

/// "Add Profile": names the new profile, then opens it in a window of its own, as Chrome does.
pub(crate) fn add(window: &BrowserWindow) {
    let registry = window.browser().profiles();
    let window = window.clone();
    glib::spawn_future_local(async move {
        let Some((name, color)) = ask(&window, "Add Profile", "_Add", &registry.next_name(), registry.next_color()).await else {
            return;
        };
        if let Err(e) = window.browser().add_profile(&window, &name, color) {
            window.toast(plain_toast(&e));
        }
    });
}

/// "Manage Profiles": every profile, to open, rename and recolour, or remove.
pub(crate) fn manage(window: &BrowserWindow) {
    window.browser().profile_used();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let add_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Profile")
        .css_classes(["flat"])
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Profiles")
        .description("Each profile has its own bookmarks, history, settings, extensions and sync account, and opens in windows of its own.")
        .header_suffix(&add_button)
        .build();
    group.add(&list);
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&page));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&toasts));
    let dialog = adw::Dialog::builder()
        .title("Manage Profiles")
        .content_width(520)
        .content_height(480)
        .child(&toolbar)
        .build();
    add_button.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| add(&window)
    ));
    let view = Rc::new_cyclic(|view: &std::rc::Weak<Manage>| {
        let view = view.clone();
        Manage {
            window: window.clone(),
            dialog: dialog.downgrade(),
            list,
            toasts,
            watch: Rc::new(move || {
                if let Some(view) = view.upgrade() {
                    view.fill();
                }
            }),
        }
    });
    window.browser().watch_profiles(&view.watch);
    view.fill();
    // The dialog's handlers hold the view until the dialog goes.
    dialog.connect_closed(move |_| {
        let _ = &view;
    });
    dialog.present(Some(window));
}

struct Manage {
    window: BrowserWindow,
    dialog: glib::WeakRef<adw::Dialog>,
    list: gtk::ListBox,
    toasts: adw::ToastOverlay,
    /// Kept for [`crate::browser::Browser::watch_profiles`], which holds it weakly.
    watch: Rc<dyn Fn()>,
}

impl Manage {
    fn fill(self: &Rc<Self>) {
        self.list.remove_all();
        let browser = self.window.browser();
        let registry = browser.profiles();
        let current = browser.home().map(|home| home.id.clone());
        for entry in registry.profiles() {
            let row = adw::ActionRow::builder()
                .title(&entry.name)
                .use_markup(false)
                .build();
            row.add_prefix(&avatar(&entry.name, entry.color, false));
            let id = entry.id.clone();
            if Some(&id) == current.as_ref() {
                row.set_subtitle("This profile");
            } else {
                let open = row_button("window-new-symbolic", "Open");
                open.connect_clicked(glib::clone!(
                    #[weak(rename_to = view)]
                    self,
                    #[strong]
                    id,
                    move |_| view.window.browser().open_profile(&view.window, &id)
                ));
                row.add_suffix(&open);
            }
            let edit = row_button("document-edit-symbolic", "Edit");
            let (name, color) = (entry.name.clone(), entry.color);
            edit.connect_clicked(glib::clone!(
                #[weak(rename_to = view)]
                self,
                #[strong]
                id,
                move |_| view.edit(id.clone(), name.clone(), color)
            ));
            row.add_suffix(&edit);
            let remove = row_button("user-trash-symbolic", "Remove");
            remove.set_sensitive(registry.can_remove(&id));
            let name = entry.name.clone();
            remove.connect_clicked(glib::clone!(
                #[weak(rename_to = view)]
                self,
                move |_| view.remove(id.clone(), name.clone())
            ));
            row.add_suffix(&remove);
            self.list.append(&row);
        }
    }

    fn edit(self: &Rc<Self>, id: ProfileId, name: String, color: ProfileColor) {
        let view = self.clone();
        glib::spawn_future_local(async move {
            let Some(dialog) = view.dialog.upgrade() else { return };
            let Some((name, color)) = ask(&dialog, "Edit Profile", "_Save", &name, color).await else { return };
            if let Err(e) = view.window.browser().edit_profile(&id, &name, color) {
                view.toasts.add_toast(plain_toast(&e));
            }
        });
    }

    fn remove(self: &Rc<Self>, id: ProfileId, name: String) {
        let view = self.clone();
        glib::spawn_future_local(async move {
            let body = "Its bookmarks, history, settings, extensions and sync sign-in are deleted from this device, and its windows close. What it synced stays on the sync server.";
            let Some(dialog) = view.dialog.upgrade() else { return };
            if !confirm(&dialog, &format!("Remove {name}?"), body, "_Remove").await {
                return;
            }
            if let Err(e) = view.window.browser().remove_profile(&view.window, &id) {
                view.toasts.add_toast(plain_toast(&e));
            }
        });
    }
}

/// The picker a launch that names no profile shows when there are several and none runs.
/// Choosing one starts it and closes the picker; this process never opens a profile. It runs
/// under the plain application id, so the desktop shows it as Vsesvit, but as its own
/// instance, apart from the `Default` profile's.
pub(crate) fn run_picker(dir: ProfilesDir) -> ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| {
        glib::set_application_name("Vsesvit");
        gtk::Window::set_default_icon_name(APP_ID);
        crate::app::load_css();
    });
    app.connect_activate(move |app| picker(app, &dir).present());
    let status = app.run_with_args::<&str>(&[]);
    status.into()
}

/// The picker's window: Chrome's "Who's using Chrome?".
pub(crate) fn picker(app: &adw::Application, dir: &ProfilesDir) -> adw::ApplicationWindow {
    let registry = dir.load();
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Vsesvit")
        .default_width(760)
        .default_height(520)
        .build();
    let toasts = adw::ToastOverlay::new();
    let cards = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .max_children_per_line(5)
        .halign(gtk::Align::Center)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    for entry in registry.profiles() {
        let card = card(&avatar(&entry.name, entry.color, true).upcast(), &entry.name);
        let root = dir.root(&entry.id);
        card.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            toasts,
            move |_| pick(&window, &toasts, &root)
        ));
        cards.append(&card);
    }
    let plus = gtk::Image::builder()
        .icon_name("list-add-symbolic")
        .pixel_size(48)
        .width_request(72)
        .height_request(72)
        .build();
    let add = card(&plus.upcast(), "Add");
    add.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        toasts,
        #[strong]
        dir,
        move |_| {
            let registry = dir.load();
            let dir = dir.clone();
            glib::spawn_future_local(async move {
                let Some((name, color)) = ask(&window, "Add Profile", "_Add", &registry.next_name(), registry.next_color()).await else {
                    return;
                };
                match dir.add(&name, color) {
                    Ok((id, _)) => pick(&window, &toasts, &dir.root(&id)),
                    Err(e) => toasts.add_toast(plain_toast(&e.to_string())),
                }
            });
        }
    ));
    cards.append(&add);

    let show = gtk::CheckButton::builder()
        .label("Show on startup")
        .active(registry.show_picker())
        .halign(gtk::Align::Center)
        .build();
    show.connect_toggled(glib::clone!(
        #[strong]
        dir,
        move |check| {
            if let Err(e) = dir.set_show_picker(check.is_active()) {
                log::warn!("the profile list: {e}");
            }
        }
    ));
    let heading = gtk::Label::builder()
        .label("Who's Using Vsesvit?")
        .css_classes(["title-1"])
        .build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .valign(gtk::Align::Center)
        .margin_top(24)
        .margin_bottom(24)
        .build();
    body.append(&heading);
    body.append(&cards);
    body.append(&show);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&body)
        .build();
    toasts.set_child(Some(&scrolled));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    toolbar.set_content(Some(&toasts));
    window.set_content(Some(&toolbar));
    window
}

fn card(picture: &gtk::Widget, name: &str) -> gtk::Button {
    let label = gtk::Label::builder()
        .label(name)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(14)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.append(picture);
    content.append(&label);
    gtk::Button::builder()
        .child(&content)
        .tooltip_text(name)
        .css_classes(["flat", "profile-card"])
        .build()
}

fn pick(window: &adw::ApplicationWindow, toasts: &adw::ToastOverlay, root: &Path) {
    match profile::launch(window, root) {
        Ok(()) => window.close(),
        Err(e) => toasts.add_toast(plain_toast(&format!("Could not open the profile: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn labels(widget: &gtk::Widget) -> Vec<String> {
        let mut found = Vec::new();
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                found.push(label.label().to_string());
            }
            found.extend(labels(&widget));
            child = widget.next_sibling();
        }
        found
    }

    #[gtk::test]
    fn the_picker_lists_every_profile_and_can_add_one() {
        let path = glib::mkdtemp(glib::tmp_dir().join("vsesvit-picker-XXXXXX")).unwrap();
        let dir = ProfilesDir::at(path.clone());
        fs::create_dir_all(dir.root(&ProfileId::default_profile())).unwrap();
        dir.opened(&ProfileId::default_profile()).unwrap();
        dir.add("Work", ProfileColor::Green).unwrap();
        let window = picker(crate::test_support::browser().app(), &dir);
        let shown = labels(window.upcast_ref());
        for expected in ["Who's Using Vsesvit?", "Person 1", "P", "Work", "W", "Add"] {
            assert!(shown.iter().any(|s| s == expected), "{expected:?} in {shown:?}");
        }
        window.destroy();
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn every_colour_has_its_avatar_class() {
        let css = avatar_css();
        for color in ProfileColor::ALL {
            assert!(css.contains(&format!(".profile-avatar.{} {{ background-color: {}; }}", color_class(color), color.css())));
        }
    }
}
