//! The resolved run configuration: which profile, what to open, and which mode.

use std::fmt;
use std::path::{Path, PathBuf};

use vsesvit_core::profiles::{Home, ProfileId, ProfilesDir, Startup};

use crate::cli::{Args, RunKind};

#[derive(Debug)]
pub(crate) struct Config {
    /// Absolute. The vsesvit-core profile root; WebView2 keeps its data in its `engine` folder.
    pub profile_dir: PathBuf,
    /// Its place in the profile list; none for a `--profile-dir` outside it. The scripted runs
    /// keep their list next to their profile, never in the user's.
    pub home: Option<Home>,
    pub start_urls: Vec<String>,
    pub mode: Mode,
    /// Unpacked extension folders installed through the normal pipeline at startup.
    pub load_extensions: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Browse,
    SelfTest { out_dir: PathBuf, network: bool },
    UiSmoke { out_dir: PathBuf },
}

impl Mode {
    /// Scripted runs never show modal message boxes and never activate a window.
    pub fn is_interactive(&self) -> bool {
        matches!(self, Self::Browse)
    }
}

#[derive(Debug)]
pub(crate) struct ConfigError(PathBuf, std::io::Error);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.0.display(), self.1)
    }
}

impl std::error::Error for ConfigError {}

/// What this launch runs.
#[derive(Debug)]
pub(crate) enum Start {
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
    fn resolve_with(
        args: Args,
        profiles: &ProfilesDir,
        startup: impl FnOnce(bool) -> Startup,
    ) -> Result<Start, ConfigError> {
        let (mode, profile_dir, home) = match args.run {
            RunKind::SelfTest(out_dir) => {
                let out_dir = absolute(&out_dir)?;
                let home = Home {
                    dir: ProfilesDir::at(out_dir.clone()),
                    id: ProfileId::parse("profile").expect("a plain directory name"),
                };
                let mode = Mode::SelfTest {
                    out_dir,
                    network: args.network,
                };
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
                    let home = profiles.locate(&profile).map(|id| Home {
                        dir: profiles.clone(),
                        id,
                    });
                    (Mode::Browse, profile, home)
                }
                None => match startup(!args.urls.is_empty()) {
                    Startup::Open(id) => {
                        let home = Home {
                            dir: profiles.clone(),
                            id,
                        };
                        (Mode::Browse, home.root(), Some(home))
                    }
                    Startup::Picker => return Ok(Start::Picker(profiles.clone())),
                },
            },
        };
        let load_extensions = args
            .load_extensions
            .iter()
            .map(|dir| absolute(dir))
            .collect::<Result<_, _>>()?;
        Ok(Start::Browse(Self {
            profile_dir,
            home,
            start_urls: args.urls,
            mode,
            load_extensions,
        }))
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

    fn args(run: RunKind, profile_dir: Option<&str>) -> Args {
        Args {
            urls: vec!["https://example.test/".into()],
            profile_dir: profile_dir.map(PathBuf::from),
            run,
            load_extensions: vec![PathBuf::from("ext")],
            network: false,
        }
    }

    fn profiles() -> ProfilesDir {
        ProfilesDir::at(PathBuf::from(r"C:\L\Vsesvit\data\profiles"))
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
        assert_eq!(
            config.profile_dir,
            PathBuf::from(r"C:\L\Vsesvit\data\profiles\Default")
        );
        assert_eq!(
            config.log_file(),
            PathBuf::from(r"C:\L\Vsesvit\data\profiles\Default\vsesvit.log")
        );
        assert_eq!(config.start_urls, ["https://example.test/"]);
        assert!(config.load_extensions[0].is_absolute());
        assert!(config.mode.is_interactive());
        assert_eq!(
            config.home,
            Some(Home {
                dir: profiles(),
                id: ProfileId::default_profile()
            })
        );

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
        let config = resolve(args(RunKind::Browse, Some("rel")), Startup::Picker);
        assert!(config.profile_dir.is_absolute());
        assert!(config.profile_dir.ends_with("rel"));
        assert_eq!(config.home, None, "outside the profile list");
        let listed = resolve(
            args(
                RunKind::Browse,
                Some(r"C:\L\Vsesvit\data\profiles\Profile 1"),
            ),
            Startup::Picker,
        );
        assert_eq!(
            listed.home.map(|home| home.id),
            ProfileId::parse("Profile 1")
        );
    }

    #[test]
    fn self_test_uses_a_profile_inside_out_dir() {
        let mut a = args(RunKind::SelfTest(PathBuf::from(r"C:\out")), None);
        a.network = true;
        let config = resolve(a, Startup::Picker);
        assert_eq!(config.profile_dir, PathBuf::from(r"C:\out\profile"));
        assert_eq!(
            config.home.as_ref().map(|home| &home.dir),
            Some(&ProfilesDir::at(PathBuf::from(r"C:\out")))
        );
        assert_eq!(config.log_file(), PathBuf::from(r"C:\out\vsesvit.log"));
        assert_eq!(
            config.mode,
            Mode::SelfTest {
                out_dir: PathBuf::from(r"C:\out"),
                network: true,
            }
        );
        assert!(!config.mode.is_interactive());
    }
}
