//! # vsesvit-update
//!
//! Vsesvit's self-updater: the client side of the `tauri-plugin-updater` 2.12 protocol, so
//! the server that serves Tauri apps serves Vsesvit too. The contract is
//! `docs/design/packaging.md`.
//!
//! Everything here blocks. Each step is a `Send` value the shell runs on a worker thread
//! (docs/PLAN.md, "Threading model"):
//!
//! ```ignore
//! let installation = Installation::detect();
//! let updater = Updater::new(Config::builtin()?, current_version, installation.clone())?;
//! let available = updater.check("stable")?;
//! remove_stale_downloads(&cache_dir, available.as_ref().map(|a| &a.release().version))?;
//! if let Some(Available::Update(update)) = available {
//!     let downloaded = update.download(&cache_dir, |received, total| { /* progress */ })?;
//!     match downloaded.install(&installation, &relaunch_args) {
//!         Ok(Installed::ExitNow) => { /* save the session, then exit */ }
//!         Ok(Installed::Relaunch) => { /* offer "Restart" */ }
//!         Ok(Installed::NextLaunch) => { /* nothing to do */ }
//!         Err(failed) => { /* keep failed.downloaded to try again */ }
//!     }
//! }
//! ```
//!
//! | module          | owns                                                                  |
//! |-----------------|-----------------------------------------------------------------------|
//! | [`config`]      | `packaging/updater.json`, build-time overrides                        |
//! | [`installation`]| how this copy was installed (the `package-format` marker), artifact magic |
//! | `release`       | pure protocol logic: URL templating, response parsing, key lookup, signatures |
//! | `updater`       | the network: `check` and `download`                                   |
//! | `install`       | applying a verified artifact, per format                              |

pub mod config;
pub mod installation;
mod install;
mod release;
mod updater;

pub use config::{Config, WindowsInstallMode};
pub use install::{InstallFailed, Installed};
pub use installation::{Format, Installation};
pub use updater::{Available, Downloaded, Release, Update, Updater, remove_stale_downloads};

/// The `{{target}}` of the protocol, and the first part of a static-format platform key.
pub const TARGET: &str = if cfg!(windows) { "windows" } else { "linux" };

/// The `{{arch}}` of the protocol, spelled as Tauri spells it.
pub const ARCH: &str = if cfg!(target_arch = "x86") {
    "i686"
} else if cfg!(target_arch = "x86_64") {
    "x86_64"
} else if cfg!(target_arch = "arm") {
    "armv7"
} else if cfg!(target_arch = "aarch64") {
    "aarch64"
} else if cfg!(target_arch = "riscv64") {
    "riscv64"
} else {
    std::env::consts::ARCH
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the updater is off: {0}")]
    Disabled(DisabledReason),
    #[error("network: {0}")]
    Network(String),
    #[error("update server returned HTTP {0}")]
    Http(u16),
    #[error("unexpected update server response: {0}")]
    BadResponse(String),
    #[error("the release has no artifact for this platform (looked for {})", .0.join(", "))]
    NoArtifactForTarget(Vec<String>),
    #[error("the update's signature is not valid: {0}")]
    Signature(String),
    #[error("the update is signed for version {signed} but was announced as {announced}")]
    SignedVersionMismatch { signed: String, announced: semver::Version },
    #[error("the update is larger than {} MiB", .0 / (1024 * 1024))]
    TooLarge(u64),
    #[error("the downloaded file is not a {0:?} package")]
    WrongArtifactType(Format),
    #[error("installing the update failed: {0}")]
    Install(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DisabledReason {
    #[error("no public key is configured")]
    NoPublicKey,
    #[error("the configured public key is not a minisign public key: {0}")]
    InvalidPublicKey(String),
    #[error("no update endpoints are configured")]
    NoEndpoints,
    #[error("invalid updater configuration: {0}")]
    InvalidConfig(String),
    /// Unpackaged builds and Flatpak installs can check for updates but not apply them.
    #[error("this installation does not update itself")]
    NotSelfUpdating,
}
