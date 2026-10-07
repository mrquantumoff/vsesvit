//! The Extensions dialog: install from a store link or id, a `.crx`/`.xpi` file or an
//! unpacked folder, with the job's progress shown and Chrome's install prompt before a store
//! or package install goes in; the installed list with icon, name, version, provenance, an
//! enabled switch and removal, and Chrome's Update, which checks them all for newer versions
//! now; and, per extension, what its manifest asks for that the Linux runtime does not
//! provide, the new permissions an update waits for the user to approve, whether its
//! notifications may show and whether it runs in private windows.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::extensions::{ExtensionId, InstallSource, InstalledExtension, SourceParseError};

use super::plain_toast;
use crate::browser::Browser;
use crate::extensions::{
    EnableFailure, InstallFailure, icon_path, progress_to, unsupported_notice,
};
use crate::window::BrowserWindow;

pub(crate) const ALLOW_IN_PRIVATE_ROW: &str = "Allow in private windows";
/// The widget name of the Update button.
pub(crate) const UPDATE_BUTTON: &str = "update-extensions";

/// Holds no [`Browser`] of its own: the widgets' handlers keep this state alive for as long
/// as the dialog's widgets exist, which must not keep the profile open.
struct State {
    window: glib::WeakRef<BrowserWindow>,
    dialog: adw::PreferencesDialog,
    progress: adw::ActionRow,
    /// Installs still running: two can overlap, and they share the progress row.
    installing: Cell<u32>,
    installed: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
    update: gtk::Button,
    /// The extension whose row opens expanded.
    shown: Option<ExtensionId>,
    /// Kept for [`Browser::watch_extensions`], which holds it weakly.
    watch: OnceCell<Rc<dyn Fn()>>,
}

pub(crate) fn present(window: &BrowserWindow) {
    build(window, None).dialog.present(Some(window));
}

/// The dialog with `id`'s row expanded, which is where a notification's Settings button leads,
/// as Chrome's leads to the extension's settings.
pub(crate) fn present_extension(window: &BrowserWindow, id: &ExtensionId) {
    build(window, Some(id.clone())).dialog.present(Some(window));
}

fn build(window: &BrowserWindow, shown: Option<ExtensionId>) -> Rc<State> {
    let source = adw::EntryRow::builder()
        .title("Chrome Web Store, Edge Add-ons or Firefox Add-ons link or ID")
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

    let update = gtk::Button::builder()
        .label("_Update")
        .use_underline(true)
        .name(UPDATE_BUTTON)
        .tooltip_text("Check every extension for a newer version now")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let installed = adw::PreferencesGroup::builder().title("Installed").header_suffix(&update).build();
    let page = adw::PreferencesPage::new();
    page.add(&install);
    page.add(&installed);
    let dialog = adw::PreferencesDialog::builder().title("Extensions").build();
    dialog.add(&page);

    let state = Rc::new(State {
        window: window.downgrade(),
        dialog,
        progress,
        installing: Cell::new(0),
        installed,
        rows: RefCell::new(Vec::new()),
        update,
        shown,
        watch: OnceCell::new(),
    });
    state.refresh();
    let weak = Rc::downgrade(&state);
    let watch: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(state) = weak.upgrade() {
            state.refresh();
        }
    });
    window.browser().watch_extensions(&watch);
    let _ = state.watch.set(watch);

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
    state.update.connect_clicked(glib::clone!(
        #[strong]
        state,
        move |_| state.update()
    ));
    state
}

impl State {
    fn browser(&self) -> Option<Browser> {
        self.window.upgrade().map(|window| window.browser().clone())
    }

    fn toast(&self, text: &str) {
        self.dialog.add_toast(plain_toast(text));
    }

    fn install(self: &Rc<Self>, source: Result<InstallSource, SourceParseError>) {
        let source = match source {
            Ok(source) => source,
            Err(e) => {
                self.toast(&e.to_string());
                return;
            }
        };
        let Some(window) = self.window.upgrade() else { return };
        self.install_started();
        let state = self.clone();
        glib::spawn_future_local(async move {
            let progress = {
                let row = state.progress.clone();
                progress_to(move |text| row.set_subtitle(&text))
            };
            let result = window.browser().clone().install_asking(&window, source, progress).await;
            state.install_finished();
            match result {
                Err(InstallFailure::Cancelled) => {}
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

    /// Chrome's Update. The rows follow through [`Browser::watch_extensions`].
    fn update(self: &Rc<Self>) {
        let Some(browser) = self.browser() else { return };
        self.update.set_sensitive(false);
        let state = self.clone();
        glib::spawn_future_local(async move {
            let result = browser.update_extensions().await;
            state.update.set_sensitive(true);
            match result {
                Ok(report) => state.toast(&report.summary()),
                Err(e) => state.toast(&format!("Cannot update the extensions: {e}")),
            }
        });
    }

    fn install_started(&self) {
        self.installing.set(self.installing.get() + 1);
        self.progress.set_subtitle("Starting…");
        self.progress.set_visible(true);
    }

    fn install_finished(&self) {
        let left = self.installing.get() - 1;
        self.installing.set(left);
        self.progress.set_visible(left > 0);
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
            .subtitle(format!("{} · {}", ext.version, ext.verification.label()))
            .use_markup(false)
            .build();
        row.add_prefix(&icon_image(icon_path(ext).as_deref()));
        row.set_expanded(self.shown.as_ref() == Some(&ext.id));

        let enabled = gtk::Switch::builder()
            .active(ext.enabled)
            .sensitive(ext.withheld.is_empty())
            .valign(gtk::Align::Center)
            .tooltip_text("Enabled")
            .build();
        enabled.connect_state_set(glib::clone!(
            #[strong(rename_to = state)]
            self,
            #[strong(rename_to = id)]
            ext.id,
            move |switch, active| {
                let Some(browser) = state.browser() else {
                    // Back to the state it shows, after this handler returns.
                    let switch = switch.clone();
                    glib::idle_add_local_once(move || switch.set_active(switch.state()));
                    return glib::Propagation::Stop;
                };
                let failed_before = browser.extension_error(&id).is_some();
                match browser.set_extension_enabled(&id, active) {
                    Ok(()) => {}
                    Err(EnableFailure::Load(e)) => state.toast(&format!("Enabled, but it cannot run: {e}")),
                    Err(e) => {
                        state.toast(&format!("Cannot change the extension: {e}"));
                        // Shows what the profile holds, or no row for an extension gone from it.
                        let state = state.clone();
                        glib::idle_add_local_once(move || state.refresh());
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

        let remove = super::row_button("user-trash-symbolic", "Remove");
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
        if let Some(notice) = ext.approval_notice() {
            row.add_row(&self.approval_row(&ext.id, notice));
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
        if ext.manifest.permissions.iter().any(|p| p == "notifications") {
            row.add_row(&self.notifications_row(&ext.id));
        }
        let in_private = adw::SwitchRow::builder()
            .title(ALLOW_IN_PRIVATE_ROW)
            .subtitle("Vsesvit cannot stop it from recording what you do there")
            .active(self.browser().is_some_and(|b| b.core().borrow_mut().extensions().allowed_in_private(&ext.id)))
            .build();
        in_private.connect_active_notify(glib::clone!(
            #[strong(rename_to = state)]
            self,
            #[strong(rename_to = id)]
            ext.id,
            move |row| {
                let Some(browser) = state.browser() else { return };
                if let Err(e) = browser.set_extension_allowed_in_private(&id, row.is_active()) {
                    state.toast(&format!("Cannot change the extension: {e}"));
                }
            }
        ));
        row.add_row(&in_private);
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

    /// What an update turned the extension off for, and the button that approves it.
    fn approval_row(self: &Rc<Self>, id: &ExtensionId, notice: String) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title("Needs your approval")
            .subtitle(notice)
            .subtitle_lines(0)
            .use_markup(false)
            .css_classes(["warning"])
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        let approve = gtk::Button::builder().label("_Approve").use_underline(true).valign(gtk::Align::Center).build();
        approve.connect_clicked(glib::clone!(
            #[strong(rename_to = state)]
            self,
            #[strong]
            id,
            move |_| {
                let Some(browser) = state.browser() else { return };
                match browser.approve_extension_permissions(&id) {
                    Ok(()) => {}
                    Err(EnableFailure::Load(e)) => state.toast(&format!("Approved, but it cannot run: {e}")),
                    Err(e) => state.toast(&format!("Cannot approve the permissions: {e}")),
                }
                state.refresh();
            }
        ));
        row.add_suffix(&approve);
        row
    }

    /// Chrome's per-extension notification switch: off, the extension's notifications close
    /// and `chrome.notifications` refuses to show more.
    fn notifications_row(self: &Rc<Self>, id: &ExtensionId) -> adw::SwitchRow {
        let allowed = self.browser().is_some_and(|browser| browser.core().borrow_mut().extensions().notifications_allowed(id));
        let row = adw::SwitchRow::builder().title("Notifications").subtitle("Let this extension show notifications").active(allowed).build();
        row.connect_active_notify(glib::clone!(
            #[strong(rename_to = state)]
            self,
            #[strong]
            id,
            move |row| {
                let Some(browser) = state.browser() else { return };
                let stored = browser.core().borrow_mut().extensions().set_notifications_allowed(&id, row.is_active());
                match stored {
                    Ok(()) => browser.runtime().notification_permission_changed(&id),
                    Err(e) => {
                        state.toast(&format!("Cannot change the extension's notifications: {e}"));
                        let state = state.clone();
                        glib::idle_add_local_once(move || state.refresh());
                    }
                }
            }
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{browser, scratch_dir};

    /// The extensions' Enabled switches, not the row's own hidden one.
    fn switches(widget: &gtk::Widget) -> Vec<gtk::Switch> {
        let enabled = widget.downcast_ref::<gtk::Switch>().filter(|s| s.tooltip_text().as_deref() == Some("Enabled"));
        let mut found: Vec<gtk::Switch> = enabled.cloned().into_iter().collect();
        for child in std::iter::successors(widget.first_child(), |child| child.next_sibling()) {
            found.extend(switches(&child));
        }
        found
    }

    fn row_titled(state: &State, title: &str) -> Option<gtk::Widget> {
        let is_titled = |row: &&gtk::Widget| row.downcast_ref::<adw::ExpanderRow>().is_some_and(|r| r.title() == title);
        state.rows.borrow().iter().find(is_titled).cloned()
    }

    #[gtk::test]
    async fn the_private_windows_switch_lets_the_extension_into_private_tabs() {
        use vsesvit_core::private::Browsing;

        let browser = browser();
        let dir = scratch_dir("private-switch");
        std::fs::write(dir.join("manifest.json"), r#"{ "manifest_version": 3, "name": "Private switch", "version": "1.0" }"#).unwrap();
        let installed = browser.install(InstallSource::from_path(&dir).unwrap(), |_| {}).await;
        let id = installed.expect("the extension installs").expect("and is committed").id;
        let window = BrowserWindow::new(&browser);
        let state = build(&window, None);
        let row = row_titled(&state, "Private switch").expect("the extension's row");
        let switch = descendants(&row).into_iter().find_map(|w| w.downcast::<adw::SwitchRow>().ok()).expect("its private windows switch");
        let state_of = || (browser.core().borrow_mut().extensions().allowed_in_private(&id), browser.runtime().runs_in(&id, Browsing::Private));
        let before = (switch.title(), switch.is_active(), state_of());
        switch.set_active(true);
        let allowed = state_of();
        switch.set_active(false);
        let disallowed = state_of();
        browser.uninstall_extension(&id).ok();
        window.destroy();
        assert_eq!(before, (ALLOW_IN_PRIVATE_ROW.into(), false, (false, false)), "off until the user turns it on");
        assert_eq!(allowed, (true, true));
        assert_eq!(disallowed, (false, false));
    }

    fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        std::iter::successors(widget.first_child(), |child| child.next_sibling())
            .flat_map(|child| std::iter::once(child.clone()).chain(descendants(&child)))
            .collect()
    }

    #[gtk::test]
    async fn an_update_asking_for_new_permissions_waits_for_the_approve_button() {
        use std::time::Duration;

        use vsesvit_core::extensions::Stores;
        use vsesvit_core::testkit::{CrxKey, FixtureServer, FixtureStore, update_probe_files};

        let browser = browser();
        let server = FixtureServer::start().unwrap();
        let store = FixtureStore::start(&server);
        browser.core().borrow_mut().set_stores(store.stores());
        let id = store.publish_crx(&update_probe_files("1.0", &["storage"]), &CrxKey::second());
        let installed = browser.install(InstallSource::ChromeWebStore { id: id.clone() }, |_| {}).await;
        assert!(installed.is_ok_and(|ext| ext.is_some()), "the extension installs from the store");
        let window = BrowserWindow::new(&browser);
        let state = build(&window, None);

        store.publish_crx(&update_probe_files("2.0", &["storage", "tabs"]), &CrxKey::second());
        let (first, second) = futures_util::future::join(browser.update_extensions(), browser.update_extensions()).await;
        let updated: Vec<(String, bool)> = first.expect("the first check runs").updated.iter().map(|ext| (ext.version.clone(), ext.enabled)).collect();
        let second = second.err().map(|e| e.to_string());
        let row = row_titled(&state, "Vsesvit update probe").expect("the extension's row");
        let switch = switches(&row).pop().expect("its switch");
        let withheld = (switch.is_sensitive(), switch.is_active(), browser.runtime().loaded().contains(&id));

        state.update.emit_clicked();
        let sensitive_while_running = state.update.is_sensitive();
        while !state.update.is_sensitive() {
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let approve = |row: &gtk::Widget| descendants(row).into_iter().find_map(|w| w.downcast::<gtk::Button>().ok().filter(|b| b.label().as_deref() == Some("_Approve")));
        let row = row_titled(&state, "Vsesvit update probe").expect("the extension's row");
        approve(&row).expect("its Approve button").emit_clicked();
        let approved = browser.runtime().loaded().contains(&id);
        let row = row_titled(&state, "Vsesvit update probe").expect("the extension's row");
        let switch = switches(&row).pop().expect("its switch");
        let shown = (switch.is_sensitive(), switch.is_active(), approve(&row).is_some());

        browser.uninstall_extension(&id).ok();
        browser.core().borrow_mut().set_stores(Stores::default());
        window.destroy();
        assert_eq!(updated, [("2.0".to_owned(), false)]);
        assert_eq!(second.as_deref(), Some("an update check is already running"));
        assert_eq!(withheld, (false, false, false), "the open dialog shows the switch insensitive and off, and the extension is not running");
        assert!(!sensitive_while_running, "the Update button stays insensitive while its check runs");
        assert!(approved, "Approve runs it again");
        assert_eq!(shown, (true, true, false));
    }

    #[gtk::test]
    fn the_progress_row_stays_while_another_install_runs() {
        let window = BrowserWindow::new(&browser());
        let state = build(&window, None);
        state.install_started();
        state.install_started();
        state.install_finished();
        assert!(state.progress.is_visible(), "the install still running shows no progress");
        state.install_finished();
        assert!(!state.progress.is_visible());
        window.destroy();
    }

    #[gtk::test]
    async fn a_switch_that_cannot_change_the_extension_shows_what_the_profile_holds() {
        let browser = browser();
        let dir = scratch_dir("switch-fails");
        let manifest = r#"{ "manifest_version": 3, "name": "Switched", "version": "1.0" }"#;
        std::fs::write(dir.join("manifest.json"), manifest).unwrap();
        let installed = browser.install(InstallSource::from_path(&dir).unwrap(), |_| {}).await;
        let id = installed.expect("the extension installs").expect("and is committed").id;
        let window = BrowserWindow::new(&browser);
        let state = build(&window, None);
        let row = row_titled(&state, "Switched").expect("the extension's row");
        let switch = switches(&row).pop().expect("its switch");
        assert!(switch.is_active() && switch.state());

        // Removed elsewhere, say by sync, while the dialog shows it.
        browser.core().borrow_mut().extensions().uninstall(&id).unwrap();
        switch.set_active(false);
        while glib::MainContext::default().iteration(false) {}

        let shown: Vec<gtk::Switch> = state.rows.borrow().iter().flat_map(switches).collect();
        assert!(shown.iter().all(|s| s.is_active() == s.state()), "a switch is left half-way");
        assert!(row_titled(&state, "Switched").is_none(), "the list shows what the profile holds");
        browser.uninstall_extension(&id).ok();
        window.destroy();
    }
}
