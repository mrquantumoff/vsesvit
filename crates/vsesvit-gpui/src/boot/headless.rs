//! Owned by W1.4.
//! Boot step 3 (design section 3.1): `--help`, `--version`, `--check-for-updates`, `--update` and a
//! command line that does not parse. None opens a profile, a display or the engine. A Windows release
//! build is a GUI-subsystem program, so it first joins the console of the shell that started it.
//! The update commands print what `vsesvit_update::cli` documents and follow the channel this build
//! was released on, since they open no profile to read `updates.channel` from.

use std::path::PathBuf;
use std::process::ExitCode;

use vsesvit_core::cli::UpdateCommand;
use vsesvit_core::prefs::UpdateChannel;
use vsesvit_update::{Installation, cli};

use super::cli::{EXIT_USAGE, Headless, usage};

pub fn run(command: Headless) -> ExitCode {
    #[cfg(windows)]
    crate::platform::windows::console::attach_parent_console();
    match command {
        Headless::Help => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Headless::Version => {
            println!("vsesvit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Headless::Update(command) => update(command),
        Headless::Usage(e) => {
            eprintln!("vsesvit: {e}\n\n{}", usage());
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn update(command: UpdateCommand) -> ExitCode {
    let installation = Installation::detect();
    let channel = UpdateChannel::of_build().name();
    let current = current_version();
    cli::print(match command {
        UpdateCommand::Check => cli::check(&installation, &current, channel).map_err(|e| e.to_string()),
        UpdateCommand::Install => {
            download_dir().and_then(|dir| cli::update(&installation, &current, channel, &dir).map_err(|e| e.to_string()))
        }
    })
}

pub fn current_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package versions are semver")
}

/// Where updates download: `$XDG_CACHE_HOME/vsesvit/updates` (or `~/.cache/…`) on Linux,
/// `%LOCALAPPDATA%\Vsesvit\updates` on Windows, as the GTK and WinUI shells.
pub fn download_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    return std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("Vsesvit").join("updates"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_owned());
    #[cfg(not(windows))]
    {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .ok_or_else(|| "neither XDG_CACHE_HOME nor HOME is set".to_owned())?;
        Ok(cache.join("vsesvit").join("updates"))
    }
}
