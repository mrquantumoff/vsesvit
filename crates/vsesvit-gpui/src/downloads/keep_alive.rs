//! Owned by W3.3.

use vsesvit_engine::TabId;

/// What the shell's close path does with a closing tab's browser (design section 4.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ParkDecision {
    /// Park the page in the hidden host and detach the tab; the browser lives on for its download.
    Park,
    /// Close the browser with the tab.
    Close,
}

/// Asked by the tab and window close paths (W1.5, W1.7) before a tab closes. Until W3.3 lands, every
/// tab closes.
pub fn park_decision(_tab: TabId, _cx: &gpui::App) -> ParkDecision {
    ParkDecision::Close
}
