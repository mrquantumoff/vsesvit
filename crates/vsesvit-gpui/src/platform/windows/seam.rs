//! Owned by W1.2.
//! A copy of design section 1.4's seam, so the Windows backend compiles before W1.1 fills
//! `platform/mod.rs`. Once it does, this file goes and `windows/mod.rs` imports the same names from
//! `super` (one `use` line). `PlatformError` is the one shape section 1.4 leaves open; the backend
//! builds it only through `windows::error`.

use std::fmt;

pub use vsesvit_engine::{NativeHandle, PhysRect};

/// Screen pixels; never in the engine contract.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PhysPoint {
    pub x: i32,
    pub y: i32,
}

/// A rectangle over the content rect that GPUI paints into. `interactive` holes also take the mouse.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Hole {
    pub rect: PhysRect,
    pub interactive: bool,
}

pub enum FocusTarget {
    Chrome,
    Page(NativeHandle),
}

/// `Refused`: our toplevel is not the active window.
pub enum FocusOutcome {
    Done,
    Refused,
}

/// A second host above the content host (extension popups).
pub struct AuxHost(pub(super) NativeHandle);

#[derive(Debug)]
pub struct PlatformError(pub String);

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlatformError {}

/// One per GPUI browser window. Main thread only. Calls that send messages to the GPUI window
/// are deferred by the implementation out of the current GPUI update (SW §5 resize trap).
pub trait NativeSurface: Sized + 'static {
    /// Windows: GPUI's DirectComposition target is topmost, so the content rect must stay transparent (SW §5).
    const TRANSPARENT_CONTENT: bool;
    fn attach(window: &gpui::Window) -> Result<Self, PlatformError>;
    fn host(&self) -> NativeHandle;
    fn set_content_rect(&mut self, rect: PhysRect);
    fn show_page(&mut self, page: NativeHandle, visible: bool);
    fn set_holes(&mut self, holes: &[Hole]);
    fn focus(&mut self, target: FocusTarget) -> FocusOutcome;
    fn move_page_to(&mut self, page: NativeHandle, to: &mut Self) -> Result<(), PlatformError>;
    /// Download keep-alive (§4.2): hide the page and reparent it into Desktop's hidden host. Returns only after
    /// the native reparent is done, so the caller may then detach this surface.
    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError>;
    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError>;
    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect);
    fn close_aux(&mut self, aux: AuxHost);
    fn capture(&self) -> Result<image::RgbaImage, PlatformError>;
    fn detach(self);
}

/// Process-wide. Main thread only.
pub trait Desktop: Sized + 'static {
    fn init() -> Result<Self, PlatformError>;
    fn adopt_popup(&self, popup: &gpui::Window, owner: &gpui::Window);
    fn set_dialog_parent(&self, dialog: &gpui::Window, parent: &gpui::Window);
    fn toplevel_at(&self, at: PhysPoint) -> Option<NativeHandle>;
    fn pointer_presses(&self) -> futures::channel::mpsc::UnboundedReceiver<PhysPoint>;
    fn raise_for_invocation(&self, window: &gpui::Window, token: Option<&str>);
    fn hidden_host(&self) -> NativeHandle;
    /// Synchronous and App-free: called from inside `on_after_created` through `ShellHooks::neutralize_foreign`.
    fn neutralize(&self, window: NativeHandle) -> Result<(), PlatformError>;
}
