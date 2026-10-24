//! Owned by W1.3.
//! The CEF glue: implements the `vsesvit_engine` contract over CEF 154 (design sections 1.2 and 3). It
//! never names a GPUI type. Every module is declared here, so the units that fill the handler files
//! never edit this one.

use std::process::ExitCode;

pub mod app;
pub mod client;
pub mod contexts;
pub mod devtools;
pub mod driver;
pub mod engine;
pub mod events;
pub mod ext;
pub mod handlers {
    pub mod auth;
    pub mod command;
    pub mod context_menu;
    pub mod cookie_access;
    pub mod dialog;
    pub mod display;
    pub mod download;
    pub mod favicon;
    pub mod find;
    pub mod focus;
    pub mod jsdialog;
    pub mod keyboard;
    pub mod life_span;
    pub mod load;
    pub mod permission;
    pub mod request;
    pub mod resource_request;
}
pub mod hidden;
pub mod idc;
pub mod lifecycle;
pub mod native;
pub mod prefs;
pub mod prompts;
pub mod registry;
pub mod schemes;
pub mod settings;
pub mod site_data;
pub mod strings;
pub mod teardown;

/// The Windows sandbox information `RunWinMain` receives (design section 1.1); none in a dev build and
/// off Windows.
pub struct Sandbox {
    _private: (),
}

impl Sandbox {
    /// No sandbox information: Linux, and Windows `cargo run`.
    pub fn none() -> Sandbox {
        Sandbox { _private: () }
    }
}

/// Boot step 0: `api_hash`, then `execute_process`; `Some(code)` in a CEF sub-process. Not built yet:
/// nothing starts a sub-process, so every process is the browser process.
pub fn run_subprocess(_sandbox: &Sandbox) -> Option<ExitCode> {
    None
}

/// Builds the CEF engine for the browser process.
pub struct CefEngineFactory {
    _sandbox: Sandbox,
}

impl CefEngineFactory {
    pub fn new(sandbox: Sandbox) -> CefEngineFactory {
        CefEngineFactory { _sandbox: sandbox }
    }
}

impl vsesvit_engine::EngineFactory for CefEngineFactory {}
