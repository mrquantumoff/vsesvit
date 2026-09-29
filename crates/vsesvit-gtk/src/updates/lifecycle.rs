//! The update lifecycle as a state machine, free of GTK and threads so it can be tested alone.
//! `D` is a verified download; the shell uses `vsesvit_update::Downloaded`, tests use a
//! stand-in.

use semver::Version;
use vsesvit_update::{DisabledReason, Error, Installed};

pub(crate) enum State<D> {
    Idle,
    Checking,
    /// The last check found nothing newer.
    UpToDate,
    Downloading {
        version: Version,
    },
    /// The user picked another channel while a check or download for the old one ran. Its
    /// result is dropped, and the new channel's check starts once it ends: one worker at a time
    /// keeps the two from writing the same download.
    Rechecking,
    /// Waiting for the user to click "Update", also after an install that failed.
    Ready {
        version: Version,
        downloaded: D,
    },
    Installing {
        version: Version,
    },
    Installed {
        version: Version,
        next: Installed,
    },
    /// The last check or download failed, with the error's message. The next check retries.
    Failed(String),
    Disabled(DisabledReason),
}

pub(crate) enum Event<D> {
    /// The timer or the user asks for a check.
    Check,
    /// The user picked another update channel.
    ChannelChanged,
    /// The banner's button.
    Activate,
    UpToDate,
    Found(Version),
    Downloaded(D),
    Installed(Installed),
    /// The install did not happen; the download comes back for the next try.
    InstallFailed {
        error: Error,
        downloaded: D,
    },
    Failed(Error),
}

/// What the shell does after a step.
pub(crate) enum Effect<D> {
    Check,
    Install(D),
    /// Quit, then start the new version.
    Restart,
    /// Quit; the installer starts the new version.
    Exit,
    /// An install the user asked for failed.
    Notify(String),
}

/// What every window's banner shows.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Banner {
    pub(crate) title: String,
    pub(crate) button: Option<&'static str>,
}

/// What the Updates group in Settings shows: the lifecycle in words, and its button.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) title: String,
    pub(crate) button: StatusButton,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StatusButton {
    /// "Check for Updates", insensitive while a check, download or install runs.
    Check { enabled: bool },
    /// The banner's own button, under its label, which activates `app.update` as the banner
    /// does.
    Update(&'static str),
}

pub(crate) struct Lifecycle<D> {
    state: State<D>,
    /// The running version, which Settings shows while nothing else is going on.
    current: Version,
    /// The AppImage is replaced as soon as the new one is verified, without asking; the other
    /// formats wait for the user because the package manager asks for a password.
    background_install: bool,
}

impl<D> Lifecycle<D> {
    pub(crate) fn new(current: Version, background_install: bool) -> Self {
        Lifecycle {
            state: State::Idle,
            current,
            background_install,
        }
    }

    pub(crate) fn state(&self) -> &State<D> {
        &self.state
    }

    pub(crate) fn step(&mut self, event: Event<D>) -> Option<Effect<D>> {
        let background = self.background_install;
        let (state, effect) = match (std::mem::replace(&mut self.state, State::Idle), event) {
            (_, Event::Failed(Error::Disabled(reason))) => (State::Disabled(reason), None),
            (State::Idle | State::UpToDate | State::Failed(_), Event::Check) => {
                (State::Checking, Some(Effect::Check))
            }
            // A download for the old channel is dropped with its state; the new channel's check
            // removes its file, unless that channel offers the same version.
            (
                State::Idle | State::UpToDate | State::Failed(_) | State::Ready { .. },
                Event::ChannelChanged,
            ) => (State::Checking, Some(Effect::Check)),
            (
                State::Checking | State::Downloading { .. } | State::Rechecking,
                Event::ChannelChanged,
            ) => (State::Rechecking, None),
            (State::Rechecking, Event::UpToDate | Event::Downloaded(_) | Event::Failed(_)) => {
                (State::Checking, Some(Effect::Check))
            }
            (State::Checking, Event::UpToDate) => (State::UpToDate, None),
            (State::Checking, Event::Found(version)) => (State::Downloading { version }, None),
            (State::Downloading { version }, Event::Downloaded(downloaded)) if background => (
                State::Installing { version },
                Some(Effect::Install(downloaded)),
            ),
            (State::Downloading { version }, Event::Downloaded(downloaded)) => (
                State::Ready {
                    version,
                    downloaded,
                },
                None,
            ),
            (
                State::Ready {
                    version,
                    downloaded,
                },
                Event::Activate,
            ) => (
                State::Installing { version },
                Some(Effect::Install(downloaded)),
            ),
            (State::Installing { version }, Event::Installed(next)) => {
                let effect = (next == Installed::ExitNow).then_some(Effect::Exit);
                (State::Installed { version, next }, effect)
            }
            (State::Installing { version }, Event::InstallFailed { error, downloaded })
                if !background =>
            {
                (
                    State::Ready {
                        version,
                        downloaded,
                    },
                    Some(Effect::Notify(error.to_string())),
                )
            }
            // The next check's download finds the verified file on disk and retries from there.
            (State::Installing { .. }, Event::InstallFailed { error, .. }) => {
                log::warn!("updating failed: {error}");
                (State::Idle, None)
            }
            (State::Checking | State::Downloading { .. }, Event::Failed(error)) => {
                log::warn!("updating failed: {error}");
                (State::Failed(error.to_string()), None)
            }
            // Only the install's worker thread failing to start; the download is gone with it,
            // and the next check finds the verified file on disk again.
            (State::Installing { .. }, Event::Failed(error)) => {
                log::warn!("updating failed: {error}");
                (State::Idle, None)
            }
            (
                state @ State::Installed {
                    next: Installed::Relaunch | Installed::NextLaunch,
                    ..
                },
                Event::Activate,
            ) => (state, Some(Effect::Restart)),
            (state, _) => (state, None),
        };
        self.state = state;
        effect
    }

    /// Whether a check started now would run, which is when Settings offers one.
    pub(crate) fn can_check(&self) -> bool {
        matches!(self.state, State::Idle | State::UpToDate | State::Failed(_))
    }

    pub(crate) fn status(&self) -> Status {
        let status = |title: String| Status {
            title,
            button: StatusButton::Check {
                enabled: self.can_check(),
            },
        };
        let update = |title: String, label| Status {
            title,
            button: StatusButton::Update(label),
        };
        match &self.state {
            State::Idle => status(format!("Vsesvit {}", self.current)),
            State::Checking | State::Rechecking => status("Checking for updates…".to_owned()),
            State::UpToDate => status("Vsesvit is up to date".to_owned()),
            State::Downloading { version } => status(format!("Downloading Vsesvit {version}…")),
            State::Ready { version, .. } => {
                update(format!("Vsesvit {version} is ready to install"), "Update")
            }
            State::Installing { version } => status(format!("Installing Vsesvit {version}…")),
            State::Installed {
                version,
                next: Installed::NextLaunch,
            } => update(
                format!("Vsesvit {version} will open next time"),
                "Restart now",
            ),
            State::Installed {
                next: Installed::Relaunch,
                ..
            } => update("Restart to finish updating".to_owned(), "Restart"),
            State::Installed {
                version,
                next: Installed::ExitNow,
            } => status(format!("Vsesvit {version} is installed")),
            State::Failed(message) => status(format!("Could not check for updates: {message}")),
            State::Disabled(reason) => status(format!("Updates are off: {reason}")),
        }
    }

    /// The banner shows the status while the user has something to do or wait for: an update
    /// to install, a restart, or an install they started.
    pub(crate) fn banner(&self) -> Option<Banner> {
        let shown = match &self.state {
            State::Ready { .. } => true,
            State::Installing { .. } => !self.background_install,
            State::Installed { next, .. } => *next != Installed::ExitNow,
            _ => false,
        };
        if !shown {
            return None;
        }
        let status = self.status();
        Some(Banner {
            title: status.title,
            button: match status.button {
                StatusButton::Update(label) => Some(label),
                StatusButton::Check { .. } => None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Machine = Lifecycle<&'static str>;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn found(machine: &mut Machine) {
        assert!(matches!(machine.step(Event::Check), Some(Effect::Check)));
        assert!(machine.step(Event::Found(v("2.0.0"))).is_none());
        assert!(matches!(machine.state(), State::Downloading { .. }));
    }

    fn banner(title: &str, button: Option<&'static str>) -> Option<Banner> {
        Some(Banner {
            title: title.to_owned(),
            button,
        })
    }

    fn status(title: &str, button: StatusButton) -> Status {
        Status {
            title: title.to_owned(),
            button,
        }
    }

    const CAN_CHECK: StatusButton = StatusButton::Check { enabled: true };
    const CANNOT_CHECK: StatusButton = StatusButton::Check { enabled: false };

    #[test]
    fn up_to_date_is_kept_until_the_next_check() {
        let mut machine = Machine::new(v("1.0.0"), false);
        assert_eq!(machine.status(), status("Vsesvit 1.0.0", CAN_CHECK));
        assert!(matches!(machine.step(Event::Check), Some(Effect::Check)));
        assert_eq!(
            machine.status(),
            status("Checking for updates…", CANNOT_CHECK)
        );
        assert!(machine.step(Event::Check).is_none(), "one check at a time");
        assert!(machine.step(Event::UpToDate).is_none());
        assert!(matches!(machine.state(), State::UpToDate));
        assert_eq!(machine.status(), status("Vsesvit is up to date", CAN_CHECK));
        assert_eq!(machine.banner(), None);
        assert!(matches!(machine.step(Event::Check), Some(Effect::Check)));
    }

    #[test]
    fn a_package_waits_for_the_user_then_asks_for_a_restart() {
        let mut machine = Machine::new(v("1.0.0"), false);
        found(&mut machine);
        assert_eq!(machine.banner(), None, "downloading is quiet");
        assert!(machine.step(Event::Downloaded("file")).is_none());
        assert_eq!(
            machine.banner(),
            banner("Vsesvit 2.0.0 is ready to install", Some("Update"))
        );
        assert!(
            machine.step(Event::Check).is_none(),
            "no checks while an update waits"
        );

        assert!(matches!(
            machine.step(Event::Activate),
            Some(Effect::Install("file"))
        ));
        assert_eq!(machine.banner(), banner("Installing Vsesvit 2.0.0…", None));
        assert!(
            machine.step(Event::Activate).is_none(),
            "one install at a time"
        );

        assert!(
            machine
                .step(Event::Installed(Installed::Relaunch))
                .is_none()
        );
        assert_eq!(
            machine.banner(),
            banner("Restart to finish updating", Some("Restart"))
        );
        assert!(matches!(
            machine.step(Event::Activate),
            Some(Effect::Restart)
        ));
        assert!(
            machine.step(Event::Check).is_none(),
            "nothing more to do until the restart"
        );
    }

    #[test]
    fn a_failed_install_keeps_the_download_for_the_retry() {
        let mut machine = Machine::new(v("1.0.0"), false);
        found(&mut machine);
        machine.step(Event::Downloaded("file"));
        machine.step(Event::Activate);
        let failed = Event::InstallFailed {
            error: Error::Install("administrator authorization was not given".into()),
            downloaded: "file",
        };
        match machine.step(failed) {
            Some(Effect::Notify(message)) => {
                assert!(message.contains("authorization"), "{message}")
            }
            _ => panic!("the user hears about it"),
        }
        assert_eq!(
            machine.banner(),
            banner("Vsesvit 2.0.0 is ready to install", Some("Update"))
        );
        assert!(
            machine.step(Event::Check).is_none(),
            "the retry does not check or download"
        );
        assert!(
            matches!(machine.step(Event::Activate), Some(Effect::Install("file"))),
            "Update installs the same file again"
        );
    }

    #[test]
    fn an_appimage_installs_in_the_background() {
        let mut machine = Machine::new(v("1.0.0"), true);
        found(&mut machine);
        let effect = machine.step(Event::Downloaded("file"));
        assert!(matches!(effect, Some(Effect::Install("file"))));
        assert_eq!(
            machine.banner(),
            None,
            "installing in the background is quiet"
        );
        assert!(
            machine
                .step(Event::Installed(Installed::NextLaunch))
                .is_none()
        );
        assert_eq!(
            machine.banner(),
            banner("Vsesvit 2.0.0 will open next time", Some("Restart now"))
        );
        assert!(matches!(
            machine.step(Event::Activate),
            Some(Effect::Restart)
        ));
    }

    #[test]
    fn a_failed_background_install_is_retried_by_the_next_check() {
        let mut machine = Machine::new(v("1.0.0"), true);
        found(&mut machine);
        machine.step(Event::Downloaded("file"));
        let failed = Event::InstallFailed {
            error: Error::Install("disk full".into()),
            downloaded: "file",
        };
        assert!(machine.step(failed).is_none());
        assert!(matches!(machine.state(), State::Idle));
        assert!(matches!(machine.step(Event::Check), Some(Effect::Check)));
    }

    #[test]
    fn check_and_download_failures_are_shown_until_the_next_check() {
        let mut machine = Machine::new(v("1.0.0"), false);
        machine.step(Event::Check);
        assert!(machine.step(Event::Failed(Error::Http(503))).is_none());
        assert_eq!(
            machine.status(),
            status(
                "Could not check for updates: update server returned HTTP 503",
                CAN_CHECK
            )
        );
        assert_eq!(machine.banner(), None);
        found(&mut machine);
        assert_eq!(
            machine.status(),
            status("Downloading Vsesvit 2.0.0…", CANNOT_CHECK)
        );
        assert!(
            machine
                .step(Event::Failed(Error::Network("reset".into())))
                .is_none()
        );
        assert!(matches!(machine.state(), State::Failed(message) if message == "network: reset"));
        assert_eq!(machine.banner(), None);
        assert!(matches!(machine.step(Event::Check), Some(Effect::Check)));
    }

    #[test]
    fn settings_offers_the_banners_button() {
        let mut machine = Machine::new(v("1.0.0"), false);
        found(&mut machine);
        machine.step(Event::Downloaded("file"));
        assert_eq!(
            machine.status(),
            status(
                "Vsesvit 2.0.0 is ready to install",
                StatusButton::Update("Update")
            )
        );
        machine.step(Event::Activate);
        assert_eq!(
            machine.status(),
            status("Installing Vsesvit 2.0.0…", CANNOT_CHECK)
        );
        machine.step(Event::Installed(Installed::Relaunch));
        assert_eq!(
            machine.status(),
            status(
                "Restart to finish updating",
                StatusButton::Update("Restart")
            )
        );
    }

    #[test]
    fn settings_shows_a_background_install_the_banner_keeps_quiet_about() {
        let mut machine = Machine::new(v("1.0.0"), true);
        found(&mut machine);
        machine.step(Event::Downloaded("file"));
        assert_eq!(machine.banner(), None);
        assert_eq!(
            machine.status(),
            status("Installing Vsesvit 2.0.0…", CANNOT_CHECK)
        );
        machine.step(Event::Installed(Installed::NextLaunch));
        assert_eq!(
            machine.status(),
            status(
                "Vsesvit 2.0.0 will open next time",
                StatusButton::Update("Restart now")
            )
        );
    }

    #[test]
    fn a_channel_change_checks_at_once_when_nothing_runs() {
        let mut machine = Machine::new(v("1.0.0"), false);
        assert!(matches!(
            machine.step(Event::ChannelChanged),
            Some(Effect::Check)
        ));
        machine.step(Event::UpToDate);
        assert!(matches!(
            machine.step(Event::ChannelChanged),
            Some(Effect::Check)
        ));
        machine.step(Event::Failed(Error::Http(503)));
        assert!(matches!(
            machine.step(Event::ChannelChanged),
            Some(Effect::Check)
        ));
    }

    #[test]
    fn a_channel_change_drops_the_download_waiting_for_update() {
        let mut machine = Machine::new(v("1.0.0"), false);
        found(&mut machine);
        machine.step(Event::Downloaded("old"));
        assert!(matches!(
            machine.step(Event::ChannelChanged),
            Some(Effect::Check)
        ));
        assert!(matches!(machine.state(), State::Checking));
        assert_eq!(machine.banner(), None);
        assert!(
            machine.step(Event::Activate).is_none(),
            "nothing left to install"
        );
    }

    #[test]
    fn a_check_for_the_old_channel_never_lands() {
        let mut machine = Machine::new(v("1.0.0"), false);
        found(&mut machine);
        assert!(machine.step(Event::ChannelChanged).is_none());
        assert_eq!(
            machine.status(),
            status("Checking for updates…", CANNOT_CHECK)
        );
        assert!(machine.step(Event::Check).is_none(), "one worker at a time");
        assert!(machine.step(Event::ChannelChanged).is_none());
        assert!(
            matches!(machine.step(Event::Downloaded("old")), Some(Effect::Check)),
            "the old channel's download ends in the new channel's check"
        );
        assert!(matches!(machine.state(), State::Checking));
        assert_eq!(machine.banner(), None);
        machine.step(Event::Found(v("3.0.0")));
        machine.step(Event::Downloaded("new"));
        assert_eq!(
            machine.banner(),
            banner("Vsesvit 3.0.0 is ready to install", Some("Update"))
        );
        assert!(matches!(
            machine.step(Event::Activate),
            Some(Effect::Install("new"))
        ));
    }

    #[test]
    fn any_end_of_the_old_channels_check_starts_the_new_one() {
        for end in [
            Event::UpToDate,
            Event::Failed(Error::Http(503)),
            Event::Downloaded("old"),
        ] {
            let mut machine = Machine::new(v("1.0.0"), true);
            machine.step(Event::Check);
            machine.step(Event::ChannelChanged);
            assert!(
                machine.step(Event::Found(v("2.0.0"))).is_none(),
                "the old download goes on"
            );
            assert!(matches!(machine.step(end), Some(Effect::Check)));
            assert!(matches!(machine.state(), State::Checking));
        }
    }

    #[test]
    fn an_install_is_past_a_channel_change() {
        let mut machine = Machine::new(v("1.0.0"), false);
        found(&mut machine);
        machine.step(Event::Downloaded("file"));
        machine.step(Event::Activate);
        assert!(machine.step(Event::ChannelChanged).is_none());
        assert!(matches!(machine.state(), State::Installing { .. }));
        machine.step(Event::Installed(Installed::Relaunch));
        assert!(machine.step(Event::ChannelChanged).is_none());
        assert!(matches!(
            machine.step(Event::Activate),
            Some(Effect::Restart)
        ));
    }

    #[test]
    fn disabled_is_terminal() {
        let mut machine = Machine::new(v("1.0.0"), false);
        machine.step(Event::Check);
        machine.step(Event::Failed(Error::Disabled(
            DisabledReason::NotSelfUpdating,
        )));
        assert!(matches!(
            machine.state(),
            State::Disabled(DisabledReason::NotSelfUpdating)
        ));
        assert!(machine.step(Event::Check).is_none());
        assert!(machine.step(Event::ChannelChanged).is_none());
        assert!(machine.step(Event::Activate).is_none());
        assert_eq!(machine.status().button, CANNOT_CHECK);
    }

    #[test]
    fn exit_now_quits_without_a_relaunch() {
        let mut machine = Machine::new(v("1.0.0"), false);
        found(&mut machine);
        machine.step(Event::Downloaded("file"));
        machine.step(Event::Activate);
        assert!(matches!(
            machine.step(Event::Installed(Installed::ExitNow)),
            Some(Effect::Exit)
        ));
        assert_eq!(machine.banner(), None);
    }

    #[test]
    fn the_button_does_nothing_without_an_update() {
        let mut machine = Machine::new(v("1.0.0"), false);
        assert!(machine.step(Event::Activate).is_none());
        machine.step(Event::Check);
        assert!(machine.step(Event::Activate).is_none());
        assert!(matches!(machine.state(), State::Checking));
    }
}
