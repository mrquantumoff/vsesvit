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
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, PostMessageW, WM_KEYDOWN, WM_KEYUP};

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
    #[allow(clippy::not_unsafe_ptr_arg_deref)] // an HWND is a handle the system checks, never dereferenced
    pub fn post_key(&self, hwnd: HWND, vk: u16) -> Result<(), String> {
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
        ours(owner, unsafe { GetCurrentProcessId() })?;
        // lParam: repeat count 1; the release also sets the previous-state and transition bits.
        for (message, lparam) in [(WM_KEYDOWN, 1 as LPARAM), (WM_KEYUP, 0xC000_0001u32 as LPARAM)] {
            if unsafe { PostMessageW(hwnd, message, usize::from(vk), lparam) } == 0 {
                return Err(format!("PostMessageW: {}", std::io::Error::last_os_error()));
            }
        }
        Ok(())
    }
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
