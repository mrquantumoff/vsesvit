//! The resolved run configuration: which profile, what to open, and which mode.

use std::fmt;
use std::path::{Path, PathBuf};

use vsesvit_core::Profile;

use crate::cli::{Args, RunKind};

#[derive(Debug)]
pub(crate) struct Config {
    /// Absolute. The vsesvit-core profile root; WebView2 keeps its data in its `engine` folder.
    pub profile_dir: PathBuf,
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

impl Config {
    pub fn resolve(args: Args) -> Result<Self, ConfigError> {
        Self::resolve_with(args, || Profile::default_root("Default"))
    }

    fn resolve_with(
        args: Args,
        default_root: impl FnOnce() -> PathBuf,
    ) -> Result<Self, ConfigError> {
        let (mode, profile_dir) = match args.run {
            RunKind::SelfTest(out_dir) => {
                let out_dir = absolute(&out_dir)?;
                let profile = out_dir.join("profile");
                let mode = Mode::SelfTest {
                    out_dir,
                    network: args.network,
                };
                (mode, profile)
            }
            RunKind::UiSmoke(out_dir) => {
                let out_dir = absolute(&out_dir)?;
                let profile = match args.profile_dir {
                    Some(dir) => absolute(&dir)?,
                    None => out_dir.join("profile"),
                };
                (Mode::UiSmoke { out_dir }, profile)
            }
            RunKind::Browse => {
                let profile = match args.profile_dir {
                    Some(dir) => absolute(&dir)?,
                    None => default_root(),
                };
                (Mode::Browse, profile)
            }
        };
        let load_extensions = args
            .load_extensions
            .iter()
            .map(|dir| absolute(dir))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            profile_dir,
            start_urls: args.urls,
            mode,
            load_extensions,
        })
    }

    /// The self-test replaces its profile on every run, so its log lives next to the report.
    pub fn log_file(&self) -> PathBuf {
        match &self.mode {
            Mode::SelfTest { out_dir, .. } => out_dir.join("vsesvit.log"),
            _ => self.profile_dir.join("vsesvit.log"),
        }
    }
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

    #[test]
    fn default_profile_is_cores_default_root() {
        let config = Config::resolve_with(args(RunKind::Browse, None), || {
            PathBuf::from(r"C:\L\Vsesvit\data\profiles\Default")
        })
        .unwrap();
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
    }

    #[test]
    fn profile_dir_is_made_absolute() {
        let config =
            Config::resolve_with(args(RunKind::Browse, Some("rel")), || unreachable!()).unwrap();
        assert!(config.profile_dir.is_absolute());
        assert!(config.profile_dir.ends_with("rel"));
    }

    #[test]
    fn self_test_uses_a_profile_inside_out_dir() {
        let mut a = args(RunKind::SelfTest(PathBuf::from(r"C:\out")), None);
        a.network = true;
        let config = Config::resolve_with(a, || unreachable!()).unwrap();
        assert_eq!(config.profile_dir, PathBuf::from(r"C:\out\profile"));
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
