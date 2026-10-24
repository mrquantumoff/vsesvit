//! Owned by W2.6.

use gpui::App;
use vsesvit_engine::{PromptEnd, PromptId, PromptRequest};

/// A JS dialog, beforeunload, file chooser or HTTP auth prompt (design section 1.5), from the bridge's
/// router. Until W2.6 lands nothing shows, and the prompt stays open until the engine ends it.
pub fn show(_id: PromptId, _request: PromptRequest, _cx: &mut App) {}

/// The engine ended one of these prompts before an answer.
pub fn ended(_id: PromptId, _why: PromptEnd, _cx: &mut App) {}
