//! Owned by W0.1.
//! The GPUI browser shell (docs/design/linux-cef/design.md). It names no CEF type: the engine arrives as
//! a `vsesvit_engine::EngineFactory` from `main`, so its tests run with `FakeEngine` and no libcef.
//! Every module is declared here, so no later unit edits this file; each unit fills its own files.

use std::process::ExitCode;

pub mod app;
pub mod bookmarks;
pub mod boot;
pub mod context_menu;
pub mod dialogs;
pub mod downloads;
pub mod engine_bridge;
pub mod extensions;
pub mod favicons;
pub mod find_bar;
pub mod omnibox;
pub mod overlay;
pub mod platform;
pub mod privacy;
pub mod profiles;
pub mod save_page;
#[cfg(feature = "self-test")]
pub mod self_test;
pub mod state;
pub mod sync;
pub mod tab;
pub mod tabs_ui;
#[cfg(test)]
mod test_support;
pub mod theme;
pub mod toolbar;
pub mod updates;
pub mod util;
pub mod window;

/// The browser process, from boot step 1 on (design section 3.1). `main` has already returned from
/// any CEF sub-process (step 0).
pub fn run(factory: &dyn vsesvit_engine::EngineFactory) -> ExitCode {
    boot::run(factory)
}
