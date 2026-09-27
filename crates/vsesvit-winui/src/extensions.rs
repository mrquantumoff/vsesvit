//! Extensions on Windows. vsesvit-core owns which extensions are installed (and where their
//! immutable folders are); WebView2 runs them and persists its own list. This module installs
//! through core's pipeline (the download and verification run on a worker thread, the commit on
//! the UI thread) and brings WebView2's list in line with core's (`sync_extensions`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use vsesvit_core::extensions::{
    ExtensionId, InstallJob, InstallPhase, InstallSource, InstalledExtension, Verification,
};

use crate::browser::Browser;
use crate::popup::ExtensionAction;
use crate::{engine, exec};

/// WebView2 calls that never answer (their web view closed meanwhile) must not stall the loop.
const ENGINE_CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// Adding a large extension makes the engine index its rulesets.
const ENGINE_ADD_TIMEOUT: Duration = Duration::from_secs(90);
const SYNC_WAIT: Duration = Duration::from_secs(180);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub(crate) struct ExtensionHost {
    actions: RefCell<Vec<ExtensionAction>>,
    sync_requested: Cell<u64>,
    sync_done: Cell<u64>,
    sync_running: Cell<bool>,
    /// The last sync's failure, if it failed.
    last_sync_error: RefCell<Option<String>>,
    /// Why the engine refused to load an extension, from the last sync.
    engine_errors: RefCell<HashMap<ExtensionId, String>>,
    listeners: RefCell<Vec<Weak<dyn Fn()>>>,
}

impl ExtensionHost {
    pub fn actions(&self) -> Vec<ExtensionAction> {
        self.actions.borrow().clone()
    }

    pub fn engine_error(&self, id: &ExtensionId) -> Option<String> {
        self.engine_errors.borrow().get(id).cloned()
    }

    /// Calls `listener` after every change to the installed set, while the caller keeps it.
    pub fn subscribe(&self, listener: &Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(Rc::downgrade(listener));
    }

    fn notify(&self) {
        let live: Vec<Rc<dyn Fn()>> = {
            let mut listeners = self.listeners.borrow_mut();
            listeners.retain(|l| l.strong_count() > 0);
            listeners.iter().filter_map(Weak::upgrade).collect()
        };
        for listener in live {
            listener();
        }
    }
}

/// Where a running install is, as the install UI shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Progress {
    pub text: String,
    /// `None` while the amount of work is unknown.
    pub fraction: Option<f64>,
}

pub(crate) fn describe(phase: &InstallPhase) -> Progress {
    const MB: f64 = 1024.0 * 1024.0;
    let text = |t: &str| Progress {
        text: t.to_owned(),
        fraction: None,
    };
    match phase {
        InstallPhase::Resolving => text("Looking up the add-on on addons.mozilla.org"),
        InstallPhase::Downloading { received, total } => {
            let received_mb = *received as f64 / MB;
            match total.filter(|t| *t > 0) {
                Some(total) => Progress {
                    text: format!(
                        "Downloading: {received_mb:.1} of {:.1} MB",
                        total as f64 / MB
                    ),
                    fraction: Some((*received as f64 / total as f64).clamp(0.0, 1.0)),
                },
                None => text(&format!("Downloading: {received_mb:.1} MB")),
            }
        }
        InstallPhase::Verifying => text("Checking the signatures"),
        InstallPhase::Unpacking => text("Unpacking"),
        InstallPhase::ReadingManifest => text("Reading the manifest"),
    }
}

/// How an install was verified, in words.
pub(crate) fn verification_label(verification: &Verification) -> &'static str {
    match verification {
        Verification::ChromeWebStore {
            publisher_verified: true,
        } => "Chrome Web Store, signed by the store",
        Verification::ChromeWebStore {
            publisher_verified: false,
        } => "Chrome Web Store",
        Verification::AmoHash => "addons.mozilla.org, checksum verified",
        Verification::LocalCrx => "Local .crx, developer signature verified",
        Verification::LocalXpi => "Local .xpi, not verified",
        Verification::Unpacked => "Unpacked folder",
    }
}

impl Browser {
    /// At startup: `--load-extension` folders, installs another device asked for, then the
    /// engine sync.
    pub(crate) async fn start_extensions(self: Rc<Self>) {
        for dir in self.config().load_extensions.clone() {
            let installed = match InstallSource::from_path(&dir) {
                Ok(source) => self.install_extension(source, &|_| {}).await,
                Err(e) => Err(e.to_string()),
            };
            match installed {
                Ok(ext) => log::info!(
                    "--load-extension: {} {} from {}",
                    ext.manifest.name,
                    ext.version,
                    dir.display()
                ),
                Err(e) => log::error!("--load-extension {}: {e}", dir.display()),
            }
        }
        match self.core(|p| p.extensions().reconcile()) {
            Ok(work) => {
                for id in &work.removed {
                    log::info!(
                        "extension {} was uninstalled on another device",
                        id.as_str()
                    );
                }
                for job in work.install {
                    let source = format!("{:?}", job.source());
                    match self.run_install_job(job, &|_| {}).await {
                        Ok(Some(ext)) => {
                            log::info!("installed {} for another device's install", ext.id.as_str())
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("reconcile install {source}: {e}"),
                    }
                }
            }
            Err(e) => log::warn!("extension reconcile: {e}"),
        }
        if let Err(e) = self.sync_extensions().await {
            log::warn!("extension sync at startup: {e}");
        }
    }

    /// Installs from `source` (the Extensions dialog, `--load-extension`) and loads the result
    /// into the engine. The engine's verdict is `engine_error`.
    pub(crate) async fn install_extension(
        self: &Rc<Self>,
        source: InstallSource,
        progress: &dyn Fn(Progress),
    ) -> Result<InstalledExtension, String> {
        let ext = self.install_package(source, progress).await?;
        if let Err(e) = self.sync_extensions().await {
            log::warn!("extension sync after install: {e}");
        }
        self.core(|p| p.extensions().get(&ext.id))
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the extension was removed meanwhile".to_owned())
    }

    /// The core half of an install: fetch, verify and unpack on a worker thread, then commit.
    /// The engine does not have it until the next `sync_extensions`.
    pub(crate) async fn install_package(
        &self,
        source: InstallSource,
        progress: &dyn Fn(Progress),
    ) -> Result<InstalledExtension, String> {
        let job = self
            .core(|p| p.extensions().prepare_install(source))
            .map_err(|e| e.to_string())?;
        let ext = self
            .run_install_job(job, progress)
            .await?
            .ok_or("the extension is no longer wanted")?;
        self.extensions.notify();
        Ok(ext)
    }

    /// `job.run` on a worker thread, `commit` here on the UI thread.
    async fn run_install_job(
        &self,
        job: InstallJob,
        progress: &dyn Fn(Progress),
    ) -> Result<Option<InstalledExtension>, String> {
        let latest = Arc::new(Mutex::new(None::<InstallPhase>));
        let reported = latest.clone();
        let mut work = exec::background(move || {
            job.run(&mut |phase| {
                *reported.lock().unwrap_or_else(PoisonError::into_inner) = Some(phase);
            })
        });
        let staged = loop {
            if let Some(result) = exec::timeout(PROGRESS_INTERVAL, &mut work).await {
                break result;
            }
            let phase = latest.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(phase) = phase {
                progress(describe(&phase));
            }
        };
        let staged = staged.map_err(|e| e.to_string())?;
        self.core(|p| p.extensions().commit(staged))
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn set_extension_enabled(
        self: &Rc<Self>,
        id: &ExtensionId,
        enabled: bool,
    ) -> Result<(), String> {
        self.core(|p| p.extensions().set_enabled(id, enabled))
            .map_err(|e| e.to_string())?;
        self.sync_extensions().await
    }

    /// Unloads the extension from the engine first, so core can delete its files.
    pub(crate) async fn uninstall_extension(
        self: &Rc<Self>,
        id: &ExtensionId,
    ) -> Result<(), String> {
        let engine_id = self
            .core(|p| p.extensions().get(id))
            .map_err(|e| e.to_string())?
            .and_then(|e| e.engine_id);
        if let (Some(engine_id), Some(profile)) = (engine_id, self.engine_profile().await) {
            let loaded = engine_call(ENGINE_CALL_TIMEOUT, engine::extensions(&profile)).await?;
            if let Some(loaded) = loaded.iter().find(|e| e.id == engine_id) {
                engine_call(ENGINE_CALL_TIMEOUT, loaded.remove()).await?;
            }
        }
        self.core(|p| p.extensions().uninstall(id))
            .map_err(|e| e.to_string())?;
        self.extensions.notify();
        self.sync_extensions().await
    }

    /// Brings WebView2's extensions in line with core's and waits until a sync that started
    /// after this call has finished. Syncs never overlap.
    pub(crate) async fn sync_extensions(self: &Rc<Self>) -> Result<(), String> {
        let host = &self.extensions;
        let generation = host.sync_requested.get() + 1;
        host.sync_requested.set(generation);
        if !host.sync_running.replace(true) {
            exec::spawn(self.clone().sync_loop());
        }
        exec::wait_for(SYNC_WAIT, Duration::from_millis(50), || {
            (host.sync_done.get() >= generation).then_some(())
        })
        .await
        .ok_or("the extension sync did not finish")?;
        match host.last_sync_error.borrow().clone() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    async fn sync_loop(self: Rc<Self>) {
        loop {
            let target = self.extensions.sync_requested.get();
            if self.extensions.sync_done.get() >= target {
                break;
            }
            let result = self.sync_once().await;
            if let Err(e) = &result {
                log::warn!("extension sync: {e}");
            }
            *self.extensions.last_sync_error.borrow_mut() = result.err();
            self.extensions.sync_done.set(target);
            self.refresh_extension_actions();
            self.extensions.notify();
        }
        self.extensions.sync_running.set(false);
    }

    /// Idempotent. An extension is (re)added when WebView2 does not have the id core recorded
    /// for its current folder; add comes before remove, because adding a folder with an id the
    /// engine knows replaces that extension in place.
    async fn sync_once(&self) -> Result<(), String> {
        let profile = self
            .engine_profile()
            .await
            .ok_or("no web view to reach the engine through")?;
        let listed = engine_call(ENGINE_CALL_TIMEOUT, engine::extensions(&profile)).await?;
        let wanted = self
            .core(|p| p.extensions().list())
            .map_err(|e| e.to_string())?;
        log::debug!(
            "extension sync: engine has {:?}; core wants {:?}",
            listed
                .iter()
                .map(|e| (&e.id, e.enabled))
                .collect::<Vec<_>>(),
            wanted
                .iter()
                .map(|e| (e.id.as_str(), &e.engine_id, e.enabled))
                .collect::<Vec<_>>()
        );
        let mut errors = HashMap::new();
        for ext in &wanted {
            let loaded = ext
                .engine_id
                .as_deref()
                .is_some_and(|id| listed.iter().any(|e| e.id == id));
            if loaded {
                continue;
            }
            match engine_call(
                ENGINE_ADD_TIMEOUT,
                engine::add_extension(&profile, &ext.dir),
            )
            .await
            {
                Ok(added) => {
                    log::info!(
                        "engine loaded {} as {} from {}",
                        ext.id.as_str(),
                        added.id,
                        ext.dir.display()
                    );
                    if let Err(e) = self.core(|p| p.extensions().set_engine_id(&ext.id, &added.id))
                    {
                        log::warn!("recording the engine id of {}: {e}", ext.id.as_str());
                    }
                }
                Err(e) => {
                    log::warn!(
                        "engine refused {} ({}): {e}",
                        ext.id.as_str(),
                        ext.dir.display()
                    );
                    errors.insert(ext.id.clone(), engine_refusal(e, &ext.dir));
                }
            }
        }

        let listed = engine_call(ENGINE_CALL_TIMEOUT, engine::extensions(&profile)).await?;
        let wanted = self
            .core(|p| p.extensions().list())
            .map_err(|e| e.to_string())?;
        for loaded in listed.iter().filter(|e| !e.is_builtin()) {
            let want = wanted
                .iter()
                .find(|w| w.engine_id.as_deref() == Some(loaded.id.as_str()));
            let result = match want {
                None => {
                    log::info!("removing {} ({}) from the engine", loaded.name, loaded.id);
                    engine_call(ENGINE_CALL_TIMEOUT, loaded.remove()).await
                }
                Some(w) if w.enabled != loaded.enabled => {
                    engine_call(ENGINE_CALL_TIMEOUT, loaded.set_enabled(w.enabled)).await
                }
                Some(_) => Ok(()),
            };
            if let Err(e) = result {
                log::warn!("engine extension {}: {e}", loaded.id);
            }
        }
        *self.extensions.engine_errors.borrow_mut() = errors;
        Ok(())
    }

    /// Rebuilds the toolbar's action buttons from core's list.
    pub(crate) fn refresh_extension_actions(&self) {
        let actions: Vec<ExtensionAction> = match self.core(|p| p.extensions().list()) {
            Ok(list) => list
                .iter()
                .filter_map(ExtensionAction::from_installed)
                .collect(),
            Err(e) => {
                log::warn!("listing extensions: {e}");
                return;
            }
        };
        if *self.extensions.actions.borrow() == actions {
            return;
        }
        *self.extensions.actions.borrow_mut() = actions.clone();
        for window in self.windows() {
            window.set_extension_actions(&actions);
        }
    }
}

/// Beyond this many characters in an extension's folder path, WebView2 cannot load large
/// extensions (it failed at 217 characters for uBlock Origin Lite and loaded at 215), even
/// through a short or extended-length path, because it resolves the folder's real path.
const DEEP_FOLDER_CHARS: usize = 200;

/// The engine's refusal, with the likely cause when the folder is deep.
fn engine_refusal(error: String, dir: &std::path::Path) -> String {
    let chars = dir.as_os_str().len();
    if chars > DEEP_FOLDER_CHARS {
        format!(
            "{error} (its folder path is {chars} characters long; WebView2 cannot load larger \
             extensions from folders this deep, so use a profile folder with a shorter path)"
        )
    } else {
        error
    }
}

async fn engine_call<T>(
    limit: Duration,
    call: impl Future<Output = windows_core::Result<T>>,
) -> Result<T, String> {
    match exec::timeout(limit, call).await {
        Some(result) => result.map_err(|e| e.message()),
        None => Err(format!(
            "WebView2 did not answer within {} s",
            limit.as_secs()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_progress_has_a_fraction_when_the_size_is_known() {
        let p = describe(&InstallPhase::Downloading {
            received: 5 << 20,
            total: Some(10 << 20),
        });
        assert_eq!(p.text, "Downloading: 5.0 of 10.0 MB");
        assert_eq!(p.fraction, Some(0.5));
        let p = describe(&InstallPhase::Downloading {
            received: 1 << 20,
            total: None,
        });
        assert_eq!((p.text.as_str(), p.fraction), ("Downloading: 1.0 MB", None));
        assert_eq!(describe(&InstallPhase::Verifying).fraction, None);
    }

    #[test]
    fn deep_folders_explain_the_refusal() {
        let shallow = std::path::Path::new(r"C:\p\ext");
        assert_eq!(
            engine_refusal("Unspecified error".into(), shallow),
            "Unspecified error"
        );
        let deep = format!(r"C:\{}", "d".repeat(220));
        let text = engine_refusal("Unspecified error".into(), std::path::Path::new(&deep));
        assert!(text.starts_with("Unspecified error (its folder path is 223 characters"));
    }

    #[test]
    fn verification_labels() {
        assert_eq!(
            verification_label(&Verification::ChromeWebStore {
                publisher_verified: true
            }),
            "Chrome Web Store, signed by the store"
        );
        assert_eq!(
            verification_label(&Verification::Unpacked),
            "Unpacked folder"
        );
    }
}
