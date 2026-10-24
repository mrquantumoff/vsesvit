//! Owned by W0.2.
//! The synchronous questions the engine must answer inside its own callbacks (UA section 4.1 rule 4,
//! design section 1.5 Hooks).

use crate::ids::NativeHandle;
use crate::types::{NavigationDecision, NavigationRequest, PopupDecision, PopupRequest};

/// Called synchronously on the main thread from inside engine callbacks, possibly inside a GPUI update.
/// Implementations read only snapshots the shell publishes; they never touch `gpui::App` and never borrow
/// the core `Profile`.
pub trait ShellHooks {
    /// Whether a navigation may proceed.
    fn navigation_policy(&self, request: &NavigationRequest) -> NavigationDecision;
    /// Whether a page may open a new browser, and where.
    fn popup_policy(&self, request: &PopupRequest) -> PopupDecision;
    /// Hides a window the engine opened on its own and moves it into the hidden host (design section 5.7).
    /// An error makes the engine close that browser at once.
    fn neutralize_foreign(&self, window: NativeHandle) -> Result<(), NeutralizeFailed>;
}

/// Why `ShellHooks::neutralize_foreign` failed. The shell maps its platform error into it, so the contract
/// names no platform type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeutralizeFailed {
    /// What went wrong, for the log.
    pub reason: String,
}
