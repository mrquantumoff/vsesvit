//! Owned by W4.3.

use gpui::App;
use vsesvit_engine::extension::{ExtensionEvent, ForeignWindowEvent};
use vsesvit_engine::{Origin, PageChannel, TabId};

/// A page message (design section 1.5), from the bridge's router: the store shim's channel. Until W4.3
/// lands it is dropped.
pub fn page_message(_tab: TabId, _origin: Origin, _main_frame: bool, _channel: PageChannel, _payload: String, _cx: &mut App) {}

/// An extension event (design section 5.4), from the bridge's router. Until W4.3 lands it is dropped.
pub fn extension_event(_event: ExtensionEvent, _cx: &mut App) {}

/// A browser Chromium opened itself (design section 5.7), from the bridge's router. Until W4.3 lands it
/// is dropped, and the engine keeps the hidden browser until shutdown.
pub fn foreign_window(_event: ForeignWindowEvent, _cx: &mut App) {}
