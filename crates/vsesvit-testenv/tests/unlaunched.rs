//! Owned by W0.5.
//! The `raw_window` example started without the launcher exits 3 before it opens a window (design
//! section 7.3). On Windows it starts on a throwaway desktop of its own, so even a broken guard could
//! not show a window to the user; on Linux it gets no display at all.

use std::path::PathBuf;

const VAR: &str = "VSESVIT_TESTENV";

/// `cargo test` builds the package's examples next to its test binaries.
fn example() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe.parent().and_then(|deps| deps.parent()).unwrap();
    let path = profile_dir.join("examples").join(format!("raw_window{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "{} is missing; run `cargo test -p vsesvit-testenv`, which builds it", path.display());
    path
}

/// A well-formed `env.json` for a run that is not this one.
fn forged_env_json(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vsesvit-testenv-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("env.json");
    let token = "0123456789abcdef0123456789abcdef";
    let json = if cfg!(windows) {
        format!(r#"{{"token":"{token}","kind":"desktop","desktop":"vsesvit-test-{token}"}}"#)
    } else {
        format!(r#"{{"token":"{token}","kind":"xvfb","display":":1","xvfb_pid":1,"dbus_address":"unix:path=/nowhere"}}"#)
    };
    std::fs::write(&path, json).unwrap();
    path
}

#[cfg(unix)]
fn run(env_json: Option<&std::path::Path>) -> (Option<i32>, String) {
    let mut command = std::process::Command::new(example());
    command.env_remove(VAR).env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    if let Some(path) = env_json {
        command.env(VAR, path);
    }
    let output = command.output().unwrap();
    (output.status.code(), String::from_utf8_lossy(&output.stderr).into_owned())
}

#[cfg(windows)]
fn run(env_json: Option<&std::path::Path>) -> (Option<i32>, String) {
    let code = desktop::run_on_throwaway_desktop(&example(), env_json);
    (Some(code), String::new())
}

#[test]
fn exits_3_without_the_launcher() {
    let (code, stderr) = run(None);
    assert_eq!(code, Some(3), "stderr: {stderr}");
    if cfg!(unix) {
        assert!(stderr.contains("VSESVIT_TESTENV is not set"), "stderr: {stderr}");
    }
}

#[test]
fn exits_3_with_a_copied_env_json() {
    let path = forged_env_json("copied");
    let (code, stderr) = run(Some(&path));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    assert_eq!(code, Some(3), "stderr: {stderr}");
}

#[cfg(windows)]
mod desktop {
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, CreateDesktopW};
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess, PROCESS_INFORMATION,
        STARTUPINFOW, TerminateProcess, WaitForSingleObject,
    };

    const GENERIC_ALL: u32 = 0x1000_0000;

    fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain([0]).collect()
    }

    /// The exit code of `exe` started on a new desktop that nobody switches to, with this
    /// process's environment minus `VSESVIT_TESTENV`, set to `env_json` instead if given.
    pub fn run_on_throwaway_desktop(exe: &Path, env_json: Option<&Path>) -> i32 {
        let name = format!("vsesvit-unlaunched-{}", std::process::id());
        let desk = unsafe { CreateDesktopW(wide(&name).as_ptr(), std::ptr::null(), std::ptr::null(), 0, GENERIC_ALL, std::ptr::null()) };
        assert!(!desk.is_null(), "CreateDesktopW: {}", std::io::Error::last_os_error());

        let mut block: Vec<u16> = Vec::new();
        for (key, value) in std::env::vars_os().filter(|(k, _)| !k.eq_ignore_ascii_case(super::VAR)) {
            block.extend(key.encode_wide().chain("=".encode_utf16()).chain(value.encode_wide()).chain([0]));
        }
        if let Some(path) = env_json {
            block.extend(format!("{}={}", super::VAR, path.display()).encode_utf16().chain([0]));
        }
        block.push(0);

        let mut command = wide(format!("\"{}\"", exe.display()));
        let mut desktop = wide(format!("WinSta0\\{name}"));
        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = size_of::<STARTUPINFOW>() as u32;
        si.lpDesktop = desktop.as_mut_ptr();
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let created = unsafe {
            CreateProcessW(
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                block.as_ptr().cast(),
                std::ptr::null(),
                &si,
                &mut pi,
            )
        };
        assert!(created != 0, "CreateProcessW: {}", std::io::Error::last_os_error());
        let mut code = u32::MAX;
        unsafe {
            if WaitForSingleObject(pi.hProcess, 30_000) == WAIT_OBJECT_0 {
                GetExitCodeProcess(pi.hProcess, &mut code);
            } else {
                TerminateProcess(pi.hProcess, 1);
            }
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            CloseDesktop(desk);
        }
        code as i32
    }
}
