//! Owned by W1.4.
//! Boot step 2 (design section 3.1): the command line goes through `vsesvit_core::cli` with the GPUI
//! shell's flag table, which adds `--phase` and refuses `--ui-smoke`. Every error exits with
//! `EXIT_USAGE` (2) after the usage text.

use std::ffi::OsString;

use vsesvit_core::cli::{self, CliError, Parsed, Shell, UpdateCommand};

pub use vsesvit_core::cli::{Args, EXIT_USAGE, RunKind};

/// What this launch is, from its arguments alone.
#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    /// A browser run, or the self-test.
    Run(Args),
    /// Step 3: no profile, no display, no engine.
    Headless(Headless),
}

/// The commands that print and exit.
#[derive(Debug, PartialEq, Eq)]
pub enum Headless {
    Help,
    Version,
    Update(UpdateCommand),
    /// The arguments do not parse.
    Usage(CliError),
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Launch {
    match cli::parse(Shell::Gpui, args) {
        Ok(Parsed::Run(args)) => Launch::Run(args),
        Ok(Parsed::Help) => Launch::Headless(Headless::Help),
        Ok(Parsed::Version) => Launch::Headless(Headless::Version),
        Ok(Parsed::Update(command)) => Launch::Headless(Headless::Update(command)),
        Err(e) => Launch::Headless(Headless::Usage(e)),
    }
}

pub fn usage() -> String {
    cli::usage(Shell::Gpui)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(args: &[&str]) -> Launch {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn the_gpui_table_takes_phase_and_refuses_ui_smoke() {
        assert!(matches!(launch(&["--self-test", "out", "--phase", "restart"]), Launch::Run(a) if a.phase.as_deref() == Some("restart")));
        assert_eq!(launch(&["--ui-smoke", "out"]), Launch::Headless(Headless::Usage(CliError::NotAccepted("--ui-smoke"))));
        assert!(matches!(launch(&["--load-extension", "dir"]), Launch::Run(a) if a.load_extensions.len() == 1));
        assert_eq!(launch(&["--version"]), Launch::Headless(Headless::Version));
        assert_eq!(launch(&["--check-for-updates"]), Launch::Headless(Headless::Update(UpdateCommand::Check)));
        assert!(usage().contains("--phase NAME") && !usage().contains("--ui-smoke"));
    }
}
