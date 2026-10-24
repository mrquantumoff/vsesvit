//! Owned by W3.3.

use gpui::App;
use vsesvit_engine::{DownloadRequest, DownloadUpdate};

/// A download is about to start, from the bridge's router. Until W3.3 lands nobody answers it, so it
/// never starts.
pub fn requested(_request: DownloadRequest, _cx: &mut App) {}

/// A download progressed.
pub fn updated(_update: DownloadUpdate, _cx: &mut App) {}
