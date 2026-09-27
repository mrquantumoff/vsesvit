//! Extension installs and the runtime's view of the installed set.
//!
//! An install is core's three-step pipeline: `prepare_install` on the UI thread,
//! `InstallJob::run` on a worker thread (`gio::spawn_blocking`) with its progress relayed
//! back over a channel, and `commit` on the UI thread again, after which the runtime
//! loads the extension.

use std::fmt;
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use gtk::{gio, glib};
use vsesvit_core::extensions::manifest::Manifest;
use vsesvit_core::extensions::{
    ExtensionId, InstallError, InstallJob, InstallPhase, InstallSource, InstalledExtension,
    Verification,
};
use vsesvit_webext::Unsupported;

use crate::browser::Browser;

/// How often the worker's progress is polled while an install runs.
const PROGRESS_POLL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub(crate) enum InstallFailure {
    Prepare(vsesvit_core::Error),
    Run(InstallError),
    Commit(vsesvit_core::Error),
    WorkerPanicked,
}

impl fmt::Display for InstallFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallFailure::Prepare(e) | InstallFailure::Commit(e) => write!(f, "{e}"),
            InstallFailure::Run(e) => write!(f, "{e}"),
            InstallFailure::WorkerPanicked => write!(f, "the install thread panicked"),
        }
    }
}

impl std::error::Error for InstallFailure {}

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

    /// Runs a prepared job off the UI thread, commits it, and hands the result to the
    /// runtime.
    pub(crate) async fn run_install_job(
        &self,
        job: InstallJob,
        progress: impl Fn(InstallPhase) + 'static,
    ) -> Result<Option<InstalledExtension>, InstallFailure> {
        let (tx, rx) = mpsc::channel::<InstallPhase>();
        let handle = gio::spawn_blocking(move || {
            job.run(&mut |phase| {
                let _ = tx.send(phase);
            })
        });
        // The worker's sender drops when the job ends, which ends this pump.
        let pump = glib::spawn_future_local(async move {
            loop {
                match rx.try_recv() {
                    Ok(phase) => progress(phase),
                    Err(TryRecvError::Empty) => glib::timeout_future(PROGRESS_POLL).await,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        });
        let staged = handle.await.map_err(|_| InstallFailure::WorkerPanicked)?;
        let _ = pump.await;
        let staged = staged.map_err(InstallFailure::Run)?;
        let committed = self.core().borrow_mut().extensions().commit(staged);
        let committed = committed.map_err(InstallFailure::Commit)?;
        if let Some(ext) = &committed {
            self.load_into_runtime(ext);
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
            self.runtime().unload(id);
        }
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
    ) -> Result<(), vsesvit_core::Error> {
        self.core().borrow_mut().extensions().set_enabled(id, enabled)?;
        let ext = self.core().borrow_mut().extensions().get(id)?;
        match ext {
            Some(ext) if ext.enabled => self.load_into_runtime(&ext),
            _ => self.runtime().unload(id),
        }
        Ok(())
    }

    pub(crate) fn uninstall_extension(&self, id: &ExtensionId) -> Result<(), vsesvit_core::Error> {
        self.runtime().unload(id);
        self.core().borrow_mut().extensions().uninstall(id)
    }

    fn load_into_runtime(&self, ext: &InstalledExtension) {
        if !ext.enabled {
            self.runtime().unload(&ext.id);
            return;
        }
        if let Err(e) = self.runtime().load(ext) {
            log::warn!("extension {}: {e}", ext.id.as_str());
        }
    }
}

/// One line for the install progress row.
pub(crate) fn describe_phase(phase: &InstallPhase) -> String {
    match phase {
        InstallPhase::Resolving => "Looking up the add-on…".to_owned(),
        InstallPhase::Downloading { received, total: Some(total) } if *total > 0 => {
            format!("Downloading… {} of {}", megabytes(*received), megabytes(*total))
        }
        InstallPhase::Downloading { received, .. } => format!("Downloading… {}", megabytes(*received)),
        InstallPhase::Verifying => "Verifying the signature…".to_owned(),
        InstallPhase::Unpacking => "Unpacking…".to_owned(),
        InstallPhase::ReadingManifest => "Reading the manifest…".to_owned(),
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// How the extensions page describes an install's provenance.
pub(crate) fn describe_verification(verification: &Verification) -> &'static str {
    match verification {
        Verification::ChromeWebStore { publisher_verified: true } => "Chrome Web Store, publisher verified",
        Verification::ChromeWebStore { publisher_verified: false } => "Chrome Web Store, developer key only",
        Verification::AmoHash => "Firefox Add-ons, hash checked",
        Verification::LocalCrx => "Local CRX, signature verified",
        Verification::LocalXpi => "Local XPI, not verified",
        Verification::Unpacked => "Unpacked folder, not verified",
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
    let f = Rc::new(f);
    move |phase| f(describe_phase(&phase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_read_as_progress() {
        assert_eq!(
            describe_phase(&InstallPhase::Downloading { received: 1_250_000, total: Some(9_763_168) }),
            "Downloading… 1.2 MB of 9.8 MB"
        );
        assert_eq!(
            describe_phase(&InstallPhase::Downloading { received: 500_000, total: None }),
            "Downloading… 0.5 MB"
        );
        assert_eq!(describe_phase(&InstallPhase::Verifying), "Verifying the signature…");
    }

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
}
