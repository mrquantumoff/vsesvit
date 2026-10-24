//! Owned by W2.5.

use gpui::App;
use vsesvit_engine::{ContextMenuParams, MenuModel, PagePoint, PromptEnd, PromptId, TabId};

/// A page's context menu (design section 1.5, `PromptRequest::ContextMenu`), from the bridge's router.
/// Until W2.5 lands no menu shows, and the engine ends the prompt (`MenuDismissed`, `Navigated`, ...).
pub fn show(_id: PromptId, _tab: TabId, _at: PagePoint, _params: ContextMenuParams, _model: MenuModel, _cx: &mut App) {}

/// The engine ended a menu before an answer.
pub fn ended(_id: PromptId, _why: PromptEnd, _cx: &mut App) {}
