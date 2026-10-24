//! Owned by W0.2.
//! Building the engine in the browser process (UA section 4.2 `factory.rs`, design sections 3.1 to 3.3).

use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use futures_channel::mpsc::UnboundedReceiver;

use crate::command::Engine;
use crate::event::BrowserEvent;

/// What the engine needs to start (design sections 3.2 and 3.3). The engine derives the rest: the global
/// context's cache path, the helper's path, the sandbox flag.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    /// The engine's user-data root, `<profile root>/engine/cef`.
    pub root_cache_path: PathBuf,
    /// The engine's log file; `None` keeps the engine's default.
    pub log_file: Option<PathBuf>,
    /// Verbose engine logging (`VSESVIT_CEF_LOG=verbose`) instead of warnings only.
    pub verbose_log: bool,
    /// The `scrolling.smooth` pref; restart-only.
    pub smooth_scrolling: bool,
    /// The `system.hardware_acceleration` pref; restart-only.
    pub hardware_acceleration: bool,
    /// The product token of the user agent.
    pub user_agent_product: String,
    /// Keep session cookies across restarts (the "continue where you left off" startup pref).
    pub persist_session_cookies: bool,
    /// The page background shown before the first paint, `0xAARRGGBB`.
    pub background_color: u32,
    /// The UI locale, e.g. `en-US`.
    pub locale: String,
    /// Linux: the one scale for GPUI and Chromium (`--force-device-scale-factor`); `None` elsewhere.
    pub device_scale_factor: Option<f32>,
    /// Linux: the persisted password store (`--password-store`); `None` elsewhere.
    pub password_store: Option<PasswordStore>,
    /// Unpacked extensions to load for this session (`--load-extension`).
    pub load_extensions: Vec<PathBuf>,
    /// A self-test run: fake capture devices and the other self-test switches.
    pub scripted: bool,
    /// More switches, verbatim.
    pub extra_switches: Vec<String>,
}

/// Chromium's password store on Linux (design section 3.2, Q8).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PasswordStore {
    /// `basic`.
    Basic,
    /// `gnome-libsecret`.
    GnomeLibsecret,
}

/// Drives the engine's message loop from the shell's pump task (design section 3.4).
pub trait EngineDriver {
    /// Wake requests: the delay until the next `do_work`, sent from any thread.
    fn schedule(&mut self) -> UnboundedReceiver<Duration>;
    /// One pump step. Called from one task, outside any GPUI update.
    fn do_work(&self);
    /// Shutdown step 8: stop helper threads, drain, drop every engine reference, shut the engine down.
    /// Only after `BrowserEvent::AllBrowsersClosed`.
    fn shutdown(self: Box<Self>);
}

/// A started engine.
pub struct EngineParts {
    /// The engine.
    pub engine: Rc<dyn Engine>,
    /// Its driver.
    pub driver: Box<dyn EngineDriver>,
    /// Its events.
    pub events: UnboundedReceiver<BrowserEvent>,
}

/// Why `EngineFactory::initialize` did not start an engine.
#[derive(Debug)]
pub enum EngineInitError {
    /// The engine asked the process to exit with this code; make no further engine call.
    EarlyExit(ExitCode),
    /// It failed; the message is shown to the user.
    Failed(String),
}

/// Builds the engine in the browser process.
pub trait EngineFactory {
    /// The Chromium version, for core's `OpenOptions.chrome_version`, before the engine starts.
    fn chromium_version(&self) -> &'static str;
    /// Starts the engine (boot step 9). Called once, with no other thread started yet.
    fn initialize(&self, config: EngineConfig) -> Result<EngineParts, EngineInitError>;
}
