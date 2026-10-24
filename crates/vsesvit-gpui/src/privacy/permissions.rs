//! Owned by W3.1.

use gpui::App;
use vsesvit_engine::{PermissionRequest, PromptEnd, PromptId};

/// A site asks for permissions (`PromptRequest::Permission`), from the bridge's router. Until W3.1
/// lands nothing shows, and the prompt stays open until the engine ends it.
pub fn request(_id: PromptId, _request: PermissionRequest, _cx: &mut App) {}

/// The engine ended a permission prompt before an answer (`Withdrawn`, `Navigated`, ...).
pub fn ended(_id: PromptId, _why: PromptEnd, _cx: &mut App) {}
