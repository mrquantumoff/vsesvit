//! Windows pieces the shared glue calls (`crate::platform::*`) and the startup guards:
//! private-desktop guard, hidden host, foreign-window neutralize, CDP pipes, the optional
//! per-process HKCU override, and a WinEvent log of our own toplevels.
use crate::win::{self, Hwnd};
use std::os::windows::io::FromRawHandle as _;
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// The only desktop the spike accepts (deskrun.exe creates it; it is never switched to).
pub const PRIVATE_DESKTOP: &str = "vsesvit-extwin";

/// Fail closed: Ok only when this thread's desktop is `PRIVATE_DESKTOP` and it is not the
/// input desktop the user sees. No override exists.
pub fn guard() -> Result<String, String> {
    let (ours, input, safe) = win::init_desktop_guard();
    if !ours.eq_ignore_ascii_case(PRIVATE_DESKTOP) {
        return Err(format!("thread desktop {ours:?} is not {PRIVATE_DESKTOP:?}"));
    }
    if !safe {
        return Err(format!("desktop {ours:?} is (or may be) the input desktop {input:?}"));
    }
    Ok(format!("desktop={ours} input={input}"))
}

pub fn is_shown(handle: u64) -> bool {
    handle != 0 && win::is_visible(handle as Hwnd)
}

/// Hide a Chrome-created toplevel and park it under the hidden host, synchronously.
pub fn neutralize(handle: u64, hidden: u64) -> Result<String, String> {
    let h = handle as Hwnd;
    if h == 0 || !win::is_window(h) {
        return Err("no window".into());
    }
    let parent = win::parent(h);
    let was = win::is_visible(h);
    unsafe {
        ShowWindow(h as HWND, SW_HIDE);
        if hidden != 0 {
            let style = GetWindowLongPtrW(h as HWND, GWL_STYLE) as u32;
            SetWindowLongPtrW(h as HWND, GWL_STYLE, ((style & !WS_POPUP & !WS_CAPTION) | WS_CHILD) as isize);
            SetParent(h as HWND, hidden as HWND);
        }
    }
    Ok(format!("was_visible={was} old_parent=0x{parent:x} new_parent=0x{:x} visible_now={} class={}", win::parent(h), win::is_visible(h), win::class_name(h)))
}

unsafe extern "system" fn hidden_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    // A child browser's default close posts WM_CLOSE to its root window; the hidden host
    // must outlive every hidden browser, so it never closes on request.
    if msg == WM_CLOSE {
        crate::log("platform: hidden host ignored WM_CLOSE");
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// A never-shown window that parents hidden browsers and neutralized foreign windows.
/// SPIKE_HIDDEN_HOST: "child" (default: a hidden WS_CHILD of the GPUI window), "popup" (a WS_POPUP toplevel), "noactivate" (the same with
/// WS_EX_NOACTIVATE), "message" (a message-only window). The toplevel kinds get activated by Alloy hidden browsers (ITEM 12).
pub fn create_hidden_host(gpui: Hwnd) -> Hwnd {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("VsesvitHiddenHost");
        let wc = WNDCLASSW { lpfnWndProc: Some(hidden_proc), hInstance: hinst, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let mode = std::env::var("SPIKE_HIDDEN_HOST").unwrap_or_else(|_| "child".into());
        let (ex, style, parent, x, y) = match mode.as_str() {
            "noactivate" => (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, WS_POPUP | WS_CLIPCHILDREN, std::ptr::null_mut(), -8000, -8000),
            "child" => (0, WS_CHILD | WS_CLIPCHILDREN, gpui as HWND, 0, 0),
            "message" => (0, WS_CLIPCHILDREN, HWND_MESSAGE, 0, 0),
            _ => (WS_EX_TOOLWINDOW, WS_POPUP | WS_CLIPCHILDREN, std::ptr::null_mut(), -8000, -8000),
        };
        crate::log(&format!("platform: hidden host mode {mode}"));
        CreateWindowExW(ex, class.as_ptr(), wide("vsesvit hidden host").as_ptr(), style, x, y, 1024, 700, parent, std::ptr::null_mut(), hinst, std::ptr::null()) as Hwnd
    }
}

/// `--remote-debugging-io-pipes`: two anonymous pipes, not inheritable. Chromium's ends go
/// into the switch value (same process, so the handle values are valid); ours start the
/// CDP client.
pub fn setup_cdp_pipes() -> Result<String, String> {
    use windows_sys::Win32::System::Pipes::CreatePipe;
    let (mut r1, mut w1, mut r2, mut w2): (HANDLE, HANDLE, HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
    unsafe {
        if CreatePipe(&mut r1, &mut w1, std::ptr::null(), 1 << 20) == 0 || CreatePipe(&mut r2, &mut w2, std::ptr::null(), 1 << 20) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let value = format!("{},{}", r1 as usize, w2 as usize);
        let _ = crate::cefglue::app::IO_PIPES.set(value.clone());
        crate::cdp::start(std::fs::File::from_raw_handle(w1 as _), std::fs::File::from_raw_handle(r2 as _));
        Ok(value)
    }
}

/// `SPIKE_REG_OVERRIDE=<subkey>`: redirect HKEY_CURRENT_USER for this process only to
/// HKCU\<subkey> (RegOverridePredefKey). The per-profile form of Chromium's registry
/// install route (`Software\Google\Chrome\Extensions`). Browser process only.
pub fn reg_override(subkey: &str) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::*;
    let wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let mut key: HKEY = std::ptr::null_mut();
    unsafe {
        let rc = RegCreateKeyExW(HKEY_CURRENT_USER, wide.as_ptr(), 0, std::ptr::null(), 0, KEY_ALL_ACCESS, std::ptr::null(), &mut key, std::ptr::null_mut());
        if rc != 0 {
            return Err(format!("RegCreateKeyExW {rc}"));
        }
        let rc = RegOverridePredefKey(HKEY_CURRENT_USER, key);
        if rc != 0 {
            return Err(format!("RegOverridePredefKey {rc}"));
        }
    }
    Ok(format!("HKCU -> HKCU\\{subkey}"))
}

unsafe extern "system" fn winevent(_hook: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, _id_child: i32, _thread: u32, _time: u32) {
    const OBJID_WINDOW: i32 = 0;
    if id_object != OBJID_WINDOW || hwnd.is_null() {
        return;
    }
    let h = hwnd as Hwnd;
    let top = unsafe { GetAncestor(hwnd, GA_PARENT) } == unsafe { GetDesktopWindow() };
    let name = match event {
        EVENT_OBJECT_CREATE => "create",
        EVENT_OBJECT_DESTROY => "destroy",
        EVENT_OBJECT_SHOW => "show",
        EVENT_OBJECT_HIDE => "hide",
        EVENT_OBJECT_FOCUS => "focus",
        EVENT_SYSTEM_FOREGROUND => "foreground",
        _ => return,
    };
    if !top && event != EVENT_OBJECT_FOCUS {
        return;
    }
    if event == EVENT_OBJECT_DESTROY {
        crate::log(&format!("winevent: {name} 0x{h:x}"));
        return;
    }
    let root = win::root_of(h);
    crate::log(&format!("winevent: {name} 0x{h:x} top={top} class={:?} title={:?} root=0x{root:x} root_class={:?} active=0x{:x}", win::class_name(h), win::title(h), win::class_name(root), win::active_window()));
}

/// Log create/show/hide/focus of this process's windows (out-of-context hook, delivered
/// on the calling thread's message loop). Only our own process.
pub fn start_winevent_log() {
    unsafe {
        let pid = std::process::id();
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, std::ptr::null_mut(), Some(winevent), pid, 0, WINEVENT_OUTOFCONTEXT);
        SetWinEventHook(EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE, std::ptr::null_mut(), Some(winevent), pid, 0, WINEVENT_OUTOFCONTEXT);
        SetWinEventHook(EVENT_OBJECT_FOCUS, EVENT_OBJECT_FOCUS, std::ptr::null_mut(), Some(winevent), pid, 0, WINEVENT_OUTOFCONTEXT);
    }
}

/// True when `hwnd` is a window of this process (input is only ever sent to those).
pub fn ours(hwnd: Hwnd) -> bool {
    hwnd != 0 && win::is_window(hwnd) && win::thread_of(hwnd).1 == std::process::id()
}

/// Destroy one of our windows (a CEF browser's own HWND, after `do_close` returned 1).
pub fn destroy_window(hwnd: u64) {
    if hwnd != 0 && ours(hwnd as Hwnd) {
        unsafe { DestroyWindow(hwnd as HWND) };
        crate::log(&format!("platform: DestroyWindow 0x{hwnd:x}"));
    }
}
