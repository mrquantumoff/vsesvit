//! Owned by W4.3.

use gpui::App;
use vsesvit_engine::{Origin, PageChannel, TabId};

/// A page message (design section 1.5), from the bridge's router: the store shim's channel. Until W4.3
/// lands it is dropped.
pub fn page_message(_tab: TabId, _origin: Origin, _main_frame: bool, _channel: PageChannel, _payload: String, _cx: &mut App) {}
