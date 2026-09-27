//! The resolved run configuration. Until vsesvit-core owns the profile, the profile directory
//! is `--profile-dir` or `%LOCALAPPDATA%\Vsesvit\dev`, and WebView2 keeps its data in the
//! `engine` subfolder, which is where core's profile layout puts it.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::cli::{Args, RunKind};

#[derive(Debug)]
pub(crate) struct Config {
    /// Absolute.
    pub profile_dir: PathBuf,
    pub start_urls: Vec<String>,
    pub mode: Mode,
    /// Unpacked extension folders loaded into the engine for this session only.
    pub load_extensions: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Browse,
    SelfTest { out_dir: PathBuf },
    UiSmoke { out_dir: PathBuf },
}

impl Mode {
    /// Scripted runs never show modal message boxes and never activate the window.
    pub fn is_interactive(&self) -> bool {
        matches!(self, Self::Browse)
    }
}

#[derive(Debug)]
pub(crate) enum ConfigError {
    NoLocalAppData,
    Path(PathBuf, std::io::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLocalAppData => {
                f.write_str("LOCALAPPDATA is not set; pass --profile-dir to choose a profile")
            }
            Self::Path(path, e) => write!(f, "{}: {e}", path.display()),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn resolve(args: Args) -> Result<Self, ConfigError> {
        Self::resolve_with(args, std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
    }

    fn resolve_with(args: Args, local_app_data: Option<PathBuf>) -> Result<Self, ConfigError> {
        let (mode, profile_dir) = match args.run {
            RunKind::SelfTest(out_dir) => {
                let out_dir = absolute(&out_dir)?;
                let profile = out_dir.join("profile");
                (Mode::SelfTest { out_dir }, profile)
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
                    None => local_app_data
                        .ok_or(ConfigError::NoLocalAppData)?
                        .join("Vsesvit")
                        .join("dev"),
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

    /// The WebView2 user data folder.
    pub fn engine_dir(&self) -> PathBuf {
        self.profile_dir.join("engine")
    }
}

fn absolute(path: &Path) -> Result<PathBuf, ConfigError> {
    std::path::absolute(path).map_err(|e| ConfigError::Path(path.to_owned(), e))
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
        }
    }

    #[test]
    fn default_profile_is_under_local_app_data() {
        let config =
            Config::resolve_with(args(RunKind::Browse, None), Some(PathBuf::from(r"C:\L")))
                .unwrap();
        assert_eq!(config.profile_dir, PathBuf::from(r"C:\L\Vsesvit\dev"));
        assert_eq!(
            config.engine_dir(),
            PathBuf::from(r"C:\L\Vsesvit\dev\engine")
        );
        assert_eq!(config.start_urls, ["https://example.test/"]);
        assert!(config.load_extensions[0].is_absolute());
        assert!(config.mode.is_interactive());
    }

    #[test]
    fn profile_dir_is_made_absolute() {
        let config = Config::resolve_with(args(RunKind::Browse, Some("rel")), None).unwrap();
        assert!(config.profile_dir.is_absolute());
        assert!(config.profile_dir.ends_with("rel"));
    }

    #[test]
    fn self_test_uses_a_profile_inside_out_dir() {
        let run = RunKind::SelfTest(PathBuf::from(r"C:\out"));
        let config = Config::resolve_with(args(run, None), None).unwrap();
        assert_eq!(config.profile_dir, PathBuf::from(r"C:\out\profile"));
        assert_eq!(
            config.mode,
            Mode::SelfTest {
                out_dir: PathBuf::from(r"C:\out")
            }
        );
        assert!(!config.mode.is_interactive());
    }

    #[test]
    fn missing_local_app_data_is_an_error() {
        assert!(matches!(
            Config::resolve_with(args(RunKind::Browse, None), None),
            Err(ConfigError::NoLocalAppData)
        ));
    }
}
