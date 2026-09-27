//! The update lifecycle as a state machine, free of GTK and threads so it can be tested alone.
//! `D` is a verified download; the shell uses `vsesvit_update::Downloaded`, tests use a
//! stand-in.

use semver::Version;
use vsesvit_update::{DisabledReason, Error, Installed};

pub(crate) enum State<D> {
    Idle,
    Checking,
    Downloading {
        version: Version,
    },
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
    Disabled(DisabledReason),
}

pub(crate) enum Event<D> {
    /// The timer asks for a check.
    Check,
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

pub(crate) struct Lifecycle<D> {
    state: State<D>,
    /// The AppImage is replaced as soon as the new one is verified, without asking; the other
    /// formats wait for the user because the package manager asks for a password.
    background_install: bool,
}

impl<D> Lifecycle<D> {
    pub(crate) fn new(background_install: bool) -> Self {
        Lifecycle {
            state: State::Idle,
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
            (State::Idle, Event::Check) => (State::Checking, Some(Effect::Check)),
            (State::Checking, Event::UpToDate) => (State::Idle, None),
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
            (
                State::Checking | State::Downloading { .. } | State::Installing { .. },
                Event::Failed(error),
            ) => {
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

    pub(crate) fn banner(&self) -> Option<Banner> {
        let banner = |title: String, button| Some(Banner { title, button });
        match &self.state {
            State::Ready { version, .. } => banner(
                format!("Vsesvit {version} is ready to install"),
                Some("Update"),
            ),
            State::Installing { version } if !self.background_install => {
                banner(format!("Installing Vsesvit {version}…"), None)
            }
            State::Installed {
                version,
                next: Installed::NextLaunch,
            } => banner(
                format!("Vsesvit {version} will open next time"),
                Some("Restart now"),
            ),
            State::Installed {
                next: Installed::Relaunch,
                ..
            } => banner("Restart to finish updating".to_owned(), Some("Restart")),
            _ => None,
        }
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

    #[test]
    fn up_to_date_returns_to_idle_and_checks_again() {
        let mut machine = Machine::new(false);
        assert!(matches!(machine.step(Event::Check), Some(Effect::Check)));
        assert!(machine.step(Event::Check).is_none(), "one check at a time");
        assert!(machine.step(Event::UpToDate).is_none());
        assert!(matches!(machine.state(), State::Idle));
        assert!(matches!(machine.step(Event::Check), Some(Effect::Check)));
    }

    #[test]
    fn a_package_waits_for_the_user_then_asks_for_a_restart() {
        let mut machine = Machine::new(false);
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
        let mut machine = Machine::new(false);
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
        let mut machine = Machine::new(true);
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
        let mut machine = Machine::new(true);
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
    fn check_and_download_failures_return_to_idle() {
        let mut machine = Machine::new(false);
        machine.step(Event::Check);
        assert!(machine.step(Event::Failed(Error::Http(503))).is_none());
        assert!(matches!(machine.state(), State::Idle));
        found(&mut machine);
        assert!(
            machine
                .step(Event::Failed(Error::Network("reset".into())))
                .is_none()
        );
        assert!(matches!(machine.state(), State::Idle));
        assert_eq!(machine.banner(), None);
    }

    #[test]
    fn disabled_is_terminal() {
        let mut machine = Machine::new(false);
        machine.step(Event::Check);
        machine.step(Event::Failed(Error::Disabled(
            DisabledReason::NotSelfUpdating,
        )));
        assert!(matches!(
            machine.state(),
            State::Disabled(DisabledReason::NotSelfUpdating)
        ));
        assert!(machine.step(Event::Check).is_none());
        assert!(machine.step(Event::Activate).is_none());
    }

    #[test]
    fn exit_now_quits_without_a_relaunch() {
        let mut machine = Machine::new(false);
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
        let mut machine = Machine::new(false);
        assert!(machine.step(Event::Activate).is_none());
        machine.step(Event::Check);
        assert!(machine.step(Event::Activate).is_none());
        assert!(matches!(machine.state(), State::Checking));
    }
}
