//! `--check-for-updates` and `--update`: the updater without a window or a display, for scripts
//! and the packaging end-to-end test. Both print what `vsesvit_update::cli` documents. Neither
//! opens a profile, so neither reads `updates.automatic`, and both follow the channel this build
//! was released on rather than `updates.channel`.

use std::process::ExitCode;

use vsesvit_core::prefs::UpdateChannel;
use vsesvit_update::{Installation, cli};

use super::{current_version, download_dir};

pub(crate) fn check() -> ExitCode {
    cli::print(cli::check(&Installation::detect(), &current_version(), UpdateChannel::of_build().name()))
}

/// Check, download (progress on stderr) and install.
pub(crate) fn update() -> ExitCode {
    cli::print(cli::update(&Installation::detect(), &current_version(), UpdateChannel::of_build().name(), &download_dir()))
}
