//! Owned by W1.4.
//! Boot steps 4 to 6 (design section 3.1): which profile this launch runs, opening it, and the
//! single-instance hand-off. Taking core's lock first means a second launch of a profile forwards its
//! command line to the owner before any engine starts. The picker branch runs GPUI with the picker
//! alone: no engine, no lock.

use std::process::ExitCode;

use vsesvit_core::cli::{Args, Config, Mode, Start};
use vsesvit_core::{OpenError, OpenOptions, Profile};

use super::instance::{self, Endpoint, Request};
use super::logging::{self, FileMode};
use crate::platform::fatal_message;

/// Step 4: the run's config, or the picker's exit code.
pub fn resolve(args: Args) -> Result<Config, ExitCode> {
    match Config::resolve(args) {
        Ok(Start::Browse(config)) => Ok(config),
        Ok(Start::Picker(profiles)) => {
            logging::open_file(&profiles.path().join("picker.log"), FileMode::Truncate);
            Err(crate::profiles::picker::run(profiles))
        }
        Err(e) => {
            fatal_message(&format!("vsesvit: {e}"));
            Err(ExitCode::FAILURE)
        }
    }
}

/// Step 5: opens the profile, or hands this launch to the process that has it open and returns the
/// exit code that process answered.
pub fn open(config: &Config, chrome_version: &str, request: &Request) -> Result<Profile, ExitCode> {
    let options = OpenOptions { chrome_version: chrome_version.to_owned(), ..OpenOptions::default() };
    match Profile::open(&config.profile_dir, options) {
        Ok(profile) => {
            logging::open_file(&config.log_file(), FileMode::Truncate);
            log::info!("vsesvit {} (GPUI), profile {}", env!("CARGO_PKG_VERSION"), config.profile_dir.display());
            if let Some(home) = &config.home
                && home.dir.load().is_removed(&home.id)
            {
                drop(profile);
                home.dir.sweep();
                fatal_message(&format!("vsesvit: the profile at {} was removed", config.profile_dir.display()));
                return Err(ExitCode::FAILURE);
            }
            Ok(profile)
        }
        Err(OpenError::Locked) if config.mode == Mode::Browse => {
            logging::open_file(&config.log_file(), FileMode::Append);
            #[cfg(windows)]
            crate::platform::windows::console::attach_parent_console();
            match instance::forward(&Endpoint::of_profile(&config.profile_dir), request, &mut std::io::stderr()) {
                Ok(status) => {
                    log::info!("the profile is open in another process; this launch went there (status {status})");
                    Err(ExitCode::from(status))
                }
                Err(e) => {
                    log::error!("forwarding to the process that has the profile open: {e}");
                    fatal_message(instance::UNREACHABLE);
                    Err(ExitCode::FAILURE)
                }
            }
        }
        Err(e) => {
            logging::open_file(&config.log_file(), FileMode::Append);
            fatal_message(&format!("vsesvit: cannot open the profile at {}: {e}", config.profile_dir.display()));
            Err(ExitCode::FAILURE)
        }
    }
}

/// Step 6: binds the instance channel, without accepting yet. The self-test has none. A channel that
/// cannot be bound costs only the forwarding of later launches, so the run goes on.
pub fn bind(config: &Config) -> Option<instance::Listener> {
    if config.mode != Mode::Browse {
        return None;
    }
    let endpoint = Endpoint::of_profile(&config.profile_dir);
    match instance::Listener::bind(&endpoint) {
        Ok(listener) => Some(listener),
        Err(e) => {
            log::error!("instance: cannot bind {endpoint:?}, so later launches of this profile cannot reach this process: {e}");
            None
        }
    }
}
