//! `deskrun DESKTOP EXE [ARGS...]`: run EXE on a separate desktop of this window station
//! and wait for it. The desktop is created if needed and never switched to, so nothing
//! the child does (activation, foreground, fullscreen, Chrome's own windows) reaches the
//! desktop the user sees. The child inherits this process's environment and std handles.
use std::os::windows::ffi::OsStrExt as _;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, CreateDesktopW};
use windows_sys::Win32::System::Threading::*;

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) { a.to_string() } else { format!("\"{}\"", a.replace('"', "\\\"")) }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: deskrun DESKTOP EXE [ARGS...]");
        return std::process::ExitCode::from(2);
    }
    let name = &args[0];
    const GENERIC_ALL: u32 = 0x1000_0000;
    let desk = unsafe { CreateDesktopW(wide(name.as_ref()).as_ptr(), std::ptr::null(), std::ptr::null(), 0, GENERIC_ALL, std::ptr::null()) };
    if desk.is_null() {
        eprintln!("deskrun: CreateDesktopW({name}) failed: {}", std::io::Error::last_os_error());
        return std::process::ExitCode::from(3);
    }
    let cmdline: Vec<String> = args[1..].iter().map(|a| quote(a)).collect();
    let mut cmd = wide(cmdline.join(" ").as_ref());
    let mut desktop = wide(format!("WinSta0\\{name}").as_ref());
    let mut si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, lpDesktop: desktop.as_mut_ptr(), ..Default::default() };
    unsafe {
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
        si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
        si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    }
    let mut pi = PROCESS_INFORMATION { hProcess: std::ptr::null_mut(), hThread: std::ptr::null_mut(), dwProcessId: 0, dwThreadId: 0 };
    let ok = unsafe { CreateProcessW(std::ptr::null(), cmd.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 1, 0, std::ptr::null(), std::ptr::null(), &si, &mut pi) };
    if ok == 0 {
        eprintln!("deskrun: CreateProcessW failed: {}", std::io::Error::last_os_error());
        unsafe { CloseDesktop(desk) };
        return std::process::ExitCode::from(4);
    }
    eprintln!("deskrun: pid {} on desktop {name}", pi.dwProcessId);
    let mut code = 1u32;
    unsafe {
        if WaitForSingleObject(pi.hProcess as HANDLE, INFINITE) == WAIT_OBJECT_0 {
            GetExitCodeProcess(pi.hProcess, &mut code);
        }
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        CloseDesktop(desk);
    }
    eprintln!("deskrun: child exited with {code}");
    std::process::ExitCode::from(code.min(255) as u8)
}
