//! Owned by W0.3.
//! The command line every shell parses: grammar, usage text, errors and the run config, with the
//! flags each shell accepts (docs/design/linux-cef/design.md section 4.3, row 1).
//!
//! `vsesvit [URL...] [--profile-dir DIR] [--self-test OUT_DIR [--network] [--phase NAME]]`, or one
//! of the update commands on its own. One grammar serves every shell; [`Flag::accepted_by`] is the
//! per-shell table, and a flag the running shell does not accept is a [`CliError::NotAccepted`].
//! Every [`CliError`] exits with [`EXIT_USAGE`].

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::profiles::{Home, ProfileId, ProfilesDir, Startup};

/// The exit code of a command line that does not parse.
pub const EXIT_USAGE: u8 = 2;

/// The shell that parses the command line.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Shell {
    WinUi,
    Gpui,
}

/// Every option of the grammar.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Flag {
    ProfileDir,
    SelfTest,
    Network,
    /// A re-executed phase of the self-test (design section 3.6 and 7.2).
    Phase,
    LoadExtension,
    UiSmoke,
    CheckForUpdates,
    Update,
    Version,
    Help,
}

impl Flag {
    const ALL: [Flag; 10] = [
        Flag::ProfileDir,
        Flag::SelfTest,
        Flag::Network,
        Flag::Phase,
        Flag::LoadExtension,
        Flag::UiSmoke,
        Flag::CheckForUpdates,
        Flag::Update,
        Flag::Version,
        Flag::Help,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Flag::ProfileDir => "--profile-dir",
            Flag::SelfTest => "--self-test",
            Flag::Network => "--network",
            Flag::Phase => "--phase",
            Flag::LoadExtension => "--load-extension",
            Flag::UiSmoke => "--ui-smoke",
            Flag::CheckForUpdates => "--check-for-updates",
            Flag::Update => "--update",
            Flag::Version => "--version",
            Flag::Help => "--help",
        }
    }

    /// The per-shell flag table. `--ui-smoke` drives WinUI's XAML; only the GPUI self-test
    /// re-executes itself in phases.
    pub const fn accepted_by(self, shell: Shell) -> bool {
        match self {
            Flag::UiSmoke => matches!(shell, Shell::WinUi),
            Flag::Phase => matches!(shell, Shell::Gpui),
            _ => true,
        }
    }

    fn from_name(name: &str) -> Option<Flag> {
        match name {
            "-h" => Some(Flag::Help),
            _ => Flag::ALL.into_iter().find(|flag| flag.name() == name),
        }
    }

    /// Its line in the usage text.
    fn usage(self) -> &'static str {
        match self {
            Flag::ProfileDir if cfg!(windows) => {
                "  --profile-dir DIR         use DIR as the profile
                            (default %LOCALAPPDATA%\\Vsesvit\\data\\profiles\\Default)"
            }
            Flag::ProfileDir => {
                "  --profile-dir DIR         use DIR as the profile
                            (default ~/.local/share/vsesvit/profiles/Default)"
            }
            Flag::SelfTest => "  --self-test OUT_DIR       run the end-to-end self-test with a fresh profile in OUT_DIR",
            Flag::Network => "  --network                 with --self-test: also install from the Chrome Web Store",
            Flag::Phase => "  --phase NAME              with --self-test: run the named phase the self-test re-executes",
            Flag::LoadExtension => "  --load-extension DIR      install an unpacked extension folder into the profile",
            Flag::UiSmoke => "  --ui-smoke OUT_DIR        drive the UI, save screenshots and smoke.json to OUT_DIR, exit",
            Flag::CheckForUpdates => {
                "  --check-for-updates       print one JSON line saying whether an update is available, exit"
            }
            Flag::Update => {
                "  --update                  download and start installing an available update (progress on
                            stderr), print the outcome as one JSON line, exit"
            }
            Flag::Version => "  --version                 show the version",
            Flag::Help => "  -h, --help                show this help",
        }
    }
}

/// The help text `shell` prints: only the flags it accepts.
pub fn usage(shell: Shell) -> String {
    let mut text = String::from(
        "usage: vsesvit [URL...] [--profile-dir DIR] [--self-test OUT_DIR [--network]]
       vsesvit --check-for-updates | --update

  URL...                    open each URL in a tab (in the running instance, if there is one)",
    );
    for flag in Flag::ALL.into_iter().filter(|flag| flag.accepted_by(shell)) {
        text.push('\n');
        text.push_str(flag.usage());
    }
    text
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Args),
    Help,
    Version,
    Update(UpdateCommand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateCommand {
    Check,
    Install,
}

impl UpdateCommand {
    fn flag(self) -> &'static str {
        match self {
            Self::Check => Flag::CheckForUpdates.name(),
            Self::Install => Flag::Update.name(),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub urls: Vec<String>,
    pub profile_dir: Option<PathBuf>,
    pub run: RunKind,
    pub load_extensions: Vec<PathBuf>,
    pub network: bool,
    pub phase: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum RunKind {
    #[default]
    Browse,
    SelfTest(PathBuf),
    UiSmoke(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    MissingValue(&'static str),
    UnknownOption(String),
    NotUnicode(OsString),
    Duplicate(&'static str),
    Conflict(&'static str, &'static str),
    Requires(&'static str, &'static str),
    UnexpectedValue(&'static str),
    Alone(&'static str),
    /// A flag of the grammar that this shell does not accept.
    NotAccepted(&'static str),
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
            Self::NotAccepted(flag) => write!(f, "{flag} is not available in this build"),
        }
    }
}

impl std::error::Error for CliError {}

/// Parses the arguments after the program name, as `shell` accepts them.
pub fn parse(shell: Shell, args: impl IntoIterator<Item = OsString>) -> Result<Parsed, CliError> {
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
        if arg == "--" {
            only_urls = true;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
            None => (arg, None),
        };
        let flag = Flag::from_name(&name).ok_or(CliError::UnknownOption(name))?;
        if !flag.accepted_by(shell) {
            return Err(CliError::NotAccepted(flag.name()));
        }
        let mut value = || -> Result<OsString, CliError> {
            match inline.clone() {
                Some(v) if !v.is_empty() => Ok(v.into()),
                Some(_) => Err(CliError::MissingValue(flag.name())),
                None => args.next().filter(|v| !v.is_empty()).ok_or(CliError::MissingValue(flag.name())),
            }
        };
        let no_value = |inline: &Option<String>| match inline {
            Some(_) => Err(CliError::UnexpectedValue(flag.name())),
            None => Ok(()),
        };
        match flag {
            Flag::Help => return Ok(Parsed::Help),
            Flag::Version => return Ok(Parsed::Version),
            Flag::CheckForUpdates | Flag::Update => {
                let next = if flag == Flag::Update { UpdateCommand::Install } else { UpdateCommand::Check };
                no_value(&inline)?;
                match command.replace(next) {
                    Some(previous) if previous == next => return Err(CliError::Duplicate(next.flag())),
                    Some(previous) => return Err(CliError::Conflict(previous.flag(), next.flag())),
                    None => {}
                }
            }
            Flag::ProfileDir => {
                let dir = PathBuf::from(value()?);
                if parsed.profile_dir.replace(dir).is_some() {
                    return Err(CliError::Duplicate(flag.name()));
                }
            }
            Flag::SelfTest => {
                let dir = PathBuf::from(value()?);
                set_run(&mut parsed.run, RunKind::SelfTest(dir))?;
            }
            Flag::UiSmoke => {
                let dir = PathBuf::from(value()?);
                set_run(&mut parsed.run, RunKind::UiSmoke(dir))?;
            }
            Flag::LoadExtension => {
                let dir = PathBuf::from(value()?);
                parsed.load_extensions.push(dir);
            }
            Flag::Phase => {
                let phase = value()?.into_string().map_err(CliError::NotUnicode)?;
                if parsed.phase.replace(phase).is_some() {
                    return Err(CliError::Duplicate(flag.name()));
                }
            }
            Flag::Network => {
                no_value(&inline)?;
                if std::mem::replace(&mut parsed.network, true) {
                    return Err(CliError::Duplicate(flag.name()));
                }
            }
        }
    }
    if let Some(command) = command {
        if parsed != Args::default() {
            return Err(CliError::Alone(command.flag()));
        }
        return Ok(Parsed::Update(command));
    }
    let self_test = matches!(parsed.run, RunKind::SelfTest(_));
    if self_test && parsed.profile_dir.is_some() {
        return Err(CliError::Conflict(Flag::SelfTest.name(), Flag::ProfileDir.name()));
    }
    if parsed.network && !self_test {
        return Err(CliError::Requires(Flag::Network.name(), Flag::SelfTest.name()));
    }
    if parsed.phase.is_some() && !self_test {
        return Err(CliError::Requires(Flag::Phase.name(), Flag::SelfTest.name()));
    }
    Ok(Parsed::Run(parsed))
}

fn set_run(current: &mut RunKind, next: RunKind) -> Result<(), CliError> {
    let name = |kind: &RunKind| match kind {
        RunKind::Browse => "",
        RunKind::SelfTest(_) => Flag::SelfTest.name(),
        RunKind::UiSmoke(_) => Flag::UiSmoke.name(),
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

/// The resolved run configuration: which profile, what to open, and which mode.
#[derive(Debug)]
pub struct Config {
    /// Absolute. The profile root; the engine keeps its data in its `engine` folder.
    pub profile_dir: PathBuf,
    /// Its place in the profile list; none for a `--profile-dir` outside it. The scripted runs
    /// keep their list next to their profile, never in the user's.
    pub home: Option<Home>,
    pub start_urls: Vec<String>,
    pub mode: Mode,
    /// Unpacked extension folders, absolute.
    pub load_extensions: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Browse,
    SelfTest { out_dir: PathBuf, network: bool, phase: Option<String> },
    UiSmoke { out_dir: PathBuf },
}

impl Mode {
    /// Scripted runs never show modal message boxes and never activate a window.
    pub fn is_interactive(&self) -> bool {
        matches!(self, Self::Browse)
    }
}

#[derive(Debug)]
pub struct ConfigError(PathBuf, std::io::Error);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.0.display(), self.1)
    }
}

impl std::error::Error for ConfigError {}

/// What this launch runs.
#[derive(Debug)]
pub enum Start {
    Browse(Config),
    /// Several profiles, none running, and the picker switched on: the picker opens first.
    Picker(ProfilesDir),
}

impl Config {
    pub fn resolve(args: Args) -> Result<Start, ConfigError> {
        let profiles = ProfilesDir::standard();
        Self::resolve_with(args, &profiles, |has_urls| {
            profiles.sweep();
            profiles.startup(has_urls)
        })
    }

    /// `startup` decides a launch that names no profile, given whether it has URLs.
    fn resolve_with(args: Args, profiles: &ProfilesDir, startup: impl FnOnce(bool) -> Startup) -> Result<Start, ConfigError> {
        let (mode, profile_dir, home) = match args.run {
            RunKind::SelfTest(out_dir) => {
                let out_dir = absolute(&out_dir)?;
                let home = Home {
                    dir: ProfilesDir::at(out_dir.clone()),
                    id: ProfileId::parse("profile").expect("a plain directory name"),
                };
                let mode = Mode::SelfTest { out_dir, network: args.network, phase: args.phase };
                (mode, home.root(), Some(home))
            }
            RunKind::UiSmoke(out_dir) => {
                let out_dir = absolute(&out_dir)?;
                let profile = match args.profile_dir {
                    Some(dir) => absolute(&dir)?,
                    None => out_dir.join("profile"),
                };
                let home = scripted_home(&profile);
                (Mode::UiSmoke { out_dir }, profile, home)
            }
            RunKind::Browse => match args.profile_dir {
                Some(dir) => {
                    let profile = absolute(&dir)?;
                    let home = profiles.locate(&profile).map(|id| Home { dir: profiles.clone(), id });
                    (Mode::Browse, profile, home)
                }
                None => match startup(!args.urls.is_empty()) {
                    Startup::Open(id) => {
                        let home = Home { dir: profiles.clone(), id };
                        (Mode::Browse, home.root(), Some(home))
                    }
                    Startup::Picker => return Ok(Start::Picker(profiles.clone())),
                },
            },
        };
        let load_extensions = args.load_extensions.iter().map(|dir| absolute(dir)).collect::<Result<_, _>>()?;
        Ok(Start::Browse(Self { profile_dir, home, start_urls: args.urls, mode, load_extensions }))
    }

    /// The self-test replaces its profile on every run, so its log lives next to the report.
    pub fn log_file(&self) -> PathBuf {
        match &self.mode {
            Mode::SelfTest { out_dir, .. } => out_dir.join("vsesvit.log"),
            _ => self.profile_dir.join("vsesvit.log"),
        }
    }
}

/// A scripted run's profile list is the directory its profile is in.
fn scripted_home(profile: &Path) -> Option<Home> {
    let id = ProfileId::parse(profile.file_name()?.to_str()?)?;
    let dir = ProfilesDir::at(profile.parent()?.to_owned());
    Some(Home { dir, id })
}

fn absolute(path: &Path) -> Result<PathBuf, ConfigError> {
    std::path::absolute(path).map_err(|e| ConfigError(path.to_owned(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_in(shell: Shell, args: &[&str]) -> Result<Parsed, CliError> {
        parse(shell, args.iter().map(OsString::from))
    }

    fn run(args: &[&str]) -> Result<Parsed, CliError> {
        run_in(Shell::WinUi, args)
    }

    fn browse_in(shell: Shell, args: &[&str]) -> Args {
        match run_in(shell, args) {
            Ok(Parsed::Run(args)) => args,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn browse(args: &[&str]) -> Args {
        browse_in(Shell::WinUi, args)
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
        assert_eq!(run(&["--self-test", "out", "--profile-dir", "p"]), Err(CliError::Conflict("--self-test", "--profile-dir")));
    }

    #[test]
    fn modes_are_exclusive() {
        assert_eq!(run(&["--self-test", "a", "--ui-smoke", "b"]), Err(CliError::Conflict("--self-test", "--ui-smoke")));
        assert_eq!(run(&["--ui-smoke", "a", "--ui-smoke", "b"]), Err(CliError::Duplicate("--ui-smoke")));
    }

    #[test]
    fn missing_values_and_unknown_flags() {
        assert_eq!(run(&["--profile-dir"]), Err(CliError::MissingValue("--profile-dir")));
        assert_eq!(run(&["--self-test="]), Err(CliError::MissingValue("--self-test")));
        assert_eq!(run(&["--bogus"]), Err(CliError::UnknownOption("--bogus".into())));
        assert_eq!(run(&["--profile-dir", "a", "--profile-dir", "b"]), Err(CliError::Duplicate("--profile-dir")));
    }

    #[test]
    fn double_dash_ends_options() {
        let args = browse(&["--", "--not-a-flag"]);
        assert_eq!(args.urls, ["--not-a-flag"]);
    }

    #[test]
    fn help_and_version() {
        assert_eq!(run(&["x", "--help"]), Ok(Parsed::Help));
        assert_eq!(run(&["-h"]), Ok(Parsed::Help));
        assert_eq!(run(&["--version"]), Ok(Parsed::Version));
        assert_eq!(run(&["--profile-dir", "p", "--version"]), Ok(Parsed::Version));
    }

    #[test]
    fn update_commands_stand_alone() {
        assert_eq!(run(&["--check-for-updates"]), Ok(Parsed::Update(UpdateCommand::Check)));
        assert_eq!(run(&["--update"]), Ok(Parsed::Update(UpdateCommand::Install)));
        assert_eq!(run(&["--update", "https://a.test/"]), Err(CliError::Alone("--update")));
        assert_eq!(run(&["--profile-dir", "p", "--check-for-updates"]), Err(CliError::Alone("--check-for-updates")));
        assert_eq!(run(&["--check-for-updates", "--update"]), Err(CliError::Conflict("--check-for-updates", "--update")));
        assert_eq!(run(&["--update", "--update"]), Err(CliError::Duplicate("--update")));
        assert_eq!(run(&["--update=now"]), Err(CliError::UnexpectedValue("--update")));
        assert_eq!(run(&["--update", "--help"]), Ok(Parsed::Help));
    }

    #[test]
    fn network_only_with_self_test() {
        let args = browse(&["--self-test", "out", "--network"]);
        assert!(args.network);
        assert_eq!(args.run, RunKind::SelfTest(PathBuf::from("out")));
        assert_eq!(run(&["--network"]), Err(CliError::Requires("--network", "--self-test")));
        assert_eq!(run(&["--self-test", "o", "--network=yes"]), Err(CliError::UnexpectedValue("--network")));
    }

    #[test]
    fn load_extension_repeats() {
        let args = browse(&["--load-extension", "a", "--load-extension=b"]);
        assert_eq!(args.load_extensions, [PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    fn load_extension_parses_for_winui_and_gpui() {
        for shell in [Shell::WinUi, Shell::Gpui] {
            let args = browse_in(shell, &["--load-extension", "ext", "https://a.test/"]);
            assert_eq!(args.load_extensions, [PathBuf::from("ext")], "{shell:?}");
            assert_eq!(args.urls, ["https://a.test/"], "{shell:?}");
        }
    }

    #[test]
    fn ui_smoke_is_winuis_alone_and_gpui_refuses_it_with_the_usage_exit() {
        assert_eq!(browse_in(Shell::WinUi, &["--ui-smoke", "out"]).run, RunKind::UiSmoke(PathBuf::from("out")));
        for args in [&["--ui-smoke", "out"][..], &["--ui-smoke=out"], &["https://a.test/", "--ui-smoke"]] {
            let refused = run_in(Shell::Gpui, args);
            assert_eq!(refused, Err(CliError::NotAccepted("--ui-smoke")), "{args:?}");
            assert_eq!(refused.unwrap_err().to_string(), "--ui-smoke is not available in this build");
        }
        assert_eq!(EXIT_USAGE, 2);
    }

    #[test]
    fn phase_is_gpuis_and_needs_the_self_test() {
        let args = browse_in(Shell::Gpui, &["--self-test", "out", "--phase", "restart"]);
        assert_eq!(args.phase.as_deref(), Some("restart"));
        assert_eq!(args.run, RunKind::SelfTest(PathBuf::from("out")));
        assert_eq!(browse_in(Shell::Gpui, &["--phase=shutdown-stuck", "--self-test=o"]).phase.as_deref(), Some("shutdown-stuck"));
        assert_eq!(run_in(Shell::Gpui, &["--phase", "restart"]), Err(CliError::Requires("--phase", "--self-test")));
        assert_eq!(run_in(Shell::Gpui, &["--self-test", "o", "--phase"]), Err(CliError::MissingValue("--phase")));
        assert_eq!(run_in(Shell::Gpui, &["--self-test", "o", "--phase", "a", "--phase", "b"]), Err(CliError::Duplicate("--phase")));
        assert_eq!(run_in(Shell::WinUi, &["--self-test", "o", "--phase", "restart"]), Err(CliError::NotAccepted("--phase")));
    }

    #[test]
    fn the_flag_table_names_every_flag_once_and_usage_lists_what_the_shell_accepts() {
        for flag in Flag::ALL {
            assert_eq!(Flag::from_name(flag.name()), Some(flag));
            assert!(flag.usage().contains(flag.name()), "{flag:?}");
        }
        for shell in [Shell::WinUi, Shell::Gpui] {
            let text = usage(shell);
            for flag in Flag::ALL {
                assert_eq!(text.contains(flag.name()), flag.accepted_by(shell), "{shell:?} {flag:?}");
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn winuis_usage_is_the_one_it_printed_before_the_move() {
        let before = r"usage: vsesvit [URL...] [--profile-dir DIR] [--self-test OUT_DIR [--network]]
       vsesvit --check-for-updates | --update

  URL...                    open each URL in a tab (in the running instance, if there is one)
  --profile-dir DIR         use DIR as the profile
                            (default %LOCALAPPDATA%\Vsesvit\data\profiles\Default)
  --self-test OUT_DIR       run the end-to-end self-test with a fresh profile in OUT_DIR
  --network                 with --self-test: also install from the Chrome Web Store
  --load-extension DIR      install an unpacked extension folder into the profile
  --ui-smoke OUT_DIR        drive the UI, save screenshots and smoke.json to OUT_DIR, exit
  --check-for-updates       print one JSON line saying whether an update is available, exit
  --update                  download and start installing an available update (progress on
                            stderr), print the outcome as one JSON line, exit
  --version                 show the version
  -h, --help                show this help";
        assert_eq!(usage(Shell::WinUi), before);
    }

    fn abs(parts: &[&str]) -> PathBuf {
        let mut path = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" });
        path.extend(parts);
        path
    }

    fn args(run: RunKind, profile_dir: Option<PathBuf>) -> Args {
        Args {
            urls: vec!["https://example.test/".into()],
            profile_dir,
            run,
            load_extensions: vec![PathBuf::from("ext")],
            network: false,
            phase: None,
        }
    }

    fn profiles() -> ProfilesDir {
        ProfilesDir::at(abs(&["L", "Vsesvit", "data", "profiles"]))
    }

    fn resolve(args: Args, startup: Startup) -> Config {
        match Config::resolve_with(args, &profiles(), |_| startup).unwrap() {
            Start::Browse(config) => config,
            Start::Picker(_) => panic!("the picker instead of a profile"),
        }
    }

    #[test]
    fn a_launch_naming_no_profile_opens_what_the_profile_list_chooses() {
        let config = resolve(args(RunKind::Browse, None), Startup::Open(ProfileId::default_profile()));
        assert_eq!(config.profile_dir, abs(&["L", "Vsesvit", "data", "profiles", "Default"]));
        assert_eq!(config.log_file(), abs(&["L", "Vsesvit", "data", "profiles", "Default", "vsesvit.log"]));
        assert_eq!(config.start_urls, ["https://example.test/"]);
        assert!(config.load_extensions[0].is_absolute());
        assert!(config.mode.is_interactive());
        assert_eq!(config.home, Some(Home { dir: profiles(), id: ProfileId::default_profile() }));

        let mut no_urls = args(RunKind::Browse, None);
        no_urls.urls.clear();
        let picker = Config::resolve_with(no_urls, &profiles(), |has_urls| {
            assert!(!has_urls);
            Startup::Picker
        });
        assert!(matches!(picker, Ok(Start::Picker(dir)) if dir == profiles()));
    }

    #[test]
    fn profile_dir_is_made_absolute() {
        let config = resolve(args(RunKind::Browse, Some(PathBuf::from("rel"))), Startup::Picker);
        assert!(config.profile_dir.is_absolute());
        assert!(config.profile_dir.ends_with("rel"));
        assert_eq!(config.home, None, "outside the profile list");
        let listed = resolve(
            args(RunKind::Browse, Some(abs(&["L", "Vsesvit", "data", "profiles", "Profile 1"]))),
            Startup::Picker,
        );
        assert_eq!(listed.home.map(|home| home.id), ProfileId::parse("Profile 1"));
    }

    #[test]
    fn self_test_uses_a_profile_inside_out_dir() {
        let mut a = args(RunKind::SelfTest(abs(&["out"])), None);
        a.network = true;
        a.phase = Some("restart".into());
        let config = resolve(a, Startup::Picker);
        assert_eq!(config.profile_dir, abs(&["out", "profile"]));
        assert_eq!(config.home.as_ref().map(|home| &home.dir), Some(&ProfilesDir::at(abs(&["out"]))));
        assert_eq!(config.log_file(), abs(&["out", "vsesvit.log"]));
        assert_eq!(config.mode, Mode::SelfTest { out_dir: abs(&["out"]), network: true, phase: Some("restart".into()) });
        assert!(!config.mode.is_interactive());
    }

    #[test]
    fn ui_smoke_keeps_its_profile_list_next_to_its_profile() {
        let config = resolve(args(RunKind::UiSmoke(abs(&["smoke"])), None), Startup::Picker);
        assert_eq!(config.profile_dir, abs(&["smoke", "profile"]));
        assert_eq!(config.home, Some(Home { dir: ProfilesDir::at(abs(&["smoke"])), id: ProfileId::parse("profile").unwrap() }));
        assert_eq!(config.log_file(), abs(&["smoke", "profile", "vsesvit.log"]));
        assert!(!config.mode.is_interactive());
    }
}
