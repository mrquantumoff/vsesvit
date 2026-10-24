//! Owned by W1.2.
//! The Windows backend of the platform seam (design sections 1.3, 1.4): plain Win32 through
//! windows-sys, on GPUI's main thread only. This file holds what the surfaces and the desktop share:
//! the window classes, the process registry (which browser windows exist and where the hidden host
//! lives) and the deferral of calls that must not run inside a GPUI update.

pub mod capture;
pub mod console;
pub mod desktop;
pub mod focus;
pub mod frame;
pub mod instance;
pub mod surface;

use std::cell::RefCell;
use std::collections::VecDeque;

use windows_sys::Win32::Foundation::{GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetParent, HWND_MESSAGE, PostMessageW, RegisterClassExW, SetParent, WM_APP,
    WM_CLOSE, WNDCLASSEXW, WNDPROC,
};

pub use desktop::WinDesktop;
pub use super::{
    AuxHost, Desktop, FocusOutcome, FocusTarget, Hole, NativeHandle, NativeSurface, PhysPoint, PhysRect,
    PlatformError,
};
pub use surface::WinSurface;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
}

pub(crate) fn error(what: impl std::fmt::Display) -> PlatformError {
    PlatformError(what.to_string())
}

/// A failed Win32 call, with the thread's last error.
pub(crate) fn last_error(call: &str) -> PlatformError {
    error(format!("{call}: {}", std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)))
}

pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn hwnd(handle: NativeHandle) -> HWND {
    handle.hwnd()
}

pub(crate) fn handle(hwnd: HWND) -> NativeHandle {
    NativeHandle::from_hwnd(hwnd)
}

/// The HWND of a GPUI window.
pub fn window_hwnd(window: &gpui::Window) -> Result<HWND, PlatformError> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => Ok(h.hwnd.get() as HWND),
        Ok(other) => Err(error(format!("GPUI window is not a Win32 window: {other:?}"))),
        Err(e) => Err(error(format!("GPUI window handle: {e}"))),
    }
}

/// The classes this backend creates windows of.
#[derive(Copy, Clone)]
pub(crate) enum Class {
    /// The content host and aux hosts: plain children of a GPUI window.
    Host,
    /// Desktop's hidden host. Ignores `WM_CLOSE`: CEF's default close of an Alloy child posts it to the
    /// root window, which would destroy every hidden browser with it (spike-ext-win §5.2).
    Hidden,
    /// The hidden toplevel that holds the hidden host while no browser window exists.
    Anchor,
    /// The message-only window that runs deferred calls.
    Dispatcher,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Class::Host => "VsesvitContentHost",
            Class::Hidden => "VsesvitHiddenHost",
            Class::Anchor => "VsesvitHiddenAnchor",
            Class::Dispatcher => "VsesvitDispatcher",
        }
    }

    fn procedure(self) -> WNDPROC {
        match self {
            Class::Host => Some(DefWindowProcW),
            Class::Hidden | Class::Anchor => Some(ignore_close),
            Class::Dispatcher => Some(dispatch),
        }
    }
}

unsafe extern "system" fn ignore_close(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_CLOSE {
        log::debug!("platform: the hidden host ignored WM_CLOSE");
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Creates a window of `class`, registering the class on first use.
pub(crate) fn create(
    class: Class,
    ex_style: u32,
    style: u32,
    rect: PhysRect,
    parent: HWND,
) -> Result<HWND, PlatformError> {
    let name = wide(class.name());
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let registered = PROCESS.with_borrow(|p| p.classes.contains(&class.name()));
        if !registered {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: class.procedure(),
                hInstance: instance,
                lpszClassName: name.as_ptr(),
                // No background: GDI paint of a child of GPUI's WS_EX_NOREDIRECTIONBITMAP window never shows.
                // Only children with their own surface do (Chromium's DirectX output, layered windows).
                ..std::mem::zeroed()
            };
            if RegisterClassExW(&wc) == 0 {
                return Err(last_error("RegisterClassExW"));
            }
            PROCESS.with_borrow_mut(|p| p.classes.push(class.name()));
        }
        let window = CreateWindowExW(
            ex_style,
            name.as_ptr(),
            name.as_ptr(),
            style,
            rect.x,
            rect.y,
            rect.width as i32,
            rect.height as i32,
            parent,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if window.is_null() {
            return Err(last_error("CreateWindowExW"));
        }
        Ok(window)
    }
}

/// What this process's surfaces and desktop share. Main thread only, so a thread-local; no borrow of
/// it is ever held across a Win32 call, because those can re-enter a window procedure.
#[derive(Default)]
struct Process {
    classes: Vec<&'static str>,
    /// Toplevels with an attached surface, oldest first.
    windows: Vec<HWND>,
    hidden: Option<Hidden>,
    dispatcher: Option<HWND>,
    deferred: VecDeque<Box<dyn FnOnce()>>,
}

#[derive(Copy, Clone)]
pub(crate) struct Hidden {
    pub host: HWND,
    pub anchor: HWND,
}

thread_local! {
    static PROCESS: RefCell<Process> = RefCell::default();
}

/// The hidden host is a hidden child of a browser window, never a toplevel: an Alloy browser
/// activates the toplevel it is created in (spike-ext-win §5.1, proposed decision 3). With no
/// browser window it waits in the anchor.
fn holder(windows: &[HWND], anchor: HWND) -> HWND {
    windows.first().copied().unwrap_or(anchor)
}

fn rehome_hidden_host() {
    let Some((hidden, to)) = PROCESS.with_borrow(|p| p.hidden.map(|h| (h, holder(&p.windows, h.anchor)))) else {
        return;
    };
    unsafe {
        if GetParent(hidden.host) != to && SetParent(hidden.host, to).is_null() {
            log::error!("platform: moving the hidden host failed: {}", last_error("SetParent"));
        }
    }
}

pub(crate) fn window_attached(window: HWND) {
    PROCESS.with_borrow_mut(|p| p.windows.push(window));
    rehome_hidden_host();
}

/// Before the window is destroyed: the hidden host moves out of it first.
pub(crate) fn window_detached(window: HWND) {
    PROCESS.with_borrow_mut(|p| p.windows.retain(|w| *w != window));
    rehome_hidden_host();
}

pub(crate) fn browser_windows() -> Vec<HWND> {
    PROCESS.with_borrow(|p| p.windows.clone())
}

pub(crate) fn hidden() -> Option<Hidden> {
    PROCESS.with_borrow(|p| p.hidden)
}

pub(crate) fn set_hidden(hidden: Option<Hidden>) {
    PROCESS.with_borrow_mut(|p| p.hidden = hidden);
    rehome_hidden_host();
}

const WM_RUN_DEFERRED: u32 = WM_APP + 0x51;

/// Runs `call` from the message loop, after the current GPUI update: a Win32 call that sends messages to
/// a GPUI window from inside an update is lost (spike-win §5). Calls run in order.
pub(crate) fn defer(call: impl FnOnce() + 'static) {
    let (dispatcher, first) = PROCESS.with_borrow_mut(|p| {
        p.deferred.push_back(Box::new(call));
        (p.dispatcher, p.deferred.len() == 1)
    });
    let dispatcher = match dispatcher {
        Some(d) => d,
        None => match create(Class::Dispatcher, 0, 0, PhysRect::default(), HWND_MESSAGE) {
            Ok(d) => {
                PROCESS.with_borrow_mut(|p| p.dispatcher = Some(d));
                d
            }
            Err(e) => {
                log::error!("platform: no dispatcher window, running a deferred call now: {e}");
                run_deferred();
                return;
            }
        },
    };
    if first && unsafe { PostMessageW(dispatcher, WM_RUN_DEFERRED, 0, 0) } == 0 {
        log::error!("platform: {}", last_error("PostMessageW"));
    }
}

fn run_deferred() {
    while let Some(call) = PROCESS.with_borrow_mut(|p| p.deferred.pop_front()) {
        call();
    }
}

unsafe extern "system" fn dispatch(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_RUN_DEFERRED {
        run_deferred();
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::holder;

    #[test]
    fn the_hidden_host_lives_in_the_oldest_browser_window_or_the_anchor() {
        let (a, b, anchor) = (1 as _, 2 as _, 9 as _);
        assert_eq!(holder(&[a, b], anchor), a);
        assert_eq!(holder(&[b], anchor), b);
        assert_eq!(holder(&[], anchor), anchor);
    }
}
