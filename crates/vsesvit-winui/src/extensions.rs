//! Extensions on Windows. vsesvit-core owns which extensions are installed (and where their
//! immutable folders are); WebView2 runs them and persists its own list. This module installs
//! through core's pipeline (the download and verification run on a worker thread, the commit on
//! the UI thread) and brings WebView2's list in line with core's (`sync_extensions`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use vsesvit_core::extensions::{
    ExtensionId, InstallJob, InstallPhase, InstallSource, InstalledExtension,
};

use crate::bindings::CoreWebView2Profile;
use crate::browser::Browser;
use crate::engine::EngineExtension;
use crate::popup::ExtensionAction;
use crate::sync::live;
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
    reconcile_running: Cell<bool>,
    /// Asked for during a reconcile, which then runs once more.
    reconcile_again: Cell<bool>,
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
        for listener in live(&self.listeners) {
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

impl Browser {
    /// At startup: `--load-extension` folders, then what other devices asked for.
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
        self.reconcile_extensions();
    }

    /// Installs and removes what other devices asked for (at startup, and after a sync changed
    /// the synced extensions), then brings the engine in line. Reconciles run one at a time.
    pub(crate) fn reconcile_extensions(self: &Rc<Self>) {
        let host = &self.extensions;
        if host.reconcile_running.replace(true) {
            host.reconcile_again.set(true);
            return;
        }
        let browser = self.clone();
        exec::spawn(async move {
            loop {
                browser.extensions.reconcile_again.set(false);
                browser.reconcile_once().await;
                if !browser.extensions.reconcile_again.get() {
                    break;
                }
            }
            browser.extensions.reconcile_running.set(false);
        });
    }

    async fn reconcile_once(self: &Rc<Self>) {
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
            log::warn!("extension sync after reconcile: {e}");
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
                progress(Progress {
                    text: phase.describe(),
                    fraction: phase.fraction(),
                });
            }
        };
        let staged = staged
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
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

    /// One `sync_engine` pass against the WebView2 profile.
    async fn sync_once(&self) -> Result<(), String> {
        let profile = self
            .engine_profile()
            .await
            .ok_or("no web view to reach the engine through")?;
        let list = || {
            self.core(|p| p.extensions().list())
                .map_err(|e| e.to_string())
        };
        let record = |id: &ExtensionId, engine_id: &str| {
            if let Err(e) = self.core(|p| p.extensions().set_engine_id(id, engine_id)) {
                log::warn!("recording the engine id of {}: {e}", id.as_str());
            }
        };
        let errors = sync_engine(&ProfileEngine(profile), &list, &record).await?;
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
        *self.extensions.actions.borrow_mut() = actions;
        self.show_extension_actions();
    }
}

/// What the sync needs from the engine: the WebView2 profile, or a fake in the tests.
trait EngineExtensions {
    type Loaded: LoadedExtension;
    async fn list(&self) -> Result<Vec<Self::Loaded>, String>;
    async fn add(&self, dir: &Path) -> Result<Self::Loaded, String>;
}

/// An extension the engine has loaded.
trait LoadedExtension {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn enabled(&self) -> bool;
    async fn remove(&self) -> Result<(), String>;
    async fn set_enabled(&self, enabled: bool) -> Result<(), String>;
}

struct ProfileEngine(CoreWebView2Profile);

impl EngineExtensions for ProfileEngine {
    type Loaded = EngineExtension;

    async fn list(&self) -> Result<Vec<EngineExtension>, String> {
        engine_call(ENGINE_CALL_TIMEOUT, engine::extensions(&self.0)).await
    }

    async fn add(&self, dir: &Path) -> Result<EngineExtension, String> {
        engine_call(ENGINE_ADD_TIMEOUT, engine::add_extension(&self.0, dir)).await
    }
}

impl LoadedExtension for EngineExtension {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn remove(&self) -> Result<(), String> {
        engine_call(ENGINE_CALL_TIMEOUT, EngineExtension::remove(self)).await
    }

    async fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        engine_call(
            ENGINE_CALL_TIMEOUT,
            EngineExtension::set_enabled(self, enabled),
        )
        .await
    }
}

/// Brings the engine's extensions in line with core's `list`. Idempotent. An extension is
/// (re)added when the engine does not have the id core recorded for its current folder; add
/// comes before remove, because adding a folder with an id the engine knows replaces that
/// extension in place. Returns what went wrong per extension, for the Extensions dialog.
async fn sync_engine<E: EngineExtensions>(
    engine: &E,
    list: &dyn Fn() -> Result<Vec<InstalledExtension>, String>,
    record_engine_id: &dyn Fn(&ExtensionId, &str),
) -> Result<HashMap<ExtensionId, String>, String> {
    let listed = engine.list().await?;
    let wanted = list()?;
    log::debug!(
        "extension sync: engine has {:?}; core wants {:?}",
        listed
            .iter()
            .map(|e| (e.id(), e.enabled()))
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
            .is_some_and(|id| listed.iter().any(|e| e.id() == id));
        if loaded {
            continue;
        }
        if ext.engine_id.is_none()
            && let Some(key) = engine_key(ext)
            && let Err(e) = add_manifest_key(&ext.dir, &key)
        {
            log::warn!("{} gets no stable engine id: {e}", ext.id.as_str());
        }
        match engine.add(&ext.dir).await {
            Ok(added) => {
                log::info!(
                    "engine loaded {} as {} from {}",
                    ext.id.as_str(),
                    added.id(),
                    ext.dir.display()
                );
                record_engine_id(&ext.id, added.id());
                // The engine starts every extension it adds; one that is off stops at once.
                if !ext.enabled
                    && added.enabled()
                    && let Err(e) = added.set_enabled(false).await
                {
                    log::warn!("switching {} off: {e}", ext.id.as_str());
                    errors.insert(ext.id.clone(), format!("could not be switched off: {e}"));
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

    let listed = engine.list().await?;
    let wanted = list()?;
    for loaded in &listed {
        let want = wanted
            .iter()
            .find(|w| w.engine_id.as_deref() == Some(loaded.id()));
        match want {
            Some(w) if w.enabled != loaded.enabled() => {
                if let Err(e) = loaded.set_enabled(w.enabled).await {
                    log::warn!("engine extension {}: {e}", loaded.id());
                    let state = if w.enabled { "on" } else { "off" };
                    errors
                        .entry(w.id.clone())
                        .or_insert_with(|| format!("could not be switched {state}: {e}"));
                }
            }
            Some(_) => {}
            None if engine::is_builtin(loaded.id()) => {}
            None => {
                log::info!(
                    "removing {} ({}) from the engine",
                    loaded.name(),
                    loaded.id()
                );
                if let Err(e) = loaded.remove().await {
                    log::warn!("engine extension {}: {e}", loaded.id());
                }
            }
        }
    }
    Ok(errors)
}

/// What the `key` of a keyless managed install is made of. Chromium derives an unpacked
/// extension's id from its `key`, or from its folder's path when it has none, and every version
/// of a managed install gets its own folder; a key made from core's id keeps the engine id, and
/// with it the extension's storage, across versions.
const ENGINE_KEY_PREFIX: &str = "vsesvit-extension:";

/// The `key` to write into a managed install without one before the engine first loads it.
/// Unpacked folders are the developer's and are never written to.
fn engine_key(ext: &InstalledExtension) -> Option<String> {
    let managed = !matches!(ext.source, InstallSource::Unpacked { .. });
    (managed && ext.manifest.key.is_none())
        .then(|| STANDARD.encode(format!("{ENGINE_KEY_PREFIX}{}", ext.id.as_str())))
}

/// Adds `"key": key` as the first member of `dir`'s manifest.json. Inserted as text, so the
/// comments Chromium accepts there survive. Does nothing if the manifest already has this key.
fn add_manifest_key(dir: &Path, key: &str) -> std::io::Result<()> {
    let path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&path)?;
    if text.contains(&format!("\"{key}\"")) {
        return Ok(());
    }
    let open = object_start(&text).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manifest.json does not hold a JSON object",
        )
    })?;
    let updated = format!(
        "{}\n  \"key\": \"{key}\",{}",
        &text[..=open],
        &text[open + 1..]
    );
    let temp = dir.join("manifest.json.vsesvit-new");
    std::fs::write(&temp, updated)?;
    std::fs::rename(&temp, &path)
}

/// Where the top-level object's `{` is, past a byte order mark, whitespace and comments.
fn object_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = if text.starts_with('\u{feff}') { 3 } else { 0 };
    while let Some(&b) = bytes.get(i) {
        match (b, bytes.get(i + 1)) {
            (b' ' | b'\t' | b'\r' | b'\n', _) => i += 1,
            (b'/', Some(b'/')) => i = text[i..].find('\n').map_or(bytes.len(), |n| i + n),
            (b'/', Some(b'*')) => i = i + 2 + text[i + 2..].find("*/")? + 2,
            (b'{', _) => return Some(i),
            _ => return None,
        }
    }
    None
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
    use std::path::PathBuf;

    use vsesvit_core::extensions::Verification;
    use vsesvit_core::extensions::manifest::Manifest;

    use super::*;

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
    fn the_key_goes_first_and_the_rest_of_the_manifest_is_kept() {
        let dir = temp_dir("key-first");
        let original = "\u{feff}// made by hand\n/* {not this} */ {\n  \"name\": \"X\", // why\n  \"version\": \"1\"\n}\n";
        std::fs::write(dir.join("manifest.json"), original).unwrap();
        add_manifest_key(&dir, "a2V5").unwrap();
        let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        assert_eq!(
            text,
            "\u{feff}// made by hand\n/* {not this} */ {\n  \"key\": \"a2V5\",\n  \"name\": \"X\", // why\n  \"version\": \"1\"\n}\n"
        );
        add_manifest_key(&dir, "a2V5").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("manifest.json")).unwrap(),
            text
        );
        std::fs::write(dir.join("manifest.json"), "[]").unwrap();
        assert!(add_manifest_key(&dir, "a2V5").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_engine_key_is_padded_base64_of_the_id() {
        let xpi = installed("x@vsesvit.test", r"C:\p\x\1.0_ab", None, true);
        assert_eq!(
            engine_key(&xpi).as_deref(),
            Some("dnNlc3ZpdC1leHRlbnNpb246eEB2c2Vzdml0LnRlc3Q="),
            "existing installs keep their engine id"
        );
    }

    #[test]
    fn only_keyless_managed_installs_get_a_key() {
        let xpi = installed("x@vsesvit.test", r"C:\p\x\1.0_ab", None, true);
        let key = engine_key(&xpi).expect("a keyless XPI install gets a key");
        assert_eq!(
            engine_key(&xpi),
            Some(key.clone()),
            "the key depends only on the id"
        );
        assert_ne!(
            engine_key(&installed("y@vsesvit.test", r"C:\p\x\1.0_ab", None, true)),
            Some(key)
        );
        let mut unpacked = xpi.clone();
        unpacked.source = InstallSource::Unpacked {
            dir: unpacked.dir.clone(),
        };
        assert_eq!(
            engine_key(&unpacked),
            None,
            "a developer's folder is not written to"
        );
        let mut keyed = xpi;
        keyed.manifest.key = Some("a2V5".into());
        assert_eq!(engine_key(&keyed), None);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vsesvit-winui-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn installed(
        id: &str,
        dir: &str,
        engine_id: Option<&str>,
        enabled: bool,
    ) -> InstalledExtension {
        let manifest = Manifest::parse(
            r#"{"manifest_version": 3, "name": "X", "version": "1.0"}"#,
            &|_| None,
        )
        .unwrap();
        InstalledExtension {
            id: ExtensionId::parse(id).unwrap(),
            version: "1.0".into(),
            dir: PathBuf::from(dir),
            manifest,
            enabled,
            source: InstallSource::XpiFile {
                path: PathBuf::from(r"C:\x.xpi"),
            },
            verification: Verification::LocalXpi,
            engine_id: engine_id.map(Into::into),
        }
    }

    /// Engine state shared by a fake engine and the extensions it hands out.
    #[derive(Default)]
    struct EngineState {
        /// Id, name and whether it is enabled, in load order.
        loaded: Vec<(String, String, bool)>,
        /// Every call that changed something, in order.
        calls: Vec<String>,
        /// Ids whose `set_enabled` fails.
        broken: Vec<String>,
    }

    #[derive(Clone, Default)]
    struct FakeEngine(Rc<RefCell<EngineState>>);

    struct FakeLoaded {
        id: String,
        name: String,
        enabled: bool,
        state: Rc<RefCell<EngineState>>,
    }

    impl FakeEngine {
        fn with(loaded: &[(&str, &str, bool)]) -> Self {
            let engine = Self::default();
            engine.0.borrow_mut().loaded = loaded
                .iter()
                .map(|(id, name, on)| ((*id).into(), (*name).into(), *on))
                .collect();
            engine
        }

        fn calls(&self) -> Vec<String> {
            self.0.borrow().calls.clone()
        }

        fn handle(&self, (id, name, enabled): &(String, String, bool)) -> FakeLoaded {
            FakeLoaded {
                id: id.clone(),
                name: name.clone(),
                enabled: *enabled,
                state: self.0.clone(),
            }
        }
    }

    impl EngineExtensions for FakeEngine {
        type Loaded = FakeLoaded;

        async fn list(&self) -> Result<Vec<FakeLoaded>, String> {
            let loaded = self.0.borrow().loaded.clone();
            Ok(loaded.iter().map(|e| self.handle(e)).collect())
        }

        /// Chromium's rule: the id comes from the manifest's `key`, else from the folder path.
        async fn add(&self, dir: &Path) -> Result<FakeLoaded, String> {
            let id = Manifest::load(dir, "en")
                .ok()
                .and_then(|m| m.key_id())
                .unwrap_or_else(|| ExtensionId::for_unpacked_dir(dir));
            let entry = (id.as_str().to_owned(), "Added".to_owned(), true);
            let mut state = self.0.borrow_mut();
            state.calls.push(format!("add {}", dir.display()));
            state.loaded.retain(|(id, _, _)| *id != entry.0);
            state.loaded.push(entry.clone());
            drop(state);
            Ok(self.handle(&entry))
        }
    }

    impl LoadedExtension for FakeLoaded {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn enabled(&self) -> bool {
            self.enabled
        }

        async fn remove(&self) -> Result<(), String> {
            let mut state = self.state.borrow_mut();
            state.calls.push(format!("remove {}", self.id));
            state.loaded.retain(|(id, _, _)| *id != self.id);
            Ok(())
        }

        async fn set_enabled(&self, enabled: bool) -> Result<(), String> {
            let mut state = self.state.borrow_mut();
            state.calls.push(format!("enable {} {enabled}", self.id));
            if state.broken.contains(&self.id) {
                return Err("EnableAsync failed".into());
            }
            for entry in state.loaded.iter_mut().filter(|(id, _, _)| *id == self.id) {
                entry.2 = enabled;
            }
            Ok(())
        }
    }

    /// One sync against `engine`, with `core` as core's list; engine ids are recorded into it.
    fn sync(
        engine: &FakeEngine,
        core: &RefCell<Vec<InstalledExtension>>,
    ) -> HashMap<ExtensionId, String> {
        let list = || Ok(core.borrow().clone());
        let record = |id: &ExtensionId, engine_id: &str| {
            for ext in core.borrow_mut().iter_mut().filter(|e| e.id == *id) {
                ext.engine_id = Some(engine_id.to_owned());
            }
        };
        let mut future = std::pin::pin!(sync_engine(engine, &list, &record));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(result) => result.unwrap(),
            std::task::Poll::Pending => panic!("the fake engine answers at once"),
        }
    }

    const PDF_VIEWER: &str = "mhjfbmdgcfjbbpaeojofohoefgiehjai";
    const CLIPBOARD: &str = "dgiklkfkllikcanfonkcabmbdfmgleag";

    #[test]
    fn the_engines_own_extensions_are_left_alone() {
        let engine = FakeEngine::with(&[
            (CLIPBOARD, "Microsoft Clipboard Extension", true),
            (PDF_VIEWER, "Microsoft Edge PDF Viewer", true),
        ]);
        sync(&engine, &RefCell::new(Vec::new()));
        assert_eq!(engine.calls(), Vec::<String>::new());
    }

    #[test]
    fn an_extension_core_recorded_follows_core_whatever_its_name() {
        let impostor = "aaaabbbbccccddddeeeeffffgggghhhh";
        let engine = FakeEngine::with(&[(impostor, "Microsoft Edge PDF Viewer", true)]);
        let core = RefCell::new(vec![installed(
            "x@vsesvit.test",
            r"C:\p\x",
            Some(impostor),
            false,
        )]);
        sync(&engine, &core);
        assert_eq!(engine.calls(), [format!("enable {impostor} false")]);
    }

    #[test]
    fn an_unrecorded_extension_is_removed_whatever_its_name() {
        let leftover = "aaaabbbbccccddddeeeeffffgggghhhh";
        let engine = FakeEngine::with(&[
            (leftover, "Microsoft Clipboard Extension", true),
            (CLIPBOARD, "Microsoft Clipboard Extension", true),
        ]);
        sync(&engine, &RefCell::new(Vec::new()));
        assert_eq!(engine.calls(), [format!("remove {leftover}")]);
    }

    #[test]
    fn a_disabled_extension_is_switched_off_right_after_it_is_added() {
        let engine = FakeEngine::default();
        let (off, on) = (r"C:\p\off", r"C:\p\on");
        let core = RefCell::new(vec![
            installed("off@vsesvit.test", off, None, false),
            installed("on@vsesvit.test", on, None, true),
        ]);
        sync(&engine, &core);
        let off_id = ExtensionId::for_unpacked_dir(Path::new(off));
        assert_eq!(
            engine.calls(),
            [
                format!("add {off}"),
                format!("enable {} false", off_id.as_str()),
                format!("add {on}"),
            ]
        );
    }

    #[test]
    fn a_failed_switch_is_reported_for_the_dialog() {
        let loaded = "aaaabbbbccccddddeeeeffffgggghhhh";
        let engine = FakeEngine::with(&[(loaded, "X", true)]);
        engine.0.borrow_mut().broken.push(loaded.into());
        let core = RefCell::new(vec![installed(
            "x@vsesvit.test",
            r"C:\p\x",
            Some(loaded),
            false,
        )]);
        let errors = sync(&engine, &core);
        let id = ExtensionId::parse("x@vsesvit.test").unwrap();
        assert!(
            errors
                .get(&id)
                .is_some_and(|e| e.contains("EnableAsync failed")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_new_version_of_a_keyless_install_keeps_its_engine_id() {
        let root = temp_dir("versions");
        let mut ids = Vec::new();
        for version in ["1.0", "1.1"] {
            let dir = root.join(format!("{version}_ab"));
            std::fs::create_dir_all(&dir).unwrap();
            let manifest =
                format!(r#"{{"manifest_version": 3, "name": "X", "version": "{version}"}}"#);
            std::fs::write(dir.join("manifest.json"), manifest).unwrap();
            let engine = FakeEngine::default();
            let core = RefCell::new(vec![installed(
                "x@vsesvit.test",
                &dir.to_string_lossy(),
                None,
                true,
            )]);
            sync(&engine, &core);
            ids.push(core.borrow()[0].engine_id.clone().unwrap());
        }
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(ids[0], ids[1]);
    }
}
