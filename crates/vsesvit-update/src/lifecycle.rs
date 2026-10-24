//! The update lifecycle every shell shares: what a check, a download and an install do to the
//! update state, what the update banner and Settings show for it, when the next check is due and
//! what quitting does with a ready update (docs/design/linux-cef/design.md section 4.3, row 13).
//!
//! Nothing here starts a thread, reads a clock or touches a toolkit. A shell owns a
//! [`Lifecycle`], feeds it [`Event`]s from its timer, its buttons and its worker threads, and does
//! what the [`Effect`] a step returns says. The download type `D` is [`crate::Downloaded`] in a
//! shell and a stand-in in tests, since only the updater makes one.
//!
//! The installs are the shell's too: [`Effect::Install`] and [`Effect::InstallOnExit`] hand over
//! the verified download, once, and [`Event::InstallFailed`] gives it back. A shell that installs
//! while it exits (the GPUI shell's restart, design section 3.6 step 10) never sends
//! `InstallFailed`, and the update stays on disk for the next check.

use std::time::Duration;

use semver::Version;

use crate::Installation;

/// The first check waits this long after the first window shows.
pub const FIRST_CHECK: Duration = Duration::from_secs(30);
/// Between checks, while nothing failed.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// After a check or download failed, the next one comes sooner.
pub const RETRY_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Which shell this is, so that it only ever updates an installation of its own flavour: the
/// installation's variant is what the update server is asked for, and a plain `nsis` variant is
/// the WinUI artifact (design section 6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    WinUi,
    Gpui,
}

impl Flavour {
    /// Why this shell must not update `installation`, `None` when it may. The `Err` text is what
    /// Settings and the log say.
    pub fn refusal(self, installation: &Installation) -> Option<String> {
        let variant = installation.variant().unwrap_or("unpackaged");
        match self {
            Self::WinUi if matches!(installation, Installation::Nsis) => None,
            Self::WinUi => Some(format!("this copy is {variant}, not an NSIS installation")),
            // Self-updating at all is the updater's own rule; this adds the flavour's.
            Self::Gpui if !installation.self_updates() => Some(format!(
                "this copy is {variant}, which does not update itself"
            )),
            Self::Gpui if matches!(installation, Installation::Nsis) => Some(
                "this copy is a WinUI installation, which the GPUI build does not update".into(),
            ),
            Self::Gpui => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Scheduled,
    User,
}

/// `D` is the verified download.
#[derive(Debug, PartialEq, Eq)]
pub enum State<D> {
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
pub enum Event<D> {
    Check(Trigger),
    /// Another channel was picked.
    ChannelChanged,
    /// "Restart to update".
    Restart,
    /// The last window closed (or the browser is quitting). `automatic` is the preference: an
    /// update installs on the way out only when the user left automatic updates on.
    Quit {
        automatic: bool,
    },
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

/// What the shell does after a step.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect<D> {
    /// Check and download on a worker thread, and apply its results only while the
    /// [`Lifecycle::is_current`] answers yes for `epoch`.
    Check { epoch: Epoch },
    /// Install, then do what [`crate::Installed`] says. If the installer does not start, send
    /// [`Event::InstallFailed`].
    Install(D),
    /// The browser is quitting with an update ready: run `Downloaded::install_on_exit` once the
    /// session is saved and the shell has shut down. Windows only, since the Linux packages need
    /// a password and a relaunch, which [`Effect::Install`] covers.
    InstallOnExit(D),
}

/// Counts channel changes. A check or download for a channel the user has left must not act on
/// its results, nor clean away the new channel's download.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Epoch(u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub severity: Severity,
    pub title: String,
    pub message: String,
    pub action: Option<Action>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Informational,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
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
pub struct Status {
    /// `None` when this copy never updates itself; Settings says why instead.
    pub text: Option<String>,
    pub button: StatusButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusButton {
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

/// The update state and the channel epoch, behind the one function that changes them.
#[derive(Debug)]
pub struct Lifecycle<D> {
    state: State<D>,
    epoch: Epoch,
    /// The running version, which Settings shows while nothing else is going on.
    current: Version,
}

impl<D> Lifecycle<D> {
    pub fn new(current: Version) -> Self {
        Self {
            state: State::Idle,
            epoch: Epoch::default(),
            current,
        }
    }

    /// For a copy that never updates itself; `reason` is logged and shown in Settings.
    pub fn disabled(current: Version, reason: String) -> Self {
        Self {
            state: State::Disabled(reason),
            ..Self::new(current)
        }
    }

    pub fn state(&self) -> &State<D> {
        &self.state
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self.state, State::Disabled(_))
    }

    /// Whether the channel is still the one a check that began at `epoch` asked.
    pub fn is_current(&self, epoch: Epoch) -> bool {
        self.epoch == epoch
    }

    /// Events that do not fit the current state are stale results and change nothing.
    pub fn step(&mut self, event: Event<D>) -> Option<Effect<D>> {
        match event {
            Event::Check(trigger) => self.check(trigger),
            Event::ChannelChanged => {
                self.epoch.0 += 1;
                self.state = self.take().switch_channel();
                self.check(Trigger::User)
            }
            Event::Restart => self.state.begin_install().map(Effect::Install),
            Event::Quit { automatic: false } => None,
            Event::Quit { automatic: true } => {
                self.state.begin_install().map(Effect::InstallOnExit)
            }
            result => {
                self.state = self.take().next(result);
                None
            }
        }
    }

    fn check(&mut self, trigger: Trigger) -> Option<Effect<D>> {
        if !self.state.can_check() {
            return None;
        }
        self.state = self.take().next(Event::Check(trigger));
        Some(Effect::Check { epoch: self.epoch })
    }

    fn take(&mut self) -> State<D> {
        std::mem::replace(&mut self.state, State::Idle)
    }

    /// How long until the next scheduled check: sooner after a failure.
    pub fn next_check_in(&self) -> Duration {
        match self.state {
            State::Failed { .. } => RETRY_INTERVAL,
            _ => CHECK_INTERVAL,
        }
    }

    /// What every window's update bar shows, if anything.
    pub fn banner(&self) -> Option<Banner> {
        self.state.banner()
    }

    pub fn status(&self) -> Status {
        self.state.status(&self.current)
    }
}

impl<D> State<D> {
    fn can_check(&self) -> bool {
        matches!(self, Self::Idle | Self::UpToDate | Self::Failed { .. })
    }

    /// The result events: the ones a worker or the installer sends. Events that do not fit the
    /// current state are stale and change nothing.
    fn next(self, event: Event<D>) -> Self {
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
    fn begin_install(&mut self) -> Option<D> {
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

    fn banner(&self) -> Option<Banner> {
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

    fn status(&self, current: &Version) -> Status {
        let text = match self {
            Self::Idle => Some(format!("Vsesvit {current}")),
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
pub fn percent(received: u64, total: Option<u64>) -> Option<u64> {
    total.filter(|t| *t > 0).map(|t| received * 100 / t)
}

#[cfg(test)]
mod tests {
    use super::*;

    type S = State<&'static str>;

    fn running() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap()
    }

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
            let status = state.status(&running());
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
        let button = ready.status(&running()).button;
        assert_eq!(
            button,
            StatusButton::Banner(ready.banner().unwrap().action.unwrap())
        );
        assert_eq!(button.label(), "Restart to update");
        assert_eq!(
            S::Idle.status(&running()).button.label(),
            "Check for updates"
        );
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
        assert!(!disabled().status(&running()).button.is_enabled());
    }

    type Machine = Lifecycle<&'static str>;

    fn fresh() -> Machine {
        Machine::new(v("1.0.0"))
    }

    fn ready() -> Machine {
        let mut machine = fresh();
        machine.step(Event::Check(Trigger::Scheduled));
        machine.step(Event::Found(v("2.0.0")));
        machine.step(Event::Downloaded("setup.exe"));
        assert!(matches!(machine.state(), State::Ready { .. }));
        machine
    }

    #[test]
    fn a_check_starts_one_worker_and_only_when_it_can() {
        let mut machine = fresh();
        assert_eq!(
            machine.step(Event::Check(Trigger::User)),
            Some(Effect::Check {
                epoch: Epoch::default()
            })
        );
        assert_eq!(
            machine.state(),
            &S::Checking {
                trigger: Trigger::User
            }
        );
        assert_eq!(machine.step(Event::Check(Trigger::User)), None);
        assert_eq!(machine.step(Event::UpToDate), None);
        assert_eq!(
            machine.status().text.as_deref(),
            Some("Vsesvit is up to date")
        );
        assert!(machine.step(Event::Check(Trigger::Scheduled)).is_some());

        let mut waiting = ready();
        assert_eq!(
            waiting.step(Event::Check(Trigger::User)),
            None,
            "no checks while an update waits"
        );
        let mut disabled = Machine::disabled(v("1.0.0"), "unpackaged".into());
        assert!(disabled.is_disabled());
        assert_eq!(disabled.step(Event::Check(Trigger::User)), None);
    }

    #[test]
    fn restart_hands_the_download_over_and_a_failed_install_gives_it_back() {
        let mut machine = ready();
        assert_eq!(
            machine.step(Event::Restart),
            Some(Effect::Install("setup.exe"))
        );
        assert_eq!(machine.banner().unwrap().message, "Restarting…");
        assert_eq!(machine.step(Event::Restart), None, "once");
        assert_eq!(
            machine.step(Event::InstallFailed {
                update: "setup.exe",
                error: "the installer did not start".into()
            }),
            None
        );
        assert_eq!(machine.banner().unwrap().severity, Severity::Error);
        assert_eq!(
            machine.step(Event::Restart),
            Some(Effect::Install("setup.exe"))
        );
        assert_eq!(fresh().step(Event::Restart), None, "nothing is ready");
    }

    #[test]
    fn quitting_installs_a_ready_update_only_with_automatic_updates_on() {
        let mut machine = ready();
        assert_eq!(machine.step(Event::Quit { automatic: false }), None);
        assert!(matches!(machine.state(), State::Ready { .. }));
        assert_eq!(
            machine.step(Event::Quit { automatic: true }),
            Some(Effect::InstallOnExit("setup.exe"))
        );
        assert!(matches!(machine.state(), State::Installing { .. }));
        assert_eq!(machine.step(Event::Quit { automatic: true }), None, "once");
        assert_eq!(fresh().step(Event::Quit { automatic: true }), None);

        let mut failed_before = ready();
        failed_before.step(Event::Restart);
        failed_before.step(Event::InstallFailed {
            update: "setup.exe",
            error: "boom".into(),
        });
        assert_eq!(
            failed_before.step(Event::Quit { automatic: true }),
            Some(Effect::InstallOnExit("setup.exe")),
            "a download whose installer failed to start is tried again on the way out"
        );
    }

    #[test]
    fn a_channel_change_drops_the_old_result_and_checks_at_once() {
        let mut machine = ready();
        let Some(Effect::Check { epoch }) = machine.step(Event::ChannelChanged) else {
            panic!("a ready update is dropped and the new channel checked");
        };
        assert_eq!(
            machine.state(),
            &S::Checking {
                trigger: Trigger::User
            }
        );
        assert!(machine.is_current(epoch));

        machine.step(Event::Failed("old channel".into()));
        let Some(Effect::Check { epoch: newer }) = machine.step(Event::ChannelChanged) else {
            panic!("a failed check is replaced by the new channel's");
        };
        assert_ne!(epoch, newer);
        assert!(
            !machine.is_current(epoch),
            "the first check began before the switch"
        );
        assert!(machine.is_current(newer));
    }

    #[test]
    fn a_channel_change_leaves_an_install_under_way_and_still_counts() {
        let mut machine = ready();
        let epoch = Epoch::default();
        machine.step(Event::Restart);
        assert_eq!(machine.step(Event::ChannelChanged), None);
        assert!(matches!(machine.state(), State::Installing { .. }));
        assert!(!machine.is_current(epoch));

        let mut disabled = Machine::disabled(v("1.0.0"), "unpackaged".into());
        assert_eq!(disabled.step(Event::ChannelChanged), None);
        assert!(disabled.is_disabled());
    }

    #[test]
    fn the_next_check_comes_sooner_after_a_failure() {
        let mut machine = fresh();
        assert_eq!(machine.next_check_in(), CHECK_INTERVAL);
        machine.step(Event::Check(Trigger::Scheduled));
        machine.step(Event::Failed("offline".into()));
        assert_eq!(machine.next_check_in(), RETRY_INTERVAL);
        machine.step(Event::Check(Trigger::Scheduled));
        machine.step(Event::UpToDate);
        assert_eq!(machine.next_check_in(), CHECK_INTERVAL);
    }

    #[test]
    fn a_shell_only_updates_installations_of_its_own_flavour() {
        let installed = |marker| Installation::from_parts(Some(marker), None);
        assert_eq!(Flavour::WinUi.refusal(&installed("nsis")), None);
        assert_eq!(
            Flavour::WinUi.refusal(&installed("deb")).as_deref(),
            Some("this copy is deb, not an NSIS installation")
        );
        assert_eq!(
            Flavour::WinUi.refusal(&Installation::Unpackaged).as_deref(),
            Some("this copy is unpackaged, not an NSIS installation")
        );

        for marker in ["deb", "rpm", "pacman"] {
            assert_eq!(Flavour::Gpui.refusal(&installed(marker)), None, "{marker}");
        }
        let appimage =
            Installation::from_parts(Some("appimage"), Some("/tmp/Vsesvit.AppImage".into()));
        assert_eq!(Flavour::Gpui.refusal(&appimage), None);
        assert!(
            Flavour::Gpui.refusal(&installed("nsis")).is_some(),
            "a GPUI build never takes the WinUI artifact"
        );
        assert!(Flavour::Gpui.refusal(&installed("flatpak")).is_some());
        assert!(Flavour::Gpui.refusal(&Installation::Unpackaged).is_some());
    }
}
