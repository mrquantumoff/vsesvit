//! Linux shell of the Vsesvit browser: GTK4 + libadwaita + WebKitGTK 6.0, on top of
//! `vsesvit-core` (profile store) and `vsesvit-webext` (extension runtime).
//! Compiles to nothing on other targets.
#![cfg(target_os = "linux")]

mod address_bar;
mod app;
mod bookmarks_bar;
mod browser;
mod cli;
mod closed_tabs;
mod dialogs;
mod downloads;
mod engine;
mod error_page;
mod extensions;
mod find_bar;
mod location;
mod omnibox;
mod permissions;
mod profile;
pub mod screenshot;
#[cfg(feature = "self-test")]
mod self_test;
mod session;
mod tab;
#[cfg(test)]
mod test_support;
mod updates;
mod window;
mod zoom;

use std::ffi::OsString;
use std::process::ExitCode;

use cli::Command;

/// The application id for the default profile. Other profiles derive theirs from it.
pub const APP_ID: &str = "dev.mrquantumoff.vsesvit";

static LOGGER: glib::GlibLogger = glib::GlibLogger::new(
    glib::GlibLoggerFormat::Plain,
    glib::GlibLoggerDomain::CrateTarget,
);

/// `vsesvit [URL...] [--profile-dir DIR] | --self-test OUT_DIR [--network] | --check-for-updates | --update`
pub fn run() -> ExitCode {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    let args: Vec<OsString> = std::env::args_os().collect();
    let command = match cli::parse(args.iter().skip(1).cloned()) {
        Ok(command) => command,
        Err(e) => {
            eprintln!("vsesvit: {e}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("vsesvit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::CheckForUpdates => updates::headless::check(),
        Command::Update => updates::headless::update(),
        #[cfg(feature = "self-test")]
        Command::SelfTest { out_dir, network } => self_test::run(&out_dir, network),
        #[cfg(not(feature = "self-test"))]
        Command::SelfTest { .. } => {
            eprintln!("vsesvit: this build was made without the `self-test` feature");
            ExitCode::FAILURE
        }
        Command::Browse { profile_dir, .. } => app::run(profile_dir, &args),
    }
}
