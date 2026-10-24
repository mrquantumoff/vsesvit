//! Owned by W4.5.

use gpui::App;
use vsesvit_core::extensions::ExtensionId;

/// Boot step 10a (design sections 3.1 and 5.9): once the first window shows, restores the records boot
/// step 7 marked "needs restoring", on GPUI's background executor. Until W4.5 lands step 7 marks none.
pub fn start(needs_restoring: Vec<ExtensionId>, _cx: &mut App) {
    if !needs_restoring.is_empty() {
        log::warn!("{} extensions need restoring, which this build cannot do yet", needs_restoring.len());
    }
}
