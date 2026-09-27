//! Command line: `vsesvit [URL...] [--profile-dir DIR] [--self-test OUT_DIR [--network]]`, or
//! one of the update commands on its own.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
usage: vsesvit [URL...] [--profile-dir DIR] [--self-test OUT_DIR [--network]]
       vsesvit --check-for-updates | --update

  URL...                    open each URL in a tab (in the running instance, if there is one)
  --profile-dir DIR         use DIR as the profile
                            (default %LOCALAPPDATA%\\Vsesvit\\data\\profiles\\Default)
  --self-test OUT_DIR       run the end-to-end self-test with a fresh profile in OUT_DIR
  --network                 with --self-test: also install from the Chrome Web Store
  --load-extension DIR      install an unpacked extension folder into the profile
  --ui-smoke OUT_DIR        drive the UI, save screenshots and smoke.json to OUT_DIR, exit
  --check-for-updates       print one JSON line saying whether an update is available, exit
  --update                  download and start installing an available update (progress on
                            stderr), print the outcome as one JSON line, exit
  --version                 show the version
  -h, --help                show this help";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Parsed {
    Run(Args),
    Help,
    Version,
    Update(UpdateCommand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateCommand {
    Check,
    Install,
}

impl UpdateCommand {
    fn flag(self) -> &'static str {
        match self {
            Self::Check => "--check-for-updates",
            Self::Install => "--update",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Args {
    pub urls: Vec<String>,
    pub profile_dir: Option<PathBuf>,
    pub run: RunKind,
    pub load_extensions: Vec<PathBuf>,
    pub network: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum RunKind {
    #[default]
    Browse,
    SelfTest(PathBuf),
    UiSmoke(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CliError {
    MissingValue(&'static str),
    UnknownOption(String),
    NotUnicode(OsString),
    Duplicate(&'static str),
    Conflict(&'static str, &'static str),
    Requires(&'static str, &'static str),
    UnexpectedValue(&'static str),
    Alone(&'static str),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(flag) => write!(f, "{flag} needs a value"),
            Self::UnknownOption(flag) => write!(f, "unknown option {flag}"),
            Self::NotUnicode(arg) => write!(f, "argument is not valid Unicode: {}", arg.display()),
            Self::Duplicate(flag) => write!(f, "{flag} given more than once"),
            Self::Conflict(a, b) => write!(f, "{a} cannot be combined with {b}"),
            Self::Requires(a, b) => write!(f, "{a} needs {b}"),
            Self::UnexpectedValue(flag) => write!(f, "{flag} takes no value"),
            Self::Alone(flag) => write!(f, "{flag} takes no other arguments"),
        }
    }
}

impl std::error::Error for CliError {}

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, CliError> {
    let mut parsed = Args::default();
    let mut command: Option<UpdateCommand> = None;
    let mut args = args.into_iter();
    let mut only_urls = false;
    while let Some(arg) = args.next() {
        let arg = arg.into_string().map_err(CliError::NotUnicode)?;
        if only_urls || !arg.starts_with('-') {
            parsed.urls.push(arg);
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_owned(), Some(value.to_owned())),
            None => (arg, None),
        };
        let mut value = |name: &'static str| -> Result<PathBuf, CliError> {
            match inline.clone() {
                Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
                Some(_) => Err(CliError::MissingValue(name)),
                None => args
                    .next()
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
                    .ok_or(CliError::MissingValue(name)),
            }
        };
        match flag.as_str() {
            "--" => only_urls = true,
            "-h" | "--help" => return Ok(Parsed::Help),
            "--version" => return Ok(Parsed::Version),
            "--check-for-updates" | "--update" => {
                let next = if flag == "--update" {
                    UpdateCommand::Install
                } else {
                    UpdateCommand::Check
                };
                if inline.is_some() {
                    return Err(CliError::UnexpectedValue(next.flag()));
                }
                match command.replace(next) {
                    Some(previous) if previous == next => {
                        return Err(CliError::Duplicate(next.flag()));
                    }
                    Some(previous) => return Err(CliError::Conflict(previous.flag(), next.flag())),
                    None => {}
                }
            }
            "--profile-dir" => {
                let dir = value("--profile-dir")?;
                if parsed.profile_dir.replace(dir).is_some() {
                    return Err(CliError::Duplicate("--profile-dir"));
                }
            }
            "--self-test" => {
                let dir = value("--self-test")?;
                set_run(&mut parsed.run, RunKind::SelfTest(dir))?;
            }
            "--ui-smoke" => {
                let dir = value("--ui-smoke")?;
                set_run(&mut parsed.run, RunKind::UiSmoke(dir))?;
            }
            "--load-extension" => {
                let dir = value("--load-extension")?;
                parsed.load_extensions.push(dir);
            }
            "--network" => {
                if inline.is_some() {
                    return Err(CliError::UnexpectedValue("--network"));
                }
                if std::mem::replace(&mut parsed.network, true) {
                    return Err(CliError::Duplicate("--network"));
                }
            }
            _ => return Err(CliError::UnknownOption(flag)),
        }
    }
    if let Some(command) = command {
        if parsed != Args::default() {
            return Err(CliError::Alone(command.flag()));
        }
        return Ok(Parsed::Update(command));
    }
    if matches!(parsed.run, RunKind::SelfTest(_)) && parsed.profile_dir.is_some() {
        return Err(CliError::Conflict("--self-test", "--profile-dir"));
    }
    if parsed.network && !matches!(parsed.run, RunKind::SelfTest(_)) {
        return Err(CliError::Requires("--network", "--self-test"));
    }
    Ok(Parsed::Run(parsed))
}

fn set_run(current: &mut RunKind, next: RunKind) -> Result<(), CliError> {
    let name = |kind: &RunKind| match kind {
        RunKind::Browse => "",
        RunKind::SelfTest(_) => "--self-test",
        RunKind::UiSmoke(_) => "--ui-smoke",
    };
    match current {
        RunKind::Browse => {
            *current = next;
            Ok(())
        }
        existing if name(existing) == name(&next) => Err(CliError::Duplicate(name(&next))),
        existing => Err(CliError::Conflict(name(existing), name(&next))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Parsed, CliError> {
        parse(args.iter().map(OsString::from))
    }

    fn browse(args: &[&str]) -> Args {
        match run(args) {
            Ok(Parsed::Run(args)) => args,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn urls_and_profile_dir() {
        let args = browse(&["https://a.test/", "--profile-dir", r"C:\p", "b.test"]);
        assert_eq!(args.urls, ["https://a.test/", "b.test"]);
        assert_eq!(args.profile_dir, Some(PathBuf::from(r"C:\p")));
        assert_eq!(args.run, RunKind::Browse);
    }

    #[test]
    fn inline_values() {
        let args = browse(&[r"--profile-dir=D:\x", "--ui-smoke=out"]);
        assert_eq!(args.profile_dir, Some(PathBuf::from("D:\\x")));
        assert_eq!(args.run, RunKind::UiSmoke(PathBuf::from("out")));
    }

    #[test]
    fn self_test_with_profile_dir_conflicts() {
        assert_eq!(
            run(&["--self-test", "out", "--profile-dir", "p"]),
            Err(CliError::Conflict("--self-test", "--profile-dir"))
        );
    }

    #[test]
    fn modes_are_exclusive() {
        assert_eq!(
            run(&["--self-test", "a", "--ui-smoke", "b"]),
            Err(CliError::Conflict("--self-test", "--ui-smoke"))
        );
        assert_eq!(
            run(&["--ui-smoke", "a", "--ui-smoke", "b"]),
            Err(CliError::Duplicate("--ui-smoke"))
        );
    }

    #[test]
    fn missing_values_and_unknown_flags() {
        assert_eq!(
            run(&["--profile-dir"]),
            Err(CliError::MissingValue("--profile-dir"))
        );
        assert_eq!(
            run(&["--self-test="]),
            Err(CliError::MissingValue("--self-test"))
        );
        assert_eq!(
            run(&["--bogus"]),
            Err(CliError::UnknownOption("--bogus".into()))
        );
        assert_eq!(
            run(&["--profile-dir", "a", "--profile-dir", "b"]),
            Err(CliError::Duplicate("--profile-dir"))
        );
    }

    #[test]
    fn double_dash_ends_options() {
        let args = browse(&["--", "--not-a-flag"]);
        assert_eq!(args.urls, ["--not-a-flag"]);
    }

    #[test]
    fn help_and_version() {
        assert_eq!(run(&["x", "--help"]), Ok(Parsed::Help));
        assert_eq!(run(&["--version"]), Ok(Parsed::Version));
        assert_eq!(
            run(&["--profile-dir", "p", "--version"]),
            Ok(Parsed::Version)
        );
    }

    #[test]
    fn update_commands_stand_alone() {
        assert_eq!(
            run(&["--check-for-updates"]),
            Ok(Parsed::Update(UpdateCommand::Check))
        );
        assert_eq!(
            run(&["--update"]),
            Ok(Parsed::Update(UpdateCommand::Install))
        );
        assert_eq!(
            run(&["--update", "https://a.test/"]),
            Err(CliError::Alone("--update"))
        );
        assert_eq!(
            run(&["--profile-dir", "p", "--check-for-updates"]),
            Err(CliError::Alone("--check-for-updates"))
        );
        assert_eq!(
            run(&["--check-for-updates", "--update"]),
            Err(CliError::Conflict("--check-for-updates", "--update"))
        );
        assert_eq!(
            run(&["--update", "--update"]),
            Err(CliError::Duplicate("--update"))
        );
        assert_eq!(
            run(&["--update=now"]),
            Err(CliError::UnexpectedValue("--update"))
        );
        assert_eq!(run(&["--update", "--help"]), Ok(Parsed::Help));
    }

    #[test]
    fn network_only_with_self_test() {
        let args = browse(&["--self-test", "out", "--network"]);
        assert!(args.network);
        assert_eq!(args.run, RunKind::SelfTest(PathBuf::from("out")));
        assert_eq!(
            run(&["--network"]),
            Err(CliError::Requires("--network", "--self-test"))
        );
        assert_eq!(
            run(&["--self-test", "o", "--network=yes"]),
            Err(CliError::UnexpectedValue("--network"))
        );
    }

    #[test]
    fn load_extension_repeats() {
        let args = browse(&["--load-extension", "a", "--load-extension=b"]);
        assert_eq!(
            args.load_extensions,
            [PathBuf::from("a"), PathBuf::from("b")]
        );
    }
}
