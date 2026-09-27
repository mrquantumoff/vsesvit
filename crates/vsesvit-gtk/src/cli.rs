//! `vsesvit [URL...] [--profile-dir DIR] | --self-test OUT_DIR [--network] | --check-for-updates | --update`

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
Usage: vsesvit [URL...] [--profile-dir DIR]
       vsesvit --self-test OUT_DIR [--network]
       vsesvit --check-for-updates
       vsesvit --update

Options:
  --profile-dir DIR     keep browsing data in DIR instead of the default profile
  --self-test OUT_DIR   run the end-to-end self-test on a fresh profile and write
                        report.json and window.png to OUT_DIR
  --network             with --self-test: also install from the Chrome Web Store
  --check-for-updates   print the newest available version as JSON, without opening a window
  --update              download and install the newest version, then print the outcome as JSON
  -h, --help            show this help
  --version             show the version";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Browse {
        targets: Vec<OsString>,
        profile_dir: Option<PathBuf>,
    },
    SelfTest {
        out_dir: PathBuf,
        network: bool,
    },
    CheckForUpdates,
    Update,
    Help,
    Version,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CliError {
    MissingValue(&'static str),
    Repeated(&'static str),
    UnknownOption(String),
    /// The self-test always runs on a fresh profile and its own fixture pages, and the update
    /// commands open no window.
    Exclusive(&'static str),
    /// `--network` only means something to the self-test.
    NetworkWithoutSelfTest,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::MissingValue(opt) => write!(f, "{opt} needs a value"),
            CliError::Repeated(opt) => write!(f, "{opt} was given more than once"),
            CliError::UnknownOption(opt) => write!(f, "unknown option {opt}"),
            CliError::Exclusive(opt) => {
                write!(f, "{opt} cannot be combined with URLs or other options")
            }
            CliError::NetworkWithoutSelfTest => write!(f, "--network needs --self-test"),
        }
    }
}

impl std::error::Error for CliError {}

pub(crate) const PROFILE_DIR: &str = "--profile-dir";
const SELF_TEST: &str = "--self-test";
const NETWORK: &str = "--network";
const CHECK_FOR_UPDATES: &str = "--check-for-updates";
const UPDATE: &str = "--update";

/// Parses the arguments after the program name.
pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, CliError> {
    let mut args = args.into_iter();
    let mut targets = Vec::new();
    let mut profile_dir: Option<PathBuf> = None;
    let mut standalone: Option<(&'static str, Command)> = None;
    let mut network = false;

    while let Some(arg) = args.next() {
        let Some(text) = arg.to_str().filter(|t| t.starts_with('-') && t.len() > 1) else {
            targets.push(arg);
            continue;
        };
        match text {
            "--" => {
                targets.extend(args.by_ref());
                break;
            }
            "-h" | "--help" => return Ok(Command::Help),
            "--version" => return Ok(Command::Version),
            NETWORK => {
                if std::mem::replace(&mut network, true) {
                    return Err(CliError::Repeated(NETWORK));
                }
            }
            CHECK_FOR_UPDATES => {
                set_standalone(&mut standalone, CHECK_FOR_UPDATES, Command::CheckForUpdates)?
            }
            UPDATE => set_standalone(&mut standalone, UPDATE, Command::Update)?,
            _ => {
                let (name, inline) = match text.split_once('=') {
                    Some((name, value)) => (name, Some(OsString::from(value))),
                    None => (text, None),
                };
                let name = match name {
                    PROFILE_DIR => PROFILE_DIR,
                    SELF_TEST => SELF_TEST,
                    _ => return Err(CliError::UnknownOption(text.to_owned())),
                };
                let value = PathBuf::from(
                    inline
                        .or_else(|| args.next())
                        .filter(|v| !v.is_empty())
                        .ok_or(CliError::MissingValue(name))?,
                );
                if name == SELF_TEST {
                    // `network` is folded in once every argument is read.
                    let command = Command::SelfTest {
                        out_dir: value,
                        network: false,
                    };
                    set_standalone(&mut standalone, SELF_TEST, command)?;
                } else if profile_dir.replace(value).is_some() {
                    return Err(CliError::Repeated(name));
                }
            }
        }
    }

    match standalone {
        Some((name, _)) if !targets.is_empty() || profile_dir.is_some() => {
            Err(CliError::Exclusive(name))
        }
        Some((_, Command::SelfTest { out_dir, .. })) => Ok(Command::SelfTest { out_dir, network }),
        Some(_) | None if network => Err(CliError::NetworkWithoutSelfTest),
        Some((_, command)) => Ok(command),
        None => Ok(Command::Browse {
            targets,
            profile_dir,
        }),
    }
}

/// A command that runs alone: given twice it is repeated, next to another one it is exclusive.
fn set_standalone(
    slot: &mut Option<(&'static str, Command)>,
    name: &'static str,
    command: Command,
) -> Result<(), CliError> {
    match slot.replace((name, command)) {
        None => Ok(()),
        Some((previous, _)) if previous == name => Err(CliError::Repeated(name)),
        Some((previous, _)) => Err(CliError::Exclusive(previous)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Command, CliError> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_browses_without_targets() {
        assert_eq!(
            run(&[]),
            Ok(Command::Browse {
                targets: vec![],
                profile_dir: None
            })
        );
    }

    #[test]
    fn urls_and_profile_dir_in_any_order() {
        assert_eq!(
            run(&[
                "https://example.com",
                "--profile-dir",
                "/tmp/p",
                "page.html"
            ]),
            Ok(Command::Browse {
                targets: vec!["https://example.com".into(), "page.html".into()],
                profile_dir: Some("/tmp/p".into()),
            })
        );
        assert_eq!(
            run(&["--profile-dir=/tmp/p"]),
            Ok(Command::Browse {
                targets: vec![],
                profile_dir: Some("/tmp/p".into())
            })
        );
    }

    #[test]
    fn self_test_takes_an_output_dir_and_a_network_flag() {
        assert_eq!(
            run(&["--self-test", "out"]),
            Ok(Command::SelfTest {
                out_dir: "out".into(),
                network: false
            })
        );
        assert_eq!(
            run(&["--self-test=out", "--network"]),
            Ok(Command::SelfTest {
                out_dir: "out".into(),
                network: true
            })
        );
        assert_eq!(
            run(&["--network", "--self-test", "out"]),
            Ok(Command::SelfTest {
                out_dir: "out".into(),
                network: true
            })
        );
    }

    #[test]
    fn self_test_is_exclusive_and_network_needs_it() {
        assert_eq!(
            run(&["--self-test", "out", "https://x"]),
            Err(CliError::Exclusive(SELF_TEST))
        );
        assert_eq!(
            run(&["--self-test", "out", "--profile-dir", "p"]),
            Err(CliError::Exclusive(SELF_TEST))
        );
        assert_eq!(
            run(&["--self-test", "a", "--self-test", "b"]),
            Err(CliError::Repeated(SELF_TEST))
        );
        assert_eq!(run(&["--network"]), Err(CliError::NetworkWithoutSelfTest));
        assert_eq!(
            run(&["--check-for-updates", "--network"]),
            Err(CliError::NetworkWithoutSelfTest)
        );
        assert_eq!(
            run(&["--self-test", "out", "--network", "--network"]),
            Err(CliError::Repeated(NETWORK))
        );
    }

    #[test]
    fn update_commands_run_alone() {
        assert_eq!(run(&["--check-for-updates"]), Ok(Command::CheckForUpdates));
        assert_eq!(run(&["--update"]), Ok(Command::Update));
        assert_eq!(
            run(&["--update", "https://x"]),
            Err(CliError::Exclusive(UPDATE))
        );
        assert_eq!(
            run(&["--profile-dir", "p", "--check-for-updates"]),
            Err(CliError::Exclusive(CHECK_FOR_UPDATES))
        );
        assert_eq!(
            run(&["--check-for-updates", "--update"]),
            Err(CliError::Exclusive(CHECK_FOR_UPDATES))
        );
        assert_eq!(
            run(&["--update", "--self-test", "out"]),
            Err(CliError::Exclusive(UPDATE))
        );
        assert_eq!(
            run(&["--check-for-updates", "--self-test", "x"]),
            Err(CliError::Exclusive(CHECK_FOR_UPDATES))
        );
        assert_eq!(
            run(&["--update", "--update"]),
            Err(CliError::Repeated(UPDATE))
        );
        assert_eq!(
            run(&["--update=now"]),
            Err(CliError::UnknownOption("--update=now".into()))
        );
    }

    #[test]
    fn option_values_are_required_and_unique() {
        assert_eq!(
            run(&["--profile-dir"]),
            Err(CliError::MissingValue(PROFILE_DIR))
        );
        assert_eq!(
            run(&["--profile-dir="]),
            Err(CliError::MissingValue(PROFILE_DIR))
        );
        assert_eq!(
            run(&["--profile-dir", "a", "--profile-dir", "b"]),
            Err(CliError::Repeated(PROFILE_DIR))
        );
    }

    #[test]
    fn unknown_options_are_rejected_but_dash_dash_ends_options() {
        assert_eq!(
            run(&["--frobnicate"]),
            Err(CliError::UnknownOption("--frobnicate".into()))
        );
        assert_eq!(
            run(&["--", "--weird-file-name"]),
            Ok(Command::Browse {
                targets: vec!["--weird-file-name".into()],
                profile_dir: None
            })
        );
        assert_eq!(
            run(&["-"]),
            Ok(Command::Browse {
                targets: vec!["-".into()],
                profile_dir: None
            })
        );
    }

    #[test]
    fn help_and_version() {
        assert_eq!(run(&["https://x", "--help"]), Ok(Command::Help));
        assert_eq!(run(&["-h"]), Ok(Command::Help));
        assert_eq!(run(&["--version"]), Ok(Command::Version));
    }
}
