//! The Extensions dialog: install from a store link or id, a `.crx`/`.xpi` file or an
//! unpacked folder, with the job's progress shown; the installed list with icon, name,
//! version, provenance, an enabled switch and removal; and, per extension, what its
//! manifest asks for that the Linux runtime does not provide.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::extensions::{InstallSource, InstalledExtension, SourceParseError};

use crate::browser::Browser;
use crate::extensions::{
    EnableFailure, InstallFailure, describe_verification, icon_path, progress_to,
    unsupported_notice,
};
use crate::window::BrowserWindow;

/// Holds no [`Browser`] of its own: the widgets' handlers keep this state alive for as long
/// as the dialog's widgets exist, which must not keep the profile open.
struct State {
    window: glib::WeakRef<BrowserWindow>,
    dialog: adw::PreferencesDialog,
    progress: adw::ActionRow,
    installed: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
}

pub(crate) fn present(window: &BrowserWindow) {
    let source = adw::EntryRow::builder()
        .title("Chrome Web Store or Firefox Add-ons link or ID")
        .show_apply_button(true)
        .input_purpose(gtk::InputPurpose::Url)
        .build();
    let from_file = adw::ButtonRow::builder()
        .title("Install from _File…")
        .use_underline(true)
        .start_icon_name("package-x-generic-symbolic")
        .build();
    let unpacked = adw::ButtonRow::builder()
        .title("Load _Unpacked Extension…")
        .use_underline(true)
        .start_icon_name("folder-symbolic")
        .build();
    let progress = adw::ActionRow::builder()
        .title("Installing…")
        .visible(false)
        .build();
    progress.add_prefix(&adw::Spinner::new());
    let install = adw::PreferencesGroup::builder()
        .title("Install")
        .description("Paste a store link or id, choose a .crx or .xpi file, or load a folder for development")
        .build();
    install.add(&source);
    install.add(&from_file);
    install.add(&unpacked);
    install.add(&progress);

    let installed = adw::PreferencesGroup::builder().title("Installed").build();
    let page = adw::PreferencesPage::new();
    page.add(&install);
    page.add(&installed);
    let dialog = adw::PreferencesDialog::builder().title("Extensions").build();
    dialog.add(&page);

    let state = Rc::new(State {
        window: window.downgrade(),
        dialog,
        progress,
        installed,
        rows: RefCell::new(Vec::new()),
    });
    state.refresh();

    source.connect_apply(glib::clone!(
        #[strong]
        state,
        move |row| {
            let text = row.text().trim().to_owned();
            if !text.is_empty() {
                state.install(InstallSource::parse(&text));
                row.set_text("");
            }
        }
    ));
    from_file.connect_activated(glib::clone!(
        #[strong]
        state,
        move |_| state.choose_file()
    ));
    unpacked.connect_activated(glib::clone!(
        #[strong]
        state,
        move |_| state.choose_folder()
    ));
    state.dialog.present(Some(window));
}

impl State {
    fn browser(&self) -> Option<Browser> {
        self.window.upgrade().map(|window| window.browser().clone())
    }

    fn toast(&self, text: &str) {
        self.dialog.add_toast(adw::Toast::new(text));
    }

    fn install(self: &Rc<Self>, source: Result<InstallSource, SourceParseError>) {
        let source = match source {
            Ok(source) => source,
            Err(e) => {
                self.toast(&e.to_string());
                return;
            }
        };
        let Some(browser) = self.browser() else { return };
        self.progress.set_subtitle("Starting…");
        self.progress.set_visible(true);
        let state = self.clone();
        glib::spawn_future_local(async move {
            let progress = {
                let row = state.progress.clone();
                progress_to(move |text| row.set_subtitle(&text))
            };
            let result = browser.install(source, progress).await;
            state.progress.set_visible(false);
            match result {
                Ok(Some(ext)) => state.toast(&format!("Installed {} {}", ext.manifest.name, ext.version)),
                Ok(None) => state.toast("The extension was removed elsewhere while it downloaded"),
                Err(InstallFailure::Load(ext, e)) => state.toast(&format!(
                    "Installed {} {}, but it cannot run: {e}",
                    ext.manifest.name, ext.version
                )),
                Err(e) => state.toast(&format!("Install failed: {e}")),
            }
            state.refresh();
        });
    }

    fn choose_file(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else { return };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Extension packages (.crx, .xpi)"));
        filter.add_suffix("crx");
        filter.add_suffix("xpi");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title("Install Extension from File")
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        chooser.open(
            Some(&window),
            None::<&gio::Cancellable>,
            glib::clone!(
                #[strong(rename_to = state)]
                self,
                move |result| match result.ok().and_then(|file| file.path()) {
                    Some(path) => state.install(InstallSource::from_path(&path)),
                    None => log::debug!("no extension file chosen"),
                }
            ),
        );
    }

    fn choose_folder(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else { return };
        let chooser = gtk::FileDialog::builder()
            .title("Load Unpacked Extension")
            .modal(true)
            .build();
        chooser.select_folder(
            Some(&window),
            None::<&gio::Cancellable>,
            glib::clone!(
                #[strong(rename_to = state)]
                self,
                move |result| match result.ok().and_then(|folder| folder.path()) {
                    Some(path) => state.install(InstallSource::from_path(&path)),
                    None => log::debug!("no extension folder chosen"),
                }
            ),
        );
    }

    fn refresh(self: &Rc<Self>) {
        for row in self.rows.take() {
            self.installed.remove(&row);
        }
        let Some(browser) = self.browser() else { return };
        let extensions = browser.installed_extensions();
        let mut rows: Vec<gtk::Widget> = Vec::with_capacity(extensions.len().max(1));
        if extensions.is_empty() {
            let none = adw::ActionRow::builder()
                .title("No extensions installed")
                .css_classes(["dim-label"])
                .build();
            self.installed.add(&none);
            rows.push(none.upcast());
        }
        for ext in extensions {
            let error = browser.extension_error(&ext.id).filter(|_| ext.enabled);
            let row = self.extension_row(&ext, error);
            self.installed.add(&row);
            rows.push(row.upcast());
        }
        self.rows.replace(rows);
    }

    fn extension_row(self: &Rc<Self>, ext: &InstalledExtension, error: Option<String>) -> adw::ExpanderRow {
        let row = adw::ExpanderRow::builder()
            .title(&ext.manifest.name)
            .subtitle(format!("{} · {}", ext.version, describe_verification(&ext.verification)))
            .use_markup(false)
            .build();
        row.add_prefix(&icon_image(icon_path(ext).as_deref()));

        let enabled = gtk::Switch::builder()
            .active(ext.enabled)
            .valign(gtk::Align::Center)
            .tooltip_text("Enabled")
            .build();
        enabled.connect_state_set(glib::clone!(
            #[strong(rename_to = state)]
            self,
            #[strong(rename_to = id)]
            ext.id,
            move |_, active| {
                let Some(browser) = state.browser() else { return glib::Propagation::Stop };
                let failed_before = browser.extension_error(&id).is_some();
                match browser.set_extension_enabled(&id, active) {
                    Ok(()) => {}
                    Err(EnableFailure::Load(e)) => state.toast(&format!("Enabled, but it cannot run: {e}")),
                    Err(e) => {
                        state.toast(&format!("Cannot change the extension: {e}"));
                        return glib::Propagation::Stop;
                    }
                }
                if failed_before != browser.extension_error(&id).is_some() {
                    // After this handler returns, so the switch is not replaced under it.
                    let state = state.clone();
                    glib::idle_add_local_once(move || state.refresh());
                }
                glib::Propagation::Proceed
            }
        ));
        row.add_suffix(&enabled);

        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        remove.connect_clicked(glib::clone!(
            #[strong(rename_to = state)]
            self,
            #[strong(rename_to = id)]
            ext.id,
            move |_| {
                let Some(browser) = state.browser() else { return };
                if let Err(e) = browser.uninstall_extension(&id) {
                    state.toast(&format!("Cannot remove the extension: {e}"));
                }
                state.refresh();
            }
        ));
        row.add_suffix(&remove);

        if let Some(notice) = unsupported_notice(&ext.manifest) {
            let unsupported = adw::ActionRow::builder()
                .title("Not supported by the Linux runtime")
                .subtitle(format!("This extension requests: {notice}. Features that depend on them will not work."))
                .subtitle_lines(0)
                .use_markup(false)
                .css_classes(["warning"])
                .build();
            unsupported.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
            row.add_row(&unsupported);
            row.set_expanded(true);
        }
        if let Some(error) = error {
            let failed = adw::ActionRow::builder()
                .title("Not running")
                .subtitle(error)
                .subtitle_lines(0)
                .use_markup(false)
                .css_classes(["error"])
                .build();
            failed.add_prefix(&gtk::Image::from_icon_name("dialog-error-symbolic"));
            row.add_row(&failed);
            row.set_expanded(true);
        }
        let id_row = adw::ActionRow::builder()
            .title("ID")
            .subtitle(ext.id.as_str())
            .subtitle_selectable(true)
            .use_markup(false)
            .css_classes(["property"])
            .build();
        row.add_row(&id_row);
        let dir_row = adw::ActionRow::builder()
            .title("Location")
            .subtitle(ext.dir.display().to_string())
            .subtitle_selectable(true)
            .subtitle_lines(0)
            .use_markup(false)
            .css_classes(["property"])
            .build();
        row.add_row(&dir_row);
        row
    }
}

fn icon_image(path: Option<&Path>) -> gtk::Image {
    let image = match path.and_then(|p| gtk::gdk::Texture::from_filename(p).ok()) {
        Some(texture) => gtk::Image::from_paintable(Some(&texture)),
        None => gtk::Image::from_icon_name("application-x-addon-symbolic"),
    };
    image.set_pixel_size(32);
    image
}
