//! The update lifecycle as a state machine, free of GTK and threads so it can be tested alone.
//! `U` and `D` are an offered update and its verified download; the shell uses
//! `vsesvit_update::Update` and `Downloaded`, tests use stand-ins.

use semver::Version;
use vsesvit_update::{DisabledReason, Error, Installed};

pub(crate) enum State<U, D> {
    Idle,
    Checking,
    Downloading {
        version: Version,
    },
    /// Waiting for the user to click "Update". `downloaded` is `None` after a failed install,
    /// which consumed the download, so the next attempt downloads again.
    Ready {
        version: Version,
        update: U,
        downloaded: Option<D>,
    },
    Installing {
        version: Version,
        update: U,
    },
    Installed {
        version: Version,
        next: Installed,
    },
    Disabled(DisabledReason),
}

pub(crate) enum Event<U, D> {
    /// The timer asks for a check.
    Check,
    /// The banner's button.
    Activate,
    UpToDate,
    Found(Version),
    Downloaded {
        update: U,
        downloaded: D,
    },
    Installed(Installed),
    Failed(Error),
}

/// What the shell does after a step.
pub(crate) enum Effect<U, D> {
    Check,
    Install {
        update: U,
        downloaded: Option<D>,
    },
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

pub(crate) struct Lifecycle<U, D> {
    state: State<U, D>,
    /// The AppImage is replaced as soon as the new one is verified, without asking; the other
    /// formats wait for the user because the package manager asks for a password.
    background_install: bool,
}

impl<U: Clone, D> Lifecycle<U, D> {
    pub(crate) fn new(background_install: bool) -> Self {
        Lifecycle {
            state: State::Idle,
            background_install,
        }
    }

    pub(crate) fn state(&self) -> &State<U, D> {
        &self.state
    }

    pub(crate) fn step(&mut self, event: Event<U, D>) -> Option<Effect<U, D>> {
        let background = self.background_install;
        let (state, effect) = match (std::mem::replace(&mut self.state, State::Idle), event) {
            (_, Event::Failed(Error::Disabled(reason))) => (State::Disabled(reason), None),
            (State::Idle, Event::Check) => (State::Checking, Some(Effect::Check)),
            (State::Checking, Event::UpToDate) => (State::Idle, None),
            (State::Checking, Event::Found(version)) => (State::Downloading { version }, None),
            (State::Downloading { version }, Event::Downloaded { update, downloaded })
                if background =>
            {
                (
                    State::Installing {
                        version,
                        update: update.clone(),
                    },
                    Some(Effect::Install {
                        update,
                        downloaded: Some(downloaded),
                    }),
                )
            }
            (State::Downloading { version }, Event::Downloaded { update, downloaded }) => (
                State::Ready {
                    version,
                    update,
                    downloaded: Some(downloaded),
                },
                None,
            ),
            (
                State::Ready {
                    version,
                    update,
                    downloaded,
                },
                Event::Activate,
            ) => (
                State::Installing {
                    version,
                    update: update.clone(),
                },
                Some(Effect::Install { update, downloaded }),
            ),
            (State::Installing { version, .. }, Event::Installed(next)) => {
                let effect = (next == Installed::ExitNow).then_some(Effect::Exit);
                (State::Installed { version, next }, effect)
            }
            (State::Installing { version, update }, Event::Failed(error)) if !background => (
                State::Ready {
                    version,
                    update,
                    downloaded: None,
                },
                Some(Effect::Notify(error.to_string())),
            ),
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
            State::Installing { version, .. } if !self.background_install => {
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

    type Machine = Lifecycle<&'static str, &'static str>;

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
        let downloaded = Event::Downloaded {
            update: "update",
            downloaded: "file",
        };
        assert!(machine.step(downloaded).is_none());
        assert_eq!(
            machine.banner(),
            banner("Vsesvit 2.0.0 is ready to install", Some("Update"))
        );
        assert!(
            machine.step(Event::Check).is_none(),
            "no checks while an update waits"
        );

        match machine.step(Event::Activate) {
            Some(Effect::Install { update, downloaded }) => {
                assert_eq!((update, downloaded), ("update", Some("file")))
            }
            _ => panic!("Update installs"),
        }
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
    fn a_failed_install_keeps_the_banner_and_downloads_again_on_retry() {
        let mut machine = Machine::new(false);
        found(&mut machine);
        machine.step(Event::Downloaded {
            update: "update",
            downloaded: "file",
        });
        machine.step(Event::Activate);
        let failed = Event::Failed(Error::Install(
            "administrator authorization was not given".into(),
        ));
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
        match machine.step(Event::Activate) {
            Some(Effect::Install { update, downloaded }) => {
                assert_eq!((update, downloaded), ("update", None))
            }
            _ => panic!("Update retries"),
        }
    }

    #[test]
    fn an_appimage_installs_in_the_background() {
        let mut machine = Machine::new(true);
        found(&mut machine);
        let effect = machine.step(Event::Downloaded {
            update: "update",
            downloaded: "file",
        });
        assert!(matches!(
            effect,
            Some(Effect::Install {
                downloaded: Some("file"),
                ..
            })
        ));
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
        machine.step(Event::Downloaded {
            update: "update",
            downloaded: "file",
        });
        assert!(
            machine
                .step(Event::Failed(Error::Install("disk full".into())))
                .is_none()
        );
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
        machine.step(Event::Downloaded {
            update: "update",
            downloaded: "file",
        });
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
