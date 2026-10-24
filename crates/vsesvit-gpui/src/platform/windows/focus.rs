//! Owned by W1.2.
//! Keyboard focus inside one browser window. `SetFocus` on an inactive toplevel activates it, and can
//! make it the foreground window (spike-win §1.1), so every request is refused unless our toplevel is
//! the active window already. GPUI handles neither `WM_SETFOCUS` nor `WM_KILLFOCUS`, so this is safe
//! inside a GPUI update. `Window::activate_window` is never the fallback: it calls `SendInput`.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, GetFocus, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{IsChild, IsWindowVisible};

use super::{FocusOutcome, FocusTarget, hwnd};

/// Gives `target` the keyboard, if `toplevel` is active and the target is a shown part of it.
pub(crate) fn focus(toplevel: HWND, target: FocusTarget) -> FocusOutcome {
    let to = match target {
        FocusTarget::Chrome => toplevel,
        FocusTarget::Page(page) => hwnd(page),
    };
    unsafe {
        if GetActiveWindow() != toplevel {
            return FocusOutcome::Refused;
        }
        // A parked or hidden page, or one of another window, never takes the keyboard.
        if to != toplevel && (IsChild(toplevel, to) == 0 || IsWindowVisible(to) == 0) {
            return FocusOutcome::Refused;
        }
        SetFocus(to);
        // Chromium hands its focus on to a child of the page window.
        let now = GetFocus();
        if now == to || (to != toplevel && IsChild(to, now) != 0) { FocusOutcome::Done } else { FocusOutcome::Refused }
    }
}
