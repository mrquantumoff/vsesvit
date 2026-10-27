//! Owned by W0.5.
//! Windows answers for the guard (this thread's desktop and the input desktop, by name) and
//! `PrivateInput`, which posts messages to this process's own windows and never calls `SendInput`
//! (design section 7.3).

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_READOBJECTS, GetThreadDesktop, GetUserObjectInformationW, HDESK, OpenInputDesktop,
    UOI_NAME,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, PostMessageW, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

fn name_of(desktop: HDESK) -> Result<String, String> {
    let mut buf = [0u16; 256];
    let mut needed = 0u32;
    let ok = unsafe {
        GetUserObjectInformationW(desktop, UOI_NAME, buf.as_mut_ptr().cast(), size_of_val(&buf) as u32, &mut needed)
    };
    if ok == 0 {
        return Err(format!("GetUserObjectInformationW: {}", std::io::Error::last_os_error()));
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf16_lossy(&buf[..len]))
}

pub(crate) fn thread_desktop() -> Result<String, String> {
    // Not closed: GetThreadDesktop's handle belongs to the thread.
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    if desktop.is_null() {
        return Err(format!("GetThreadDesktop: {}", std::io::Error::last_os_error()));
    }
    name_of(desktop)
}

pub(crate) fn input_desktop() -> Result<String, String> {
    let desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    if desktop.is_null() {
        return Err(format!("OpenInputDesktop: {}", std::io::Error::last_os_error()));
    }
    let name = name_of(desktop);
    unsafe { CloseDesktop(desktop) };
    name
}

/// Key input into this process's own windows, by posted messages. Only `TestEnv::input` builds
/// one. It reaches no other process, so it cannot reach the user's desktop.
pub struct PrivateInput {
    _private: (),
}

impl PrivateInput {
    pub(crate) fn new() -> PrivateInput {
        PrivateInput { _private: () }
    }

    /// Posts a key press and release of virtual key `vk` to `hwnd`, which must belong to this
    /// process.
    pub fn post_key(&self, hwnd: HWND, vk: u16) -> Result<(), String> {
        // lParam: repeat count 1; the release also sets the previous-state and transition bits.
        post(hwnd, &[(WM_KEYDOWN, usize::from(vk), 1), (WM_KEYUP, usize::from(vk), 0xC000_0001u32 as LPARAM)])
    }

    /// Posts the pointer's move to (`x`, `y`), in `hwnd`'s client pixels, to `hwnd`, which must belong
    /// to this process.
    pub fn post_mouse_move(&self, hwnd: HWND, x: i16, y: i16) -> Result<(), String> {
        post(hwnd, &[(WM_MOUSEMOVE, 0, point(x, y))])
    }

    /// Posts a left click at (`x`, `y`), in `hwnd`'s client pixels, to `hwnd`, which must belong to
    /// this process: the window a real click there reaches is the deepest child under the point.
    pub fn post_click(&self, hwnd: HWND, x: i16, y: i16) -> Result<(), String> {
        let at = point(x, y);
        post(hwnd, &[(WM_MOUSEMOVE, 0, at), (WM_LBUTTONDOWN, MK_LBUTTON, at), (WM_LBUTTONUP, 0, at)])
    }
}

/// wParam of a press: the left button is down (`MK_LBUTTON`).
const MK_LBUTTON: usize = 0x0001;

/// A mouse message's lParam: the client point, x in the low word.
fn point(x: i16, y: i16) -> LPARAM {
    ((y as u16 as u32) << 16 | x as u16 as u32) as LPARAM
}

#[allow(clippy::not_unsafe_ptr_arg_deref)] // an HWND is a handle the system checks, never dereferenced
fn post(hwnd: HWND, messages: &[(u32, usize, LPARAM)]) -> Result<(), String> {
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
    ours(owner, unsafe { GetCurrentProcessId() })?;
    for &(message, wparam, lparam) in messages {
        if unsafe { PostMessageW(hwnd, message, wparam, lparam) } == 0 {
            return Err(format!("PostMessageW: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

fn ours(owner: u32, me: u32) -> Result<(), String> {
    match owner {
        0 => Err("not a window".into()),
        owner if owner != me => Err(format!("the window belongs to pid {owner}, not to this process ({me})")),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mouse_points_pack_x_low_and_y_high() {
        assert_eq!(super::point(3, 2), 0x0002_0003);
        assert_eq!(super::point(-1, 0) & 0xffff, 0xffff);
    }

    #[test]
    fn input_goes_only_to_this_processs_windows() {
        assert!(super::ours(7, 7).is_ok());
        assert!(super::ours(8, 7).unwrap_err().contains("pid 8"));
        assert!(super::ours(0, 7).is_err());
    }

    #[test]
    fn the_test_runner_sits_on_a_named_desktop() {
        // A query only; nothing opens or switches a desktop.
        assert!(!super::thread_desktop().unwrap().is_empty());
    }
}
