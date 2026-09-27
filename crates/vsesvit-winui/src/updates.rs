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
use std::rc::Weak;
use std::sync::Arc;
use std::time::Duration;

use semver::Version;
use serde_json::{Value, json};
use vsesvit_update::{Config, Downloaded, Installation, Installed, Update, Updater};

use crate::browser::{self, Browser};
use crate::cli::UpdateCommand;
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
    Ready {
        version: Version,
        update: D,
    },
    Installing {
        version: Version,
    },
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
    Progress { received: u64, total: Option<u64> },
    Downloaded(D),
    Failed(String),
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

impl<D> State<D> {
    fn can_check(&self) -> bool {
        matches!(self, Self::Idle | Self::Failed { .. })
    }

    /// Events that do not fit the current state are stale results and change nothing.
    pub fn next(self, event: Event<D>) -> Self {
        match (self, event) {
            (Self::Idle | Self::Failed { .. }, Event::Check(trigger)) => Self::Checking { trigger },
            (Self::Checking { .. }, Event::UpToDate) => Self::Idle,
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
            (Self::Downloading { version, .. }, Event::Downloaded(update)) => {
                Self::Ready { version, update }
            }
            (
                Self::Checking { trigger } | Self::Downloading { trigger, .. },
                Event::Failed(error),
            ) => Self::Failed { trigger, error },
            (Self::Installing { .. }, Event::Failed(error)) => Self::Failed {
                trigger: Trigger::User,
                error,
            },
            (state, _) => state,
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
            Self::Ready { version, .. } => Some(info(
                format!("Vsesvit {version} is ready"),
                String::new(),
                Some(Action::Restart),
            )),
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
                match total {
                    Some(total) if *total > 0 => format!("{}%", received * 100 / total),
                    _ => String::new(),
                },
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
}

/// The app's updater: the state machine plus what it needs to act. `setup` is `None` exactly
/// when the state is `Disabled`.
pub(crate) struct Updates {
    state: RefCell<State<Downloaded>>,
    setup: Option<Setup>,
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
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.setup.is_none()
    }

    pub fn banner(&self) -> Option<Banner> {
        self.state.borrow().banner()
    }

    fn begin_install(&self) -> Option<(Installation, Downloaded)> {
        let setup = self.setup.as_ref()?;
        let update = self.state.borrow_mut().begin_install()?;
        Some((setup.installation.clone(), update))
    }
}

fn setup() -> Result<Setup, String> {
    let installation = Installation::detect();
    if !matches!(installation, Installation::Nsis { .. }) {
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
    let Some((updater, dir)) = browser.upgrade().and_then(|b| {
        let updates = b.updates();
        let setup = updates.setup.as_ref()?;
        if !updates.state.borrow().can_check() {
            return None;
        }
        let jobs = (setup.updater.clone(), setup.dir.clone());
        apply(&b, Event::Check(trigger));
        Some(jobs)
    }) else {
        return;
    };

    let checked = exec::background(move || updater.check()).await;
    let update = match checked {
        Ok(Some(update)) => update,
        Ok(None) => {
            log::info!("updates: Vsesvit {} is current", env!("CARGO_PKG_VERSION"));
            exec::background(move || remove_stale(&dir)).await;
            with(&browser, |b| apply(b, Event::UpToDate));
            return;
        }
        Err(e) => {
            log::warn!("updates: check failed: {e}");
            with(&browser, |b| apply(b, Event::Failed(e.to_string())));
            return;
        }
    };
    log::info!("updates: downloading Vsesvit {}", update.version);
    with(&browser, |b| apply(b, Event::Found(update.version.clone())));

    let queue = exec::dispatcher();
    let downloaded = exec::background(move || {
        let mut shown = None;
        download(&update, &dir, |received, total| {
            let percent = total.filter(|t| *t > 0).map(|t| received * 100 / t);
            if shown.replace(percent) == Some(percent) {
                return;
            }
            if let Some(queue) = &queue {
                exec::post(queue, move || {
                    if let Some(b) = browser::current() {
                        apply(&b, Event::Progress { received, total });
                    }
                });
            }
        })
    })
    .await;
    match downloaded {
        Ok(update) => {
            log::info!("updates: {} is ready", update.path().display());
            with(&browser, |b| apply(b, Event::Downloaded(update)));
        }
        Err(e) => {
            log::warn!("updates: download failed: {e}");
            with(&browser, |b| apply(b, Event::Failed(e.to_string())));
        }
    }
}

fn with(browser: &Weak<Browser>, f: impl FnOnce(&Browser)) {
    if let Some(browser) = browser.upgrade() {
        f(&browser);
    }
}

/// "Restart to update": the installer takes over from here and relaunches the browser.
pub(crate) fn restart(browser: &Browser) {
    let Some((installation, update)) = browser.updates().begin_install() else {
        return;
    };
    browser.update_state_changed();
    browser.save_session();
    let version = update.version().clone();
    match update.install(&installation, &[]) {
        Ok(outcome) => {
            log::info!("updates: installing Vsesvit {version} ({outcome:?}), exiting");
            app::exit(0);
        }
        Err(e) => {
            log::error!("updates: installing Vsesvit {version}: {e}");
            apply(browser, Event::Failed(e.to_string()));
        }
    }
}

/// The user quit with an update ready: install it silently, without relaunching. The session
/// is already saved.
pub(crate) fn install_on_exit(browser: &Browser) {
    if !browser.updates_automatic() {
        return;
    }
    let Some((installation, update)) = browser.updates().begin_install() else {
        return;
    };
    let version = update.version().clone();
    match update.install_on_exit(&installation) {
        Ok(_) => log::info!("updates: installing Vsesvit {version} as the browser exits"),
        Err(e) => log::error!("updates: installing Vsesvit {version} on exit: {e}"),
    }
}

fn download(
    update: &Update,
    dir: &Path,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<Downloaded, vsesvit_update::Error> {
    remove_stale(dir);
    std::fs::create_dir_all(dir)?;
    update.download(dir, progress)
}

/// Installers of older versions, and partial downloads. A file that is in use (an installer
/// still running) stays until the next time.
fn remove_stale(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && let Err(e) = std::fs::remove_file(&path)
        {
            log::debug!("updates: keeping {}: {e}", path.display());
        }
    }
}

// ---- `--check-for-updates` and `--update` ----

/// Prints one JSON line on stdout: 0 with the result, or 1 with `{"error": ...}`.
pub(crate) fn run_command(command: UpdateCommand) -> ExitCode {
    let installation = Installation::detect();
    let result = match command {
        UpdateCommand::Check => {
            check_now(&installation).map(|update| report(&installation, update.as_ref()))
        }
        UpdateCommand::Install => install_now(&installation),
    };
    match result {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", json!({ "error": e }));
            ExitCode::FAILURE
        }
    }
}

fn check_now(installation: &Installation) -> Result<Option<Update>, String> {
    Config::builtin()
        .and_then(|config| Updater::new(config, current_version(), installation.clone()))
        .and_then(|updater| updater.check())
        .map_err(|e| e.to_string())
}

fn install_now(installation: &Installation) -> Result<Value, String> {
    let update = check_now(installation)?;
    let mut value = report(installation, update.as_ref());
    let Some(update) = update else {
        value["installed"] = Value::Null;
        return Ok(value);
    };
    let dir = download_dir()?;
    let mut shown = None;
    let downloaded = download(&update, &dir, |received, total| {
        let step = total.map_or(received >> 20, |t| received * 10 / t.max(1));
        if shown.replace(step) != Some(step) {
            match total {
                Some(total) => eprintln!("downloading: {received} of {total} bytes"),
                None => eprintln!("downloading: {received} bytes"),
            }
        }
    })
    .map_err(|e| e.to_string())?;
    let installed = downloaded
        .install(installation, &[])
        .map_err(|e| e.to_string())?;
    value["installed"] = json!(match installed {
        Installed::ExitNow => "exit_now",
        Installed::Relaunch => "relaunch",
        Installed::NextLaunch => "next_launch",
    });
    Ok(value)
}

fn report(installation: &Installation, update: Option<&Update>) -> Value {
    let available = update.map(|update| {
        json!({
            "version": update.version.to_string(),
            "notes": update.notes,
            "pub_date": update
                .pub_date
                .and_then(|date| date.format(&time::format_description::well_known::Rfc3339).ok()),
        })
    });
    json!({
        "installation": installation.variant().unwrap_or("unpackaged"),
        "current": env!("CARGO_PKG_VERSION"),
        "available": available,
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
                update: "setup.exe"
            }
        );
        let banner = state.banner().unwrap();
        assert_eq!(banner.title, "Vsesvit 0.2.0 is ready");
        assert_eq!(banner.severity, Severity::Informational);
        assert_eq!(banner.action, Some(Action::Restart));
    }

    #[test]
    fn up_to_date_goes_back_to_idle() {
        let state = run(S::Idle, [Event::Check(Trigger::Scheduled), Event::UpToDate]);
        assert_eq!(state, S::Idle);
        assert!(state.can_check());
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
    fn a_failed_install_can_be_retried() {
        let state = S::Installing {
            version: v("0.2.0"),
        }
        .next(Event::Failed("the installer did not start".into()));
        assert_eq!(state.banner().unwrap().action, Some(Action::Retry));
        assert!(state.can_check());
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
        let checking = S::Checking {
            trigger: Trigger::Scheduled,
        };
        assert_eq!(
            run(checking, [Event::Check(Trigger::User)]),
            S::Checking {
                trigger: Trigger::Scheduled
            }
        );
        let ready = S::Ready {
            version: v("0.2.0"),
            update: "x",
        };
        assert!(!ready.can_check());
        assert_eq!(
            ready.next(Event::Failed("late".into())),
            S::Ready {
                version: v("0.2.0"),
                update: "x"
            }
        );
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
    }

    #[test]
    fn the_report_names_the_installation() {
        let value = report(&Installation::Unpackaged, None);
        assert_eq!(
            value,
            json!({
                "installation": "unpackaged",
                "current": env!("CARGO_PKG_VERSION"),
                "available": null,
            })
        );
    }
}
