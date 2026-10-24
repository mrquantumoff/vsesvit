//! Owned by W1.1.
//! Every native window call of the shell (design section 1.4). One backend per OS, chosen by `cfg`;
//! the traits make both backends compile against the same signatures. The shell holds a window's
//! surface as `Box<dyn SurfaceOps>`, built by `attach` here or by `fake::attach` in tests, and the
//! desktop as `Os`.

use std::any::Any;
use std::fmt;

pub use vsesvit_engine::{NativeHandle, PhysRect};

pub mod default_browser;
#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod files;
pub mod shell_open;
#[cfg(windows)]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod x11;

#[cfg(target_os = "linux")]
pub type Surface = x11::XSurface;
#[cfg(target_os = "linux")]
pub type Os = x11::XDesktop;
#[cfg(windows)]
pub type Surface = windows::WinSurface;
#[cfg(windows)]
pub type Os = windows::WinDesktop;

/// A point on the screen, in physical pixels. Never in the engine contract, which has its own
/// page-relative `PagePoint`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PhysPoint {
    pub x: i32,
    pub y: i32,
}

/// A rectangle over the content rect that GPUI paints into, relative to the GPUI window's client
/// area. `interactive` holes also take the mouse.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Hole {
    pub rect: PhysRect,
    pub interactive: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// GPUI's own widgets: the omnibox, menus, the tab strip.
    Chrome,
    Page(NativeHandle),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FocusOutcome {
    Done,
    /// Our toplevel is not the active window, so the keyboard stays where it is.
    Refused,
}

/// A second host above the content host, for extension popups. Closed only through
/// `NativeSurface::close_aux`, so it is neither `Copy` nor `Clone`.
#[derive(Debug, PartialEq, Eq)]
pub struct AuxHost(NativeHandle);

impl AuxHost {
    /// The native parent for the popup's browser (`TabSpec::parent`).
    pub fn native(&self) -> NativeHandle {
        self.0
    }
}

/// A native call failed. The shell never branches on why; it logs the text and takes its fallback
/// (closes the tab, refuses the foreign window, shows `fatal_message`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformError(String);

impl PlatformError {
    pub fn new(what: impl Into<String>) -> PlatformError {
        PlatformError(what.into())
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlatformError {}

/// One per GPUI browser window. Main thread only. Calls that send messages to the GPUI window
/// are deferred by the implementation out of the current GPUI update (SW section 5 resize trap).
pub trait NativeSurface: Sized + 'static {
    /// Windows: GPUI's DirectComposition target is topmost, so the content rect must stay transparent.
    const TRANSPARENT_CONTENT: bool;
    /// Creates the content host.
    fn attach(window: &gpui::Window) -> Result<Self, PlatformError>;
    /// The parent for every tab's browser.
    fn host(&self) -> NativeHandle;
    /// Moves the host, then sizes every shown page to `0, 0, width, height`.
    fn set_content_rect(&mut self, rect: PhysRect);
    /// X11: map or unmap; Windows: `SW_SHOWNA` or `SW_HIDE`.
    fn show_page(&mut self, page: NativeHandle, visible: bool);
    /// X11: SHAPE out every hole; Windows: a region without the interactive ones.
    fn set_holes(&mut self, holes: &[Hole]);
    fn focus(&mut self, target: FocusTarget) -> FocusOutcome;
    /// A tab moves to another window. The page arrives hidden in `to`'s host.
    fn move_page_to(&mut self, page: NativeHandle, to: &mut Self) -> Result<(), PlatformError>;
    /// Download keep-alive: hide the page and reparent it into `Desktop`'s hidden host. Returns only
    /// after the native reparent is done, so the caller may then detach this surface.
    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError>;
    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError>;
    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect);
    fn close_aux(&mut self, aux: AuxHost);
    /// The whole window, page included.
    fn capture(&self) -> Result<image::RgbaImage, PlatformError>;
    /// After every page of this window closed or was parked.
    fn detach(self);
}

/// The object-safe part of `NativeSurface`, which every `NativeSurface` implements. What the
/// shell holds, so tests can hand it a `FakeSurface` without swapping the `Surface` alias.
pub trait SurfaceOps: 'static {
    fn host(&self) -> NativeHandle;
    fn set_content_rect(&mut self, rect: PhysRect);
    fn show_page(&mut self, page: NativeHandle, visible: bool);
    fn set_holes(&mut self, holes: &[Hole]);
    fn focus(&mut self, target: FocusTarget) -> FocusOutcome;
    /// Fails without a native call when `to` is a surface of another type.
    fn move_page_to(&mut self, page: NativeHandle, to: &mut dyn SurfaceOps) -> Result<(), PlatformError>;
    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError>;
    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError>;
    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect);
    fn close_aux(&mut self, aux: AuxHost);
    fn capture(&self) -> Result<image::RgbaImage, PlatformError>;
    fn detach(self: Box<Self>);
    /// For `move_page_to` and for tests that inspect a `FakeSurface`.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<S: NativeSurface> SurfaceOps for S {
    fn host(&self) -> NativeHandle {
        NativeSurface::host(self)
    }

    fn set_content_rect(&mut self, rect: PhysRect) {
        NativeSurface::set_content_rect(self, rect)
    }

    fn show_page(&mut self, page: NativeHandle, visible: bool) {
        NativeSurface::show_page(self, page, visible)
    }

    fn set_holes(&mut self, holes: &[Hole]) {
        NativeSurface::set_holes(self, holes)
    }

    fn focus(&mut self, target: FocusTarget) -> FocusOutcome {
        NativeSurface::focus(self, target)
    }

    fn move_page_to(&mut self, page: NativeHandle, to: &mut dyn SurfaceOps) -> Result<(), PlatformError> {
        let to = to
            .as_any_mut()
            .downcast_mut::<S>()
            .ok_or_else(|| PlatformError::new("moving a page between surfaces of different kinds"))?;
        NativeSurface::move_page_to(self, page, to)
    }

    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError> {
        NativeSurface::park_page(self, page, hidden_host)
    }

    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError> {
        NativeSurface::open_aux(self, rect)
    }

    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect) {
        NativeSurface::set_aux_rect(self, aux, rect)
    }

    fn close_aux(&mut self, aux: AuxHost) {
        NativeSurface::close_aux(self, aux)
    }

    fn capture(&self) -> Result<image::RgbaImage, PlatformError> {
        NativeSurface::capture(self)
    }

    fn detach(self: Box<Self>) {
        NativeSurface::detach(*self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Process-wide. Main thread only.
pub trait Desktop: Sized + 'static {
    fn init() -> Result<Self, PlatformError>;
    /// Windows: owner + clear TOPMOST (SW section 9.2). X11: nothing; PopUp windows are override-redirect.
    fn adopt_popup(&self, popup: &gpui::Window, owner: &gpui::Window);
    /// X11 `WM_TRANSIENT_FOR`; Windows owner.
    fn set_dialog_parent(&self, dialog: &gpui::Window, parent: &gpui::Window);
    /// Our browser window under the pointer.
    fn toplevel_at(&self, at: PhysPoint) -> Option<NativeHandle>;
    /// Every pointer press anywhere on the screen, for outside-click dismissal. Each call gets its
    /// own receiver.
    fn pointer_presses(&self) -> futures::channel::mpsc::UnboundedReceiver<PhysPoint>;
    /// A forwarded launch asks for `window`; `token` is the launch's startup id, if it had one.
    fn raise_for_invocation(&self, window: &gpui::Window, token: Option<&str>);
    /// The process-wide hidden native window (design section 1.3).
    fn hidden_host(&self) -> NativeHandle;
    /// Synchronous and App-free: called from inside `on_after_created` through
    /// `ShellHooks::neutralize_foreign`. X11: unmap, then reparent into the hidden host. Windows:
    /// SW_HIDE, then SetParent into the hidden host. An error (the window is gone, a call failed)
    /// makes the glue close the foreign browser at once (section 5.7).
    fn neutralize(&self, window: NativeHandle) -> Result<(), PlatformError>;
}

/// The real backend's surface for `window`, as the shell holds it.
#[cfg(any(target_os = "linux", windows))]
pub fn attach(window: &gpui::Window) -> Result<Box<dyn SurfaceOps>, PlatformError> {
    Ok(Box::new(<Surface as NativeSurface>::attach(window)?))
}

/// A startup failure the user must see: stderr, and on Windows release builds (which have no
/// console) also a message box. Never a box inside a test environment, where nobody can close it.
pub fn fatal_message(text: &str) {
    eprintln!("{text}");
    #[cfg(all(windows, not(debug_assertions)))]
    if std::env::var_os("VSESVIT_TESTENV").is_none() {
        message_box(text);
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn message_box(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, caption) = (wide(text), wide("Vsesvit"));
    // SAFETY: both strings are NUL-terminated and outlive the call; a null owner is allowed.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR) };
}
