//! Self-updates (docs/design/packaging.md): one state machine for the whole app, the schedule
//! that drives it, and the `--check-for-updates` / `--update` commands.
//!
//! Updates stay out of the way. Scheduled checks and downloads show nothing; the first thing
//! the user sees is "Vsesvit <v> is ready" in every window. Progress and failures are shown
//! only for something the user asked for (restarting, trying again). Checks and downloads
//! block, so they run on worker threads (`exec::background`) and their results are applied
//! on the UI thread.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use semver::Version;
use vsesvit_core::prefs::UpdateChannel;
use vsesvit_update::{
    Available, Config, Downloaded, Installation, Update, Updater, cli, remove_stale_downloads,
};

use crate::browser::{self, Browser};
use crate::cli::UpdateCommand;
use crate::sync::live;
use crate::{app, exec};

const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trigger {
    Scheduled,
    User,
}

/// `D` is the verified download; tests use a stand-in, since only the updater makes one.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum State<D> {
    /// Not checked since launch.
    Idle,
    Checking {
        trigger: Trigger,
    },
    Downloading {
        trigger: Trigger,
        version: Version,
        received: u64,
        total: Option<u64>,
    },
    /// `error` is why the last "Restart to update" did not start the installer.
    Ready {
        version: Version,
        update: D,
        error: Option<String>,
    },
    Installing {
        version: Version,
    },
    UpToDate,
    /// Leads back to `Checking` on the next check.
    Failed {
        trigger: Trigger,
        error: String,
    },
    /// This copy never updates itself.
    Disabled(String),
}

#[derive(Debug)]
pub(crate) enum Event<D> {
    Check(Trigger),
    UpToDate,
    Found(Version),
    Progress {
        received: u64,
        total: Option<u64>,
    },
    Downloaded(D),
    Failed(String),
    /// The installer did not start; the download comes back for the next try.
    InstallFailed {
        update: D,
        error: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Banner {
    pub severity: Severity,
    pub title: String,
    pub message: String,
    pub action: Option<Action>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Severity {
    Informational,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Restart,
    Retry,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Restart => "Restart to update",
            Self::Retry => "Try again",
        }
    }
}

/// What the Updates group in Settings shows. Unlike the update bar, it shows every state,
/// scheduled checks included, since the user went looking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Status {
    /// `None` when this copy never updates itself; Settings says why instead.
    pub text: Option<String>,
    pub button: StatusButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusButton {
    /// "Check for updates", greyed out while a check cannot start.
    Check { enabled: bool },
    /// The update bar's button, doing what it does there.
    Banner(Action),
}

impl StatusButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::Check { .. } => "Check for updates",
            Self::Banner(action) => action.label(),
        }
    }

    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Check { enabled: false })
    }
}

impl<D> State<D> {
    fn can_check(&self) -> bool {
        matches!(self, Self::Idle | Self::UpToDate | Self::Failed { .. })
    }

    /// Events that do not fit the current state are stale results and change nothing.
    pub fn next(self, event: Event<D>) -> Self {
        match (self, event) {
            (Self::Idle | Self::UpToDate | Self::Failed { .. }, Event::Check(trigger)) => {
                Self::Checking { trigger }
            }
            (Self::Checking { .. }, Event::UpToDate) => Self::UpToDate,
            (Self::Checking { trigger }, Event::Found(version)) => Self::Downloading {
                trigger,
                version,
                received: 0,
                total: None,
            },
            (
                Self::Downloading {
                    trigger, version, ..
                },
                Event::Progress { received, total },
            ) => Self::Downloading {
                trigger,
                version,
                received,
                total,
            },
            (Self::Downloading { version, .. }, Event::Downloaded(update)) => Self::Ready {
                version,
                update,
                error: None,
            },
            (
                Self::Checking { trigger } | Self::Downloading { trigger, .. },
                Event::Failed(error),
            ) => Self::Failed { trigger, error },
            (Self::Installing { version }, Event::InstallFailed { update, error }) => Self::Ready {
                version,
                update,
                error: Some(error),
            },
            (state, _) => state,
        }
    }

    /// Another channel was picked: what the old one found is dropped, a ready update included
    /// (the next check deletes its file). An install already under way goes on.
    fn switch_channel(self) -> Self {
        match self {
            Self::Checking { .. }
            | Self::Downloading { .. }
            | Self::Ready { .. }
            | Self::UpToDate
            | Self::Failed { .. } => Self::Idle,
            state @ (Self::Idle | Self::Installing { .. } | Self::Disabled(_)) => state,
        }
    }

    /// `Ready` becomes `Installing` and hands over the download, once.
    pub fn begin_install(&mut self) -> Option<D> {
        let Self::Ready { version, .. } = self else {
            return None;
        };
        let installing = Self::Installing {
            version: version.clone(),
        };
        match std::mem::replace(self, installing) {
            Self::Ready { update, .. } => Some(update),
            _ => None,
        }
    }

    /// What every window's update bar shows, if anything.
    pub fn banner(&self) -> Option<Banner> {
        let info = |title: String, message: String, action| Banner {
            severity: Severity::Informational,
            title,
            message,
            action,
        };
        match self {
            Self::Ready {
                version,
                error: None,
                ..
            } => Some(info(
                format!("Vsesvit {version} is ready"),
                String::new(),
                Some(Action::Restart),
            )),
            Self::Ready {
                error: Some(error), ..
            } => Some(Banner {
                severity: Severity::Error,
                title: "Vsesvit could not update".into(),
                message: error.clone(),
                action: Some(Action::Restart),
            }),
            Self::Installing { version } => Some(info(
                format!("Vsesvit {version} is ready"),
                "Restarting…".into(),
                None,
            )),
            Self::Checking {
                trigger: Trigger::User,
            } => Some(info("Checking for updates".into(), String::new(), None)),
            Self::Downloading {
                trigger: Trigger::User,
                version,
                received,
                total,
            } => Some(info(
                format!("Downloading Vsesvit {version}"),
                percent(*received, *total).map_or_else(String::new, |p| format!("{p}%")),
                None,
            )),
            Self::Failed {
                trigger: Trigger::User,
                error,
            } => Some(Banner {
                severity: Severity::Error,
                title: "Vsesvit could not update".into(),
                message: error.clone(),
                action: Some(Action::Retry),
            }),
            _ => None,
        }
    }

    pub fn status(&self) -> Status {
        let text = match self {
            Self::Idle => Some(format!("Vsesvit {}", env!("CARGO_PKG_VERSION"))),
            Self::Checking { .. } => Some("Checking for updates…".into()),
            Self::UpToDate => Some("Vsesvit is up to date".into()),
            Self::Downloading {
                version,
                received,
                total,
                ..
            } => Some(match percent(*received, *total) {
                Some(p) => format!("Downloading Vsesvit {version}… {p}%"),
                None => format!("Downloading Vsesvit {version}…"),
            }),
            Self::Ready {
                version,
                error: None,
                ..
            } => Some(format!("Vsesvit {version} is ready")),
            Self::Ready {
                error: Some(error), ..
            } => Some(format!("Vsesvit could not update: {error}")),
            Self::Installing { .. } => Some("Restarting…".into()),
            Self::Failed { error, .. } => Some(format!("Could not check for updates: {error}")),
            Self::Disabled(_) => None,
        };
        let button = match self {
            Self::Ready { .. } => StatusButton::Banner(Action::Restart),
            _ => StatusButton::Check {
                enabled: self.can_check(),
            },
        };
        Status { text, button }
    }
}

/// How much of a download has arrived, once its size is known.
fn percent(received: u64, total: Option<u64>) -> Option<u64> {
    total.filter(|t| *t > 0).map(|t| received * 100 / t)
}

/// The app's updater: the state machine plus what it needs to act. `setup` is `None` exactly
/// when the state is `Disabled`.
pub(crate) struct Updates {
    state: RefCell<State<Downloaded>>,
    setup: Option<Setup>,
    /// Called after every change of `state`, while their owners (an open Settings) keep them.
    listeners: RefCell<Vec<Weak<dyn Fn()>>>,
    /// Counts channel switches. A check acts on its results only while this is the count it
    /// started with, so a check or download for the old channel cannot land after a switch, nor
    /// clean away the new channel's download.
    channel_switches: Cell<u64>,
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
                    state: RefCell::new(State::Idle),
                    setup: Some(setup),
                    listeners: RefCell::default(),
                    channel_switches: Cell::default(),
                }
            }
            Err(reason) => Self::disabled(reason),
        }
    }

    pub fn disabled(reason: String) -> Self {
        log::info!("updates: off, {reason}");
        Self {
            state: RefCell::new(State::Disabled(reason)),
            setup: None,
            listeners: RefCell::default(),
            channel_switches: Cell::default(),
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.setup.is_none()
    }

    pub fn banner(&self) -> Option<Banner> {
        self.state.borrow().banner()
    }

    pub fn status(&self) -> Status {
        self.state.borrow().status()
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

    /// Whether the channel is still the one a check that saw `switches` channel switches began on.
    fn on_channel(&self, switches: u64) -> bool {
        self.channel_switches.get() == switches
    }

    fn begin_install(&self) -> Option<(Installation, Downloaded)> {
        let setup = self.setup.as_ref()?;
        let update = self.state.borrow_mut().begin_install()?;
        Some((setup.installation.clone(), update))
    }
}

fn setup() -> Result<Setup, String> {
    let installation = Installation::detect();
    if !matches!(installation, Installation::Nsis) {
        return Err(format!(
            "this copy is {}, not an NSIS installation",
            installation.variant().unwrap_or("unpackaged")
        ));
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
    browser
        .updates()
        .state
        .replace_with(|state| std::mem::replace(state, State::Idle).next(event));
    browser.update_state_changed();
}

/// Forgets the old channel's check, download or ready update, then checks the new channel.
pub(crate) fn switch_channel(browser: &Browser) {
    let updates = browser.updates();
    updates
        .channel_switches
        .set(updates.channel_switches.get() + 1);
    updates
        .state
        .replace_with(|state| std::mem::replace(state, State::Idle).switch_channel());
    browser.update_state_changed();
    browser.check_for_updates();
}

/// Checks `FIRST_CHECK` after the first window shows, then every `CHECK_INTERVAL` while
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
        let Some(failed) = browser
            .upgrade()
            .map(|b| matches!(*b.updates().state.borrow(), State::Failed { .. }))
        else {
            return;
        };
        wait = if failed {
            RETRY_INTERVAL
        } else {
            CHECK_INTERVAL
        };
    }
}

/// Checks, and downloads a newer version into the updates folder, replacing what is there.
pub(crate) async fn check(browser: Weak<Browser>, trigger: Trigger) {
    let Some((updater, dir, channel, switches)) = browser.upgrade().and_then(|b| {
        let updates = b.updates();
        let setup = updates.setup.as_ref()?;
        if !updates.state.borrow().can_check() {
            return None;
        }
        let jobs = (
            setup.updater.clone(),
            setup.dir.clone(),
            b.updates_channel(),
            updates.channel_switches.get(),
        );
        apply(&b, Event::Check(trigger));
        Some(jobs)
    }) else {
        return;
    };

    let checked = exec::background(move || updater.check(channel.name()))
        .await
        .unwrap_or_else(|lost| Err(std::io::Error::from(lost).into()));
    // The new channel's check shares the updates folder, so a check for a channel the user has
    // left neither cleans it nor downloads into it.
    if !browser
        .upgrade()
        .is_some_and(|b| b.updates().on_channel(switches))
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
            with(&browser, switches, |b| apply(b, Event::UpToDate));
            return;
        }
        Ok(None) => {
            log::info!("updates: Vsesvit {} is current", env!("CARGO_PKG_VERSION"));
            if let Err(e) = exec::background(move || remove_stale(&dir, None)).await {
                log::debug!("updates: cleaning: {e}");
            }
            with(&browser, switches, |b| apply(b, Event::UpToDate));
            return;
        }
        Err(e) => {
            log::warn!("updates: check failed: {e}");
            with(&browser, switches, |b| {
                apply(b, Event::Failed(e.to_string()))
            });
            return;
        }
    };
    let version = update.release.version.clone();
    log::info!("updates: downloading Vsesvit {version}");
    with(&browser, switches, |b| apply(b, Event::Found(version)));

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
                        && b.updates().on_channel(switches)
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
            with(&browser, switches, |b| apply(b, Event::Downloaded(update)));
        }
        Err(e) => {
            log::warn!("updates: download failed: {e}");
            with(&browser, switches, |b| {
                apply(b, Event::Failed(e.to_string()))
            });
        }
    }
}

/// Runs `f` unless the browser is gone or the channel has switched since the check began.
fn with(browser: &Weak<Browser>, switches: u64, f: impl FnOnce(&Browser)) {
    if let Some(browser) = browser.upgrade()
        && browser.updates().on_channel(switches)
    {
        f(&browser);
    }
}

/// "Restart to update": the installer takes over from here and relaunches the browser. The
/// session is saved on the way out (`app::exit`), before the installer sees the process end.
pub(crate) fn restart(browser: &Browser) {
    let Some((installation, update)) = browser.updates().begin_install() else {
        return;
    };
    browser.update_state_changed();
    let version = update.version().clone();
    match update.install(&installation, &[]) {
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
    if browser.updates().is_disabled() || !browser.updates_automatic() {
        return;
    }
    if let Some(install) = browser.updates().begin_install() {
        INSTALL_ON_EXIT.set(Some(install));
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

    type S = State<&'static str>;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn run(state: S, events: impl IntoIterator<Item = Event<&'static str>>) -> S {
        events.into_iter().fold(state, S::next)
    }

    #[test]
    fn a_scheduled_update_is_silent_until_ready() {
        let mut state = S::Idle.next(Event::Check(Trigger::Scheduled));
        assert_eq!(state.banner(), None);
        state = state.next(Event::Found(v("0.2.0")));
        assert_eq!(state.banner(), None);
        state = state.next(Event::Progress {
            received: 5,
            total: Some(10),
        });
        assert_eq!(
            state,
            S::Downloading {
                trigger: Trigger::Scheduled,
                version: v("0.2.0"),
                received: 5,
                total: Some(10),
            }
        );
        assert_eq!(state.banner(), None);
        state = state.next(Event::Downloaded("setup.exe"));
        assert_eq!(
            state,
            S::Ready {
                version: v("0.2.0"),
                update: "setup.exe",
                error: None,
            }
        );
        let banner = state.banner().unwrap();
        assert_eq!(banner.title, "Vsesvit 0.2.0 is ready");
        assert_eq!(banner.severity, Severity::Informational);
        assert_eq!(banner.action, Some(Action::Restart));
    }

    #[test]
    fn up_to_date_is_silent_and_checks_again() {
        let state = run(S::Idle, [Event::Check(Trigger::User), Event::UpToDate]);
        assert_eq!(state, S::UpToDate);
        assert_eq!(state.banner(), None);
        assert_eq!(
            state.next(Event::Check(Trigger::Scheduled)),
            S::Checking {
                trigger: Trigger::Scheduled
            }
        );
    }

    #[test]
    fn settings_shows_every_state() {
        let check = StatusButton::Check { enabled: true };
        let busy = StatusButton::Check { enabled: false };
        let restart = StatusButton::Banner(Action::Restart);
        let downloading = |total| S::Downloading {
            trigger: Trigger::Scheduled,
            version: v("0.2.0"),
            received: 1,
            total,
        };
        let ready = |error: Option<&str>| S::Ready {
            version: v("0.2.0"),
            update: "setup.exe",
            error: error.map(Into::into),
        };
        let current = format!("Vsesvit {}", env!("CARGO_PKG_VERSION"));
        for (state, text, button) in [
            (S::Idle, Some(current.as_str()), check),
            (
                S::Checking {
                    trigger: Trigger::Scheduled,
                },
                Some("Checking for updates…"),
                busy,
            ),
            (S::UpToDate, Some("Vsesvit is up to date"), check),
            (downloading(None), Some("Downloading Vsesvit 0.2.0…"), busy),
            (
                downloading(Some(4)),
                Some("Downloading Vsesvit 0.2.0… 25%"),
                busy,
            ),
            (ready(None), Some("Vsesvit 0.2.0 is ready"), restart),
            (
                ready(Some("the installer did not start")),
                Some("Vsesvit could not update: the installer did not start"),
                restart,
            ),
            (
                S::Installing {
                    version: v("0.2.0"),
                },
                Some("Restarting…"),
                busy,
            ),
            (
                S::Failed {
                    trigger: Trigger::Scheduled,
                    error: "network: offline".into(),
                },
                Some("Could not check for updates: network: offline"),
                check,
            ),
            (S::Disabled("unpackaged".into()), None, busy),
        ] {
            let status = state.status();
            assert_eq!(status.text.as_deref(), text, "{state:?}");
            assert_eq!(status.button, button, "{state:?}");
            assert_eq!(status.button.is_enabled(), button != busy, "{state:?}");
        }
    }

    #[test]
    fn the_settings_button_is_the_bars_restart_when_an_update_is_ready() {
        let ready = S::Ready {
            version: v("0.2.0"),
            update: "setup.exe",
            error: None,
        };
        let button = ready.status().button;
        assert_eq!(
            button,
            StatusButton::Banner(ready.banner().unwrap().action.unwrap())
        );
        assert_eq!(button.label(), "Restart to update");
        assert_eq!(S::Idle.status().button.label(), "Check for updates");
    }

    #[test]
    fn a_scheduled_failure_is_logged_not_shown_and_retried() {
        let state = run(
            S::Idle,
            [
                Event::Check(Trigger::Scheduled),
                Event::Found(v("0.2.0")),
                Event::Failed("network: offline".into()),
            ],
        );
        assert_eq!(
            state,
            S::Failed {
                trigger: Trigger::Scheduled,
                error: "network: offline".into()
            }
        );
        assert_eq!(state.banner(), None);
        assert_eq!(
            state.next(Event::Check(Trigger::Scheduled)),
            S::Checking {
                trigger: Trigger::Scheduled
            }
        );
    }

    #[test]
    fn a_user_retry_shows_progress_and_failure() {
        let failed = S::Failed {
            trigger: Trigger::User,
            error: "boom".into(),
        };
        let banner = failed.banner().unwrap();
        assert_eq!(banner.severity, Severity::Error);
        assert_eq!(banner.message, "boom");
        assert_eq!(banner.action, Some(Action::Retry));

        let downloading = run(
            failed,
            [
                Event::Check(Trigger::User),
                Event::Found(v("0.2.0")),
                Event::Progress {
                    received: 1,
                    total: Some(4),
                },
            ],
        );
        let banner = downloading.banner().unwrap();
        assert_eq!(banner.title, "Downloading Vsesvit 0.2.0");
        assert_eq!(banner.message, "25%");
        assert_eq!(banner.action, None);
    }

    #[test]
    fn install_hands_over_the_download_once() {
        let mut state = S::Ready {
            version: v("0.2.0"),
            update: "setup.exe",
            error: None,
        };
        assert_eq!(state.begin_install(), Some("setup.exe"));
        assert_eq!(
            state,
            S::Installing {
                version: v("0.2.0")
            }
        );
        assert_eq!(state.begin_install(), None);
        assert_eq!(state.banner().unwrap().action, None);
        assert_eq!(S::Idle.begin_install(), None);
    }

    #[test]
    fn a_failed_install_keeps_the_download_for_the_retry() {
        let mut state = S::Installing {
            version: v("0.2.0"),
        }
        .next(Event::InstallFailed {
            update: "setup.exe",
            error: "the installer did not start".into(),
        });
        let banner = state.banner().unwrap();
        assert_eq!(banner.severity, Severity::Error);
        assert_eq!(banner.message, "the installer did not start");
        assert_eq!(banner.action, Some(Action::Restart));
        assert!(!state.can_check(), "the retry does not check or download");
        assert_eq!(state.begin_install(), Some("setup.exe"));
    }

    #[test]
    fn stale_events_change_nothing() {
        assert_eq!(
            S::Idle.next(Event::Progress {
                received: 1,
                total: None
            }),
            S::Idle
        );
        assert_eq!(S::Idle.next(Event::Downloaded("x")), S::Idle);
        assert_eq!(S::UpToDate.next(Event::UpToDate), S::UpToDate);
        let checking = S::Checking {
            trigger: Trigger::Scheduled,
        };
        assert_eq!(
            run(checking, [Event::Check(Trigger::User)]),
            S::Checking {
                trigger: Trigger::Scheduled
            }
        );
        let ready = || S::Ready {
            version: v("0.2.0"),
            update: "x",
            error: None,
        };
        assert!(!ready().can_check());
        assert_eq!(ready().next(Event::Failed("late".into())), ready());
        let install_failed = Event::InstallFailed {
            update: "y",
            error: "late".into(),
        };
        assert_eq!(ready().next(install_failed), ready());
    }

    #[test]
    fn switching_channel_drops_what_the_old_one_found() {
        let found = [
            S::Checking {
                trigger: Trigger::Scheduled,
            },
            S::Downloading {
                trigger: Trigger::User,
                version: v("0.2.0"),
                received: 1,
                total: None,
            },
            S::Ready {
                version: v("0.2.0"),
                update: "setup.exe",
                error: None,
            },
            S::Ready {
                version: v("0.2.0"),
                update: "setup.exe",
                error: Some("the installer did not start".into()),
            },
            S::UpToDate,
            S::Failed {
                trigger: Trigger::User,
                error: "boom".into(),
            },
        ];
        for state in found {
            let switched = state.switch_channel();
            assert_eq!(switched, S::Idle);
            assert!(switched.can_check(), "the new channel is checked at once");
        }
        let mut switched = S::Ready {
            version: v("0.2.0"),
            update: "setup.exe",
            error: None,
        }
        .switch_channel();
        assert_eq!(switched.begin_install(), None, "quitting installs nothing");
    }

    #[test]
    fn switching_channel_leaves_a_started_install_alone() {
        let installing = || S::Installing {
            version: v("0.2.0"),
        };
        assert_eq!(installing().switch_channel(), installing());
        let disabled = || S::Disabled("unpackaged".into());
        assert_eq!(disabled().switch_channel(), disabled());
    }

    #[test]
    fn disabled_is_terminal() {
        let disabled = || S::Disabled("unpackaged".into());
        for event in [
            Event::Check(Trigger::User),
            Event::UpToDate,
            Event::Found(v("9.0.0")),
            Event::Failed("x".into()),
        ] {
            assert_eq!(disabled().next(event), disabled());
        }
        assert_eq!(disabled().banner(), None);
        assert!(!disabled().status().button.is_enabled());
    }

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

    #[test]
    fn a_check_begun_before_a_channel_switch_is_off_channel() {
        let updates = Updates::disabled("unpackaged".into());
        let began = updates.channel_switches.get();
        assert!(updates.on_channel(began));
        updates.channel_switches.set(began + 1);
        assert!(!updates.on_channel(began));
    }
}
