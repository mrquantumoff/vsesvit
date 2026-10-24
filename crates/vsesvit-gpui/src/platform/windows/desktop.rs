//! Owned by W1.2.
//! The process-wide half: owner fix-ups for GPUI's PopUp and dialog windows, the browser window under
//! a screen point, pointer presses for outside-click dismissal, the forwarded-launch raise, the hidden
//! host, and neutralizing Chrome-created windows.

use std::cell::RefCell;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DestroyWindow, GA_ROOT, GW_HWNDNEXT, GWL_EXSTYLE, GWL_STYLE, GWLP_HWNDPARENT, GetAncestor,
    GetParent, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HC_ACTION, HHOOK,
    HWND_NOTOPMOST, IsIconic, IsWindow, IsWindowVisible, MSG, PM_REMOVE, SW_HIDE, SW_RESTORE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetParent, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW,
    ShowWindow, UnhookWindowsHookEx, WH_GETMESSAGE, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCLBUTTONDOWN, WM_NCMBUTTONDOWN,
    WM_NCRBUTTONDOWN, WM_NCXBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_THICKFRAME, WindowFromPoint,
};

use super::{
    Class, Desktop, Hidden, NativeHandle, PhysPoint, PhysRect, PlatformError, browser_windows, create, defer, error,
    handle, hidden, hwnd, last_error, set_hidden, window_hwnd,
};

pub struct WinDesktop {
    hidden: Hidden,
}

/// The size hidden browsers lay out at.
const HIDDEN_SIZE: PhysRect = PhysRect { x: 0, y: 0, width: 1024, height: 768 };

fn ours(window: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    pid == std::process::id()
}

fn contains(window: HWND, at: PhysPoint) -> bool {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(window, &mut r) != 0 && at.x >= r.left && at.x < r.right && at.y >= r.top && at.y < r.bottom }
}

impl Desktop for WinDesktop {
    fn init() -> Result<Self, PlatformError> {
        if hidden().is_some() {
            return Err(error("the desktop is already initialized"));
        }
        let anchor = create(Class::Anchor, WS_EX_TOOLWINDOW, WS_POPUP, HIDDEN_SIZE, std::ptr::null_mut())?;
        let host = match create(Class::Hidden, 0, WS_CHILD | WS_CLIPCHILDREN, HIDDEN_SIZE, anchor) {
            Ok(host) => host,
            Err(e) => {
                unsafe { DestroyWindow(anchor) };
                return Err(e);
            }
        };
        let hidden = Hidden { host, anchor };
        set_hidden(Some(hidden));
        Ok(WinDesktop { hidden })
    }

    /// GPUI creates a PopUp `WS_EX_TOPMOST` with no owner, so it floats above other apps' windows once
    /// ours deactivates. Owned by the browser window and not topmost, it sits just above it (spike-win §9.2).
    fn adopt_popup(&self, popup: &gpui::Window, owner: &gpui::Window) {
        let (Ok(popup), Ok(owner)) = (window_hwnd(popup), window_hwnd(owner)) else {
            return;
        };
        defer(move || unsafe {
            SetWindowLongPtrW(popup, GWLP_HWNDPARENT, owner as isize);
            SetWindowPos(popup, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        });
    }

    fn set_dialog_parent(&self, dialog: &gpui::Window, parent: &gpui::Window) {
        let (Ok(dialog), Ok(parent)) = (window_hwnd(dialog), window_hwnd(parent)) else {
            return;
        };
        defer(move || unsafe {
            SetWindowLongPtrW(dialog, GWLP_HWNDPARENT, parent as isize);
        });
    }

    /// The topmost window at `at` decides: one of our browser windows, or None for anything else.
    /// Our own PopUp windows (a dragged tab's preview, a menu) are looked through.
    fn toplevel_at(&self, at: PhysPoint) -> Option<NativeHandle> {
        let browsers = browser_windows();
        unsafe {
            let mut window = GetAncestor(WindowFromPoint(POINT { x: at.x, y: at.y }), GA_ROOT);
            while !window.is_null() {
                if browsers.contains(&window) {
                    return Some(handle(window));
                }
                let style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
                if !(ours(window) && style & WS_EX_TOOLWINDOW != 0) {
                    return None;
                }
                window = GetWindow(window, GW_HWNDNEXT);
                while !window.is_null() && !(IsWindowVisible(window) != 0 && IsIconic(window) == 0 && contains(window, at)) {
                    window = GetWindow(window, GW_HWNDNEXT);
                }
            }
            None
        }
    }

    fn pointer_presses(&self) -> UnboundedReceiver<PhysPoint> {
        let (sender, receiver) = unbounded();
        let install = PRESSES.with_borrow_mut(|p| {
            p.senders.push(sender);
            p.hook.is_none()
        });
        if install {
            let hook = unsafe { SetWindowsHookExW(WH_GETMESSAGE, Some(message_hook), std::ptr::null_mut(), GetCurrentThreadId()) };
            if hook.is_null() {
                log::error!("platform: no pointer presses: {}", last_error("SetWindowsHookExW"));
            } else {
                PRESSES.with_borrow_mut(|p| p.hook = Some(hook));
            }
        }
        receiver
    }

    /// The client of a forwarded launch granted this process the foreground (`AllowSetForegroundWindow`).
    fn raise_for_invocation(&self, window: &gpui::Window, _token: Option<&str>) {
        let Ok(window) = window_hwnd(window) else {
            return;
        };
        defer(move || unsafe {
            if IsIconic(window) != 0 {
                ShowWindow(window, SW_RESTORE);
            }
            #[allow(clippy::disallowed_methods)] // the one forwarded-launch raise (design section 7.3)
            SetForegroundWindow(window);
        });
    }

    fn hidden_host(&self) -> NativeHandle {
        handle(self.hidden.host)
    }

    fn neutralize(&self, window: NativeHandle) -> Result<(), PlatformError> {
        let window = hwnd(window);
        unsafe {
            if window.is_null() || IsWindow(window) == 0 {
                return Err(error("the window is gone"));
            }
            if !ours(window) {
                return Err(error("the window belongs to another process"));
            }
            ShowWindow(window, SW_HIDE);
            let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
            SetWindowLongPtrW(window, GWL_STYLE, ((style & !(WS_POPUP | WS_CAPTION | WS_THICKFRAME)) | WS_CHILD) as isize);
            let ex = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(window, GWL_EXSTYLE, (ex & !WS_EX_TOPMOST) as isize);
            if SetParent(window, self.hidden.host).is_null() {
                return Err(last_error("SetParent"));
            }
            if GetParent(window) != self.hidden.host {
                return Err(error("SetParent did not move the window"));
            }
        }
        Ok(())
    }
}

/// After the last browser closed (design section 3.6).
impl Drop for WinDesktop {
    fn drop(&mut self) {
        set_hidden(None);
        if let Some(hook) = PRESSES.with_borrow_mut(|p| {
            p.senders.clear();
            p.hook.take()
        }) {
            unsafe { UnhookWindowsHookEx(hook) };
        }
        unsafe {
            DestroyWindow(self.hidden.host);
            DestroyWindow(self.hidden.anchor);
        }
    }
}

#[derive(Default)]
struct Presses {
    hook: Option<HHOOK>,
    senders: Vec<UnboundedSender<PhysPoint>>,
}

thread_local! {
    static PRESSES: RefCell<Presses> = RefCell::default();
}

fn is_press(message: u32) -> bool {
    matches!(
        message,
        WM_LBUTTONDOWN
            | WM_RBUTTONDOWN
            | WM_MBUTTONDOWN
            | WM_XBUTTONDOWN
            | WM_NCLBUTTONDOWN
            | WM_NCRBUTTONDOWN
            | WM_NCMBUTTONDOWN
            | WM_NCXBUTTONDOWN
    )
}

/// Where a press message happened, in screen pixels, from its own `lParam`: client coordinates of its
/// window, or screen coordinates for a non-client press. Not `MSG.pt`, which for a posted message is
/// wherever the cursor was.
fn press_point(msg: &MSG) -> PhysPoint {
    let (x, y) = (msg.lParam as u16 as i16 as i32, (msg.lParam >> 16) as u16 as i16 as i32);
    let mut p = POINT { x, y };
    if !matches!(msg.message, WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN | WM_NCMBUTTONDOWN | WM_NCXBUTTONDOWN) {
        unsafe { ClientToScreen(msg.hwnd, &mut p) };
    }
    PhysPoint { x: p.x, y: p.y }
}

/// A `WH_GETMESSAGE` thread hook: it sees every message this thread's loop takes out of its queue,
/// for every window of the thread, CEF's included, and so presses that were posted as well as real
/// ones. A `WH_MOUSE` hook sees only real input (native_probe, log.tsv 2026-10-24). In-process only.
unsafe extern "system" fn message_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam as u32 == PM_REMOVE {
        let msg = unsafe { &*(lparam as *const MSG) };
        if is_press(msg.message) {
            let at = press_point(msg);
            PRESSES.with_borrow_mut(|p| p.senders.retain(|s| s.unbounded_send(at).is_ok()));
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}
