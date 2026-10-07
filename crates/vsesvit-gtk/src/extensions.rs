//! Extension installs and the runtime's view of the installed set.
//!
//! An install is core's three-step pipeline: `prepare_install` on the UI thread,
//! `InstallJob::run` on a worker thread (`gio::spawn_blocking`) with its progress relayed
//! back over a channel, and `commit` on the UI thread again, after which the runtime
//! loads the extension. A user's install the extensions dialog starts waits between the last
//! two for Chrome's install prompt, which lists what the extension can do.
//!
//! An update check runs the same way: `prepare_update_check`, `UpdateCheck::run` on a worker
//! thread, `commit_updates`, then the runtime reloads each updated extension at its new
//! version, or stops one that waits for the user to re-enable it.
//!
//! Every change to the enabled extensions applies the keymap again, which binds their commands.

use std::fmt;
use std::rc::Rc;

use futures_channel::mpsc;
use futures_util::StreamExt;
use gtk::{gio, glib};
use vsesvit_core::extensions::manifest::Manifest;
use adw::prelude::*;
use vsesvit_core::extensions::{
    ExtensionId, InstallError, InstallJob, InstallPhase, InstallSource, InstalledExtension, StagedInstall, UPDATE_INTERVAL, UpdateReport,
};
use vsesvit_core::private::Browsing;
use vsesvit_webext::{LoadError, Unsupported};

use crate::browser::Browser;
use crate::dialogs::extension_prompts;
use crate::window::BrowserWindow;

#[derive(Debug)]
pub(crate) enum InstallFailure {
    Prepare(vsesvit_core::Error),
    Run(InstallError),
    Commit(vsesvit_core::Error),
    /// Committed to the profile, but the runtime cannot run it (a file the manifest names
    /// is missing or unreadable).
    Load(Box<InstalledExtension>, LoadError),
    WorkerPanicked,
    /// The user cancelled the install prompt.
    Cancelled,
}

impl fmt::Display for InstallFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallFailure::Prepare(e) | InstallFailure::Commit(e) => write!(f, "{e}"),
            InstallFailure::Run(e) => write!(f, "{e}"),
            InstallFailure::Load(ext, e) => {
                write!(f, "{} {} is installed but cannot run: {e}", ext.manifest.name, ext.version)
            }
            InstallFailure::WorkerPanicked => write!(f, "the install thread panicked"),
            InstallFailure::Cancelled => write!(f, "the install was cancelled"),
        }
    }
}

impl std::error::Error for InstallFailure {}

/// Why an extension could not be switched on or off.
#[derive(Debug)]
pub(crate) enum EnableFailure {
    Core(vsesvit_core::Error),
    /// The switch is stored, but the runtime cannot run the extension.
    Load(LoadError),
}

impl fmt::Display for EnableFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnableFailure::Core(e) => write!(f, "{e}"),
            EnableFailure::Load(e) => write!(f, "it cannot run: {e}"),
        }
    }
}

impl std::error::Error for EnableFailure {}

impl From<vsesvit_core::Error> for EnableFailure {
    fn from(e: vsesvit_core::Error) -> Self {
        EnableFailure::Core(e)
    }
}

/// Why an update check did not run to the end.
#[derive(Debug)]
pub(crate) enum UpdateFailure {
    /// Another check is running; what it updates shows when it ends.
    Running,
    Prepare(vsesvit_core::Error),
    WorkerPanicked,
}

impl fmt::Display for UpdateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateFailure::Running => write!(f, "an update check is already running"),
            UpdateFailure::Prepare(e) => write!(f, "{e}"),
            UpdateFailure::WorkerPanicked => write!(f, "the update thread panicked"),
        }
    }
}

impl std::error::Error for UpdateFailure {}

impl Browser {
    /// Installs from a parsed source. `Ok(None)` means the extension was uninstalled on
    /// another device while this one downloaded it (reconcile installs only).
    pub(crate) async fn install(
        &self,
        source: InstallSource,
        progress: impl Fn(InstallPhase) + 'static,
    ) -> Result<Option<InstalledExtension>, InstallFailure> {
        let prepared = self.core().borrow_mut().extensions().prepare_install(source);
        let job = prepared.map_err(InstallFailure::Prepare)?;
        self.run_install_job(job, progress).await
    }

    /// [`Browser::install`] that asks the user over `window` first, with Chrome's install
    /// prompt, where core says Chrome would.
    pub(crate) async fn install_asking(
        &self,
        window: &BrowserWindow,
        source: InstallSource,
        progress: impl Fn(InstallPhase) + 'static,
    ) -> Result<Option<InstalledExtension>, InstallFailure> {
        let prepared = self.core().borrow_mut().extensions().prepare_install(source);
        let staged = self.stage(prepared.map_err(InstallFailure::Prepare)?, progress).await?;
        if staged.needs_approval() {
            let dialog = extension_prompts::install_dialog(staged.manifest());
            if dialog.choose_future(Some(window)).await != extension_prompts::ADD {
                return Err(InstallFailure::Cancelled);
            }
        }
        self.commit_install(staged)
    }

    /// Runs a prepared job off the UI thread, commits it, and hands the result to the
    /// runtime.
    pub(crate) async fn run_install_job(
        &self,
        job: InstallJob,
        progress: impl Fn(InstallPhase) + 'static,
    ) -> Result<Option<InstalledExtension>, InstallFailure> {
        let staged = self.stage(job, progress).await?;
        self.commit_install(staged)
    }

    async fn stage(&self, job: InstallJob, progress: impl Fn(InstallPhase) + 'static) -> Result<StagedInstall, InstallFailure> {
        let (tx, mut rx) = mpsc::unbounded::<InstallPhase>();
        let handle = gio::spawn_blocking(move || {
            job.run(&mut |phase| {
                let _ = tx.unbounded_send(phase);
            })
        });
        // The worker's sender drops when the job ends, which ends this pump.
        let pump = glib::spawn_future_local(async move {
            while let Some(phase) = rx.next().await {
                progress(phase);
            }
        });
        let staged = handle.await.map_err(|_| InstallFailure::WorkerPanicked)?;
        let _ = pump.await;
        staged.map_err(InstallFailure::Run)
    }

    fn commit_install(&self, staged: StagedInstall) -> Result<Option<InstalledExtension>, InstallFailure> {
        let committed = self.core().borrow_mut().extensions().commit(staged);
        let committed = committed.map_err(InstallFailure::Commit)?;
        self.apply_keymap();
        if let Some(ext) = &committed
            && let Err(e) = self.load_into_runtime(ext)
        {
            return Err(InstallFailure::Load(Box::new(ext.clone()), e));
        }
        Ok(committed)
    }

    /// Starts every install `reconcile()` asks for and unloads what it removed.
    pub(crate) fn reconcile_extensions(&self) {
        let work = self.core().borrow_mut().extensions().reconcile();
        let work = match work {
            Ok(work) => work,
            Err(e) => {
                log::warn!("cannot reconcile extensions: {e}");
                return;
            }
        };
        for id in &work.removed {
            self.unload_from_runtime(id);
        }
        self.apply_keymap();
        for job in work.install {
            let browser = self.clone();
            let source = format!("{:?}", job.source());
            glib::spawn_future_local(async move {
                match browser.run_install_job(job, |_| {}).await {
                    Ok(Some(ext)) => log::info!("installed {} from another device's list", ext.id.as_str()),
                    Ok(None) => {}
                    Err(e) => log::warn!("reconcile install from {source} failed: {e}"),
                }
            });
        }
    }

    pub(crate) fn installed_extensions(&self) -> Vec<InstalledExtension> {
        let listed = self.core().borrow_mut().extensions().list();
        listed.unwrap_or_else(|e| {
            log::warn!("cannot list extensions: {e}");
            Vec::new()
        })
    }

    pub(crate) fn set_extension_enabled(
        &self,
        id: &ExtensionId,
        enabled: bool,
    ) -> Result<(), EnableFailure> {
        self.core().borrow_mut().extensions().set_enabled(id, enabled)?;
        self.extension_changed(id)
    }

    /// The user re-enabled an extension an update turned off, so it runs again unless they
    /// had turned it off.
    pub(crate) fn approve_extension_permissions(&self, id: &ExtensionId) -> Result<(), EnableFailure> {
        self.core().borrow_mut().extensions().approve_permissions(id)?;
        self.extension_changed(id)
    }

    /// Runs `id` or stops it as the profile now has it.
    fn extension_changed(&self, id: &ExtensionId) -> Result<(), EnableFailure> {
        self.apply_keymap();
        let ext = self.core().borrow_mut().extensions().get(id)?;
        match ext {
            Some(ext) => self.load_into_runtime(&ext).map_err(EnableFailure::Load),
            None => {
                self.unload_from_runtime(id);
                Ok(())
            }
        }
    }

    /// Checks every store extension for a newer version, as Chrome does every few hours and
    /// when the user presses Update, and runs what it installed. A load failure is logged and
    /// shown on the extensions page, as after an install.
    pub(crate) async fn update_extensions(&self) -> Result<UpdateReport, UpdateFailure> {
        if self.updating_extensions().get() {
            return Err(UpdateFailure::Running);
        }
        let check = self.core().borrow_mut().extensions().prepare_update_check();
        let check = check.map_err(UpdateFailure::Prepare)?;
        if check.is_empty() {
            return Ok(UpdateReport::default());
        }
        self.updating_extensions().set(true);
        let updates = gio::spawn_blocking(move || check.run()).await;
        self.updating_extensions().set(false);
        let updates = updates.map_err(|_| UpdateFailure::WorkerPanicked)?;
        let report = self.core().borrow_mut().extensions().commit_updates(updates);
        if !report.updated.is_empty() {
            self.apply_keymap();
            for ext in &report.updated {
                let _ = self.load_into_runtime(ext);
            }
            self.extensions_updated();
        }
        Ok(report)
    }

    /// The periodic update checks, for as long as the browser lives: the first a minute after
    /// startup, then every few hours. What they do shows only in the log.
    pub(crate) fn schedule_extension_updates(&self) {
        let browser = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            loop {
                let Some(inner) = browser.upgrade() else { return };
                let due = Browser(inner).core().borrow_mut().extensions().next_update_check();
                let due = due.unwrap_or_else(|e| {
                    log::warn!("cannot schedule the extension update check: {e}");
                    UPDATE_INTERVAL
                });
                glib::timeout_future(due).await;
                let Some(inner) = browser.upgrade() else { return };
                match Browser(inner).update_extensions().await {
                    Ok(report) => log::info!("extension update check: {}", report.summary()),
                    Err(e) => log::warn!("extension update check failed: {e}"),
                }
            }
        });
    }

    /// The user's "Allow in private windows" for `id`: it joins or leaves the open private tabs
    /// at once, and the private windows' toolbars follow.
    pub(crate) fn set_extension_allowed_in_private(&self, id: &ExtensionId, allowed: bool) -> Result<(), vsesvit_core::Error> {
        self.core().borrow_mut().extensions().set_allowed_in_private(id, allowed)?;
        self.runtime().allowed_in_private_changed();
        for window in self.windows_of(Browsing::Private) {
            window.refresh_extension_actions();
        }
        Ok(())
    }

    /// Stops the extension first, so the runtime never runs files the uninstall removes. An
    /// uninstall the profile refuses runs it again as the profile still has it.
    pub(crate) fn uninstall_extension(&self, id: &ExtensionId) -> Result<(), vsesvit_core::Error> {
        self.unload_from_runtime(id);
        let removed = self.core().borrow_mut().extensions().uninstall(id);
        self.apply_keymap();
        if removed.is_err() {
            let kept = self.core().borrow_mut().extensions().get(id);
            if let Ok(Some(ext)) = kept {
                // A load failure is kept for the extensions page.
                let _ = self.load_into_runtime(&ext);
            }
        }
        removed
    }

    /// Runs an enabled extension, or stops a disabled one. A load failure is also kept for
    /// the extensions page, which shows the extension as not running.
    pub(crate) fn load_into_runtime(&self, ext: &InstalledExtension) -> Result<(), LoadError> {
        if !ext.enabled {
            self.unload_from_runtime(&ext.id);
            return Ok(());
        }
        let loaded = self.runtime().load(ext);
        let error = loaded.as_ref().err().map(|e| {
            log::warn!("extension {}: {e}", ext.id.as_str());
            e.to_string()
        });
        self.set_extension_error(&ext.id, error);
        loaded
    }

    fn unload_from_runtime(&self, id: &ExtensionId) {
        self.runtime().unload(id);
        self.set_extension_error(id, None);
    }
}

/// The notice under an extension whose manifest asks for things the Linux runtime lacks.
pub(crate) fn unsupported_notice(manifest: &Manifest) -> Option<String> {
    let unsupported = vsesvit_webext::unsupported_features(manifest);
    if unsupported.is_empty() {
        return None;
    }
    let names: Vec<String> = unsupported.iter().map(Unsupported::to_string).collect();
    Some(names.join(", "))
}

/// The largest icon the manifest declares, for the extensions page.
pub(crate) fn icon_path(ext: &InstalledExtension) -> Option<std::path::PathBuf> {
    ext.manifest
        .icons
        .iter()
        .next_back()
        .map(|(_, path)| path.resolve(&ext.dir))
        .filter(|path| path.is_file())
}

/// A progress callback that never has to be `Send`: the pump runs it on the UI thread.
pub(crate) fn progress_to<F: Fn(String) + 'static>(f: F) -> impl Fn(InstallPhase) + 'static {
    move |phase: InstallPhase| f(phase.describe())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_notice_lists_what_the_runtime_lacks() {
        let manifest = Manifest::parse(
            r#"{ "manifest_version": 3, "name": "x", "version": "1", "permissions": ["storage", "webRequest"] }"#,
            &|_| None,
        )
        .unwrap();
        assert_eq!(unsupported_notice(&manifest).as_deref(), Some("webRequest"));
        let clean = Manifest::parse(r#"{ "manifest_version": 3, "name": "x", "version": "1" }"#, &|_| None).unwrap();
        assert_eq!(unsupported_notice(&clean), None);
    }

    #[gtk::test]
    async fn an_extension_the_runtime_cannot_load_is_not_reported_as_installed() {
        use crate::test_support::{browser, scratch_dir};

        let browser = browser();
        let dir = scratch_dir("broken-extension");
        let manifest = r#"{ "manifest_version": 3, "name": "Broken", "version": "1.0",
            "content_scripts": [{ "matches": ["<all_urls>"], "js": ["missing.js"] }] }"#;
        std::fs::write(dir.join("manifest.json"), manifest).unwrap();
        let source = InstallSource::from_path(&dir).unwrap();
        let installed = browser.install(source, |_| {}).await;
        assert!(installed.is_err(), "the install reported success: {installed:?}");

        let id = browser
            .installed_extensions()
            .into_iter()
            .find(|e| e.manifest.name == "Broken")
            .map(|e| e.id)
            .expect("core committed the extension");
        assert!(browser.set_extension_enabled(&id, false).is_ok());
        assert!(browser.set_extension_enabled(&id, true).is_err(), "enabling it reported success");
        assert!(!browser.runtime().loaded().contains(&id));
        let shown = browser.extension_error(&id);
        assert!(shown.as_deref().is_some_and(|e| e.contains("missing.js")), "the page shows {shown:?}");
        browser.set_extension_enabled(&id, false).unwrap();
        assert_eq!(browser.extension_error(&id), None);
    }

    #[gtk::test]
    async fn an_uninstall_the_profile_refuses_keeps_the_extension_running() {
        use crate::test_support::{browser, scratch_dir};

        let browser = browser();
        let dir = scratch_dir("uninstall-refused");
        std::fs::write(dir.join("manifest.json"), r#"{ "manifest_version": 3, "name": "Kept", "version": "1.0" }"#).unwrap();
        let installed = browser.install(InstallSource::from_path(&dir).unwrap(), |_| {}).await;
        let id = installed.expect("the extension installs").expect("and is committed").id;
        assert!(browser.runtime().loaded().contains(&id));

        let db = browser.core().borrow().paths().db.clone();
        let blocker = rusqlite::Connection::open(db).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(browser.uninstall_extension(&id).is_err(), "the profile is locked");
        assert!(browser.runtime().loaded().contains(&id), "the extension stopped");

        blocker.execute_batch("COMMIT").unwrap();
        browser.uninstall_extension(&id).unwrap();
        assert!(!browser.runtime().loaded().contains(&id));
    }
}
