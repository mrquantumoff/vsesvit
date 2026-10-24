//! Self-updates (docs/design/packaging.md): one state machine for the whole app, the schedule
//! that drives it, and the `--check-for-updates` / `--update` commands.
//!
//! Updates stay out of the way. Scheduled checks and downloads show nothing; the first thing
//! the user sees is "Vsesvit <v> is ready" in every window. Progress and failures are shown
//! only for something the user asked for (restarting, trying again). Checks and downloads
//! block, so they run on worker threads (`exec::background`) and their results are applied
//! on the UI thread.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use semver::Version;
use vsesvit_core::cli::UpdateCommand;
use vsesvit_core::prefs::UpdateChannel;
use vsesvit_update::lifecycle::{
    Banner, Effect, Epoch, Event, FIRST_CHECK, Flavour, Lifecycle, Status, Trigger, percent,
};
use vsesvit_update::{
    Available, Config, Downloaded, Installation, Update, Updater, cli, remove_stale_downloads,
};

use crate::browser::{self, Browser};
use crate::sync::live;
use crate::{app, exec};

/// The app's updater: the state machine plus what it needs to act. `setup` is `None` exactly
/// when the state is `Disabled`.
pub(crate) struct Updates {
    lifecycle: RefCell<Lifecycle<Downloaded>>,
    setup: Option<Setup>,
    /// Called after every change of the lifecycle, while their owners (an open Settings) keep them.
    listeners: RefCell<Vec<Weak<dyn Fn()>>>,
}

struct Setup {
    installation: Installation,
    updater: Arc<Updater>,
    dir: PathBuf,
}

impl Updates {
    /// Updates run only in an NSIS installation with a usable built-in configuration.
    pub fn detect() -> Self {
        match setup() {
            Ok(setup) => {
                log::info!("updates: automatic, downloads in {}", setup.dir.display());
                Self {
                    lifecycle: RefCell::new(Lifecycle::new(current_version())),
                    setup: Some(setup),
                    listeners: RefCell::default(),
                }
            }
            Err(reason) => Self::disabled(reason),
        }
    }

    pub fn disabled(reason: String) -> Self {
        log::info!("updates: off, {reason}");
        Self {
            lifecycle: RefCell::new(Lifecycle::disabled(current_version(), reason)),
            setup: None,
            listeners: RefCell::default(),
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.setup.is_none()
    }

    pub fn banner(&self) -> Option<Banner> {
        self.lifecycle.borrow().banner()
    }

    pub fn next_check_in(&self) -> std::time::Duration {
        self.lifecycle.borrow().next_check_in()
    }

    pub fn status(&self) -> Status {
        self.lifecycle.borrow().status()
    }

    /// Calls `listener` after every change of the update state, while the caller keeps it.
    pub fn on_change(&self, listener: &Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(Rc::downgrade(listener));
    }

    /// Collected first, so a listener can read the state or add a listener.
    pub fn changed(&self) {
        for listener in live(&self.listeners) {
            listener();
        }
    }

    fn step(&self, event: Event<Downloaded>) -> Option<Effect<Downloaded>> {
        self.lifecycle.borrow_mut().step(event)
    }

    /// Whether the channel is still the one a check that began at `epoch` asked.
    fn on_channel(&self, epoch: Epoch) -> bool {
        self.lifecycle.borrow().is_current(epoch)
    }
}

fn setup() -> Result<Setup, String> {
    let installation = Installation::detect();
    if let Some(reason) = Flavour::WinUi.refusal(&installation) {
        return Err(reason);
    }
    let dir = download_dir()?;
    let updater = Config::builtin()
        .and_then(|config| Updater::new(config, current_version(), installation.clone()))
        .map_err(|e| e.to_string())?;
    Ok(Setup {
        installation,
        updater: Arc::new(updater),
        dir,
    })
}

fn download_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("Vsesvit").join("updates"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_owned())
}

fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package versions are semver")
}

fn apply(browser: &Browser, event: Event<Downloaded>) {
    browser.updates().step(event);
    browser.update_state_changed();
}

/// What a check needs from the app, taken at the moment it begins.
struct Job {
    updater: Arc<Updater>,
    dir: PathBuf,
    channel: UpdateChannel,
    /// The channel epoch the check began in: its results land only while this is current.
    epoch: Epoch,
}

/// Steps `event`; the job when that starts a check.
fn begin(browser: &Browser, event: Event<Downloaded>) -> Option<Job> {
    let updates = browser.updates();
    let Some(Effect::Check { epoch }) = updates.step(event) else {
        return None;
    };
    let setup = updates.setup.as_ref()?;
    Some(Job {
        updater: setup.updater.clone(),
        dir: setup.dir.clone(),
        channel: browser.updates_channel(),
        epoch,
    })
}

/// Forgets the old channel's check, download or ready update, then checks the new channel.
pub(crate) fn switch_channel(browser: Weak<Browser>) {
    let Some(strong) = browser.upgrade() else {
        return;
    };
    let job = begin(&strong, Event::ChannelChanged);
    strong.update_state_changed();
    if let Some(job) = job {
        exec::spawn(run(browser, job));
    }
}

/// Checks `FIRST_CHECK` after the first window shows, then as often as the lifecycle says while
/// `updates.automatic` is on.
pub(crate) async fn schedule(browser: Weak<Browser>) {
    let mut wait = FIRST_CHECK;
    loop {
        exec::sleep(wait).await;
        let Some(automatic) = browser.upgrade().map(|b| b.updates_automatic()) else {
            return;
        };
        if automatic {
            check(browser.clone(), Trigger::Scheduled).await;
        }
        let Some(next) = browser.upgrade().map(|b| b.updates().next_check_in()) else {
            return;
        };
        wait = next;
    }
}

/// Checks, and downloads a newer version into the updates folder, replacing what is there.
pub(crate) async fn check(browser: Weak<Browser>, trigger: Trigger) {
    let Some(job) = browser.upgrade().and_then(|b| {
        let job = begin(&b, Event::Check(trigger))?;
        b.update_state_changed();
        Some(job)
    }) else {
        return;
    };
    run(browser, job).await;
}

async fn run(
    browser: Weak<Browser>,
    Job {
        updater,
        dir,
        channel,
        epoch,
    }: Job,
) {
    let checked = exec::background(move || updater.check(channel.name()))
        .await
        .unwrap_or_else(|lost| Err(std::io::Error::from(lost).into()));
    // The new channel's check shares the updates folder, so a check for a channel the user has
    // left neither cleans it nor downloads into it.
    if !browser
        .upgrade()
        .is_some_and(|b| b.updates().on_channel(epoch))
    {
        return;
    }
    let update = match checked {
        Ok(Some(Available::Update(update))) => update,
        Ok(Some(Available::NotInstallable(release))) => {
            // `setup` accepts only an NSIS installation, which always has an artifact to install.
            log::warn!(
                "updates: Vsesvit {} is out, but this installation does not update itself",
                release.version
            );
            with(&browser, epoch, |b| apply(b, Event::UpToDate));
            return;
        }
        Ok(None) => {
            log::info!("updates: Vsesvit {} is current", env!("CARGO_PKG_VERSION"));
            if let Err(e) = exec::background(move || remove_stale(&dir, None)).await {
                log::debug!("updates: cleaning: {e}");
            }
            with(&browser, epoch, |b| apply(b, Event::UpToDate));
            return;
        }
        Err(e) => {
            log::warn!("updates: check failed: {e}");
            with(&browser, epoch, |b| apply(b, Event::Failed(e.to_string())));
            return;
        }
    };
    let version = update.release.version.clone();
    log::info!("updates: downloading Vsesvit {version}");
    with(&browser, epoch, |b| apply(b, Event::Found(version)));

    let queue = exec::dispatcher();
    let downloaded = exec::background(move || {
        let mut shown = None;
        download(&update, &dir, |received, total| {
            let percent = percent(received, total);
            if shown.replace(percent) == Some(percent) {
                return;
            }
            if let Some(queue) = &queue {
                exec::post(queue, move || {
                    if let Some(b) = browser::current()
                        && b.updates().on_channel(epoch)
                    {
                        apply(&b, Event::Progress { received, total });
                    }
                });
            }
        })
    })
    .await
    .unwrap_or_else(|lost| Err(std::io::Error::from(lost).into()));
    match downloaded {
        Ok(update) => {
            log::info!("updates: {} is ready", update.path().display());
            with(&browser, epoch, |b| apply(b, Event::Downloaded(update)));
        }
        Err(e) => {
            log::warn!("updates: download failed: {e}");
            with(&browser, epoch, |b| apply(b, Event::Failed(e.to_string())));
        }
    }
}

/// Runs `f` unless the browser is gone or the channel has switched since the check began.
fn with(browser: &Weak<Browser>, epoch: Epoch, f: impl FnOnce(&Browser)) {
    if let Some(browser) = browser.upgrade()
        && browser.updates().on_channel(epoch)
    {
        f(&browser);
    }
}

/// "Restart to update": the installer takes over from here and relaunches the browser. The
/// session is saved on the way out (`app::exit`), before the installer sees the process end.
pub(crate) fn restart(browser: &Browser) {
    let updates = browser.updates();
    let Some(setup) = &updates.setup else {
        return;
    };
    let Some(Effect::Install(update)) = updates.step(Event::Restart) else {
        return;
    };
    browser.update_state_changed();
    let version = update.version().clone();
    match update.install(&setup.installation, &[]) {
        Ok(outcome) => {
            log::info!("updates: installing Vsesvit {version} ({outcome:?}), exiting");
            app::exit(0);
        }
        Err(failed) => {
            log::error!("updates: installing Vsesvit {version}: {}", failed.error);
            apply(
                browser,
                Event::InstallFailed {
                    update: failed.downloaded,
                    error: failed.error.to_string(),
                },
            );
        }
    }
}

thread_local! {
    static INSTALL_ON_EXIT: RefCell<Option<(Installation, Downloaded)>> = const { RefCell::new(None) };
}

/// The user quit with an update ready: `install_on_exit` installs it silently, without
/// relaunching.
pub(crate) fn queue_install_on_exit(browser: &Browser) {
    let updates = browser.updates();
    let Some(setup) = &updates.setup else {
        return;
    };
    let automatic = browser.updates_automatic();
    if let Some(Effect::InstallOnExit(update)) = updates.step(Event::Quit { automatic }) {
        INSTALL_ON_EXIT.set(Some((setup.installation.clone(), update)));
    }
}

/// Runs the installer `queue_install_on_exit` queued. Called once `app::run` has returned: the
/// session is saved and XAML has shut down. A download that fails to launch stays on disk, and
/// the next check uses it.
pub(crate) fn install_on_exit() {
    let Some((installation, update)) = INSTALL_ON_EXIT.take() else {
        return;
    };
    let version = update.version().clone();
    match update.install_on_exit(&installation) {
        Ok(_) => log::info!("updates: installing Vsesvit {version} as the browser exits"),
        Err(failed) => {
            log::error!(
                "updates: installing Vsesvit {version} on exit: {}",
                failed.error
            );
        }
    }
}

/// Fetches `update` into `dir`. Other versions' installers and partial downloads go first; a
/// complete, verified download of this version is reused by the updater.
fn download(
    update: &Update,
    dir: &Path,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<Downloaded, vsesvit_update::Error> {
    std::fs::create_dir_all(dir)?;
    remove_stale(dir, Some(&update.release.version));
    update.download(dir, progress)
}

/// A file that is in use (an installer still running) stays until the next time.
fn remove_stale(dir: &Path, keep: Option<&Version>) {
    if let Err(e) = remove_stale_downloads(dir, keep) {
        log::debug!("updates: cleaning {}: {e}", dir.display());
    }
}

// ---- `--check-for-updates` and `--update` ----

/// Prints what `vsesvit_update::cli` documents. Opens no profile, so it follows the channel this
/// build was released on, not `updates.channel`.
pub(crate) fn run_command(command: UpdateCommand) -> ExitCode {
    let installation = Installation::detect();
    let channel = UpdateChannel::of_build().name();
    cli::print(match command {
        UpdateCommand::Check => {
            cli::check(&installation, &current_version(), channel).map_err(|e| e.to_string())
        }
        UpdateCommand::Install => download_dir().and_then(|dir| {
            cli::update(&installation, &current_version(), channel, &dir).map_err(|e| e.to_string())
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listeners_hear_changes_only_while_kept() {
        let updates = Updates::disabled("unpackaged".into());
        let heard = Rc::new(std::cell::Cell::new(0));
        let h = heard.clone();
        let listener: Rc<dyn Fn()> = Rc::new(move || h.set(h.get() + 1));
        updates.on_change(&listener);
        updates.changed();
        drop(listener);
        updates.changed();
        assert_eq!(heard.get(), 1);
        assert!(updates.listeners.borrow().is_empty());
    }
}
