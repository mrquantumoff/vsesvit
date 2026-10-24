//! Owned by W0.5.
//! `cargo xtask probe raw_window`: a plain native window (an x11rb window on Linux, a plain HWND on
//! Windows) inside the test environment, which must receive one key through `PrivateInput`. Then
//! the example runs itself again as a phase, which must pass the guard in the inherited environment
//! (design section 7.3). Started any other way, `require()` exits 3 before any window.

use std::process::{Command, ExitCode};
use std::time::Duration;

const KEY_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let env = vsesvit_testenv::require();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--phase") {
        println!("raw_window: phase {} passed the guard in {}", args.get(1).map_or("?", String::as_str), env.describe());
        return ExitCode::SUCCESS;
    }
    println!("raw_window: the guard passed in {}", env.describe());
    if let Err(e) = platform::key_reaches_window(&env) {
        eprintln!("raw_window: {e}");
        return ExitCode::FAILURE;
    }
    println!("raw_window: PrivateInput delivered a key to the window");
    let phase = std::env::current_exe().and_then(|exe| Command::new(exe).args(["--phase", "restart"]).status());
    match phase {
        Ok(status) if status.success() => {
            println!("raw_window: the re-executed phase passed");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("raw_window: the re-executed phase failed: {other:?}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fmt::Display;
    use std::time::Instant;

    use x11rb::COPY_DEPTH_FROM_PARENT;
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, EventMask, InputFocus, WindowClass};

    use super::KEY_TIMEOUT;

    const XK_A: u32 = 0x61;

    fn text(e: impl Display) -> String {
        e.to_string()
    }

    pub fn key_reaches_window(env: &vsesvit_testenv::TestEnv) -> Result<(), String> {
        let (conn, screen) = x11rb::connect(None).map_err(text)?;
        let setup = conn.setup();
        let root = setup.roots[screen].root;
        let window = conn.generate_id().map_err(text)?;
        let events = CreateWindowAux::new().event_mask(EventMask::KEY_PRESS | EventMask::STRUCTURE_NOTIFY);
        conn.create_window(COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 240, 120, 0, WindowClass::INPUT_OUTPUT, 0, &events)
            .map_err(text)?;
        conn.map_window(window).map_err(text)?;
        conn.flush().map_err(text)?;
        wait(&conn, "MapNotify", |e| matches!(e, Event::MapNotify(m) if m.window == window))?;
        conn.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME).map_err(text)?.check().map_err(text)?;

        let count = setup.max_keycode - setup.min_keycode + 1;
        let map = conn.get_keyboard_mapping(setup.min_keycode, count).map_err(text)?.reply().map_err(text)?;
        let per = usize::from(map.keysyms_per_keycode.max(1));
        let index = map.keysyms.chunks(per).position(|syms| syms.contains(&XK_A)).ok_or("no keycode types 'a'")?;
        let keycode = setup.min_keycode + u8::try_from(index).map_err(text)?;

        env.input()?.key(keycode)?;
        wait(&conn, "KeyPress", |e| matches!(e, Event::KeyPress(k) if k.event == window && k.detail == keycode))
    }

    fn wait(conn: &impl Connection, what: &str, wanted: impl Fn(&Event) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + KEY_TIMEOUT;
        while Instant::now() < deadline {
            match conn.poll_for_event().map_err(text)? {
                Some(event) if wanted(&event) => return Ok(()),
                Some(_) => {}
                None => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        Err(format!("no {what} within {KEY_TIMEOUT:?}"))
    }
}

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG, PostQuitMessage,
        RegisterClassW, SW_SHOW, SetTimer, ShowWindow, TranslateMessage, WM_KEYDOWN, WM_TIMER, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };

    use super::KEY_TIMEOUT;

    const VK_A: u16 = 0x41;
    static KEY_SEEN: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            WM_KEYDOWN if wparam == usize::from(VK_A) => {
                KEY_SEEN.store(true, Ordering::SeqCst);
                unsafe { PostQuitMessage(0) };
                0
            }
            WM_TIMER => {
                unsafe { PostQuitMessage(1) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    pub fn key_reaches_window(env: &vsesvit_testenv::TestEnv) -> Result<(), String> {
        let class = wide("vsesvit-testenv-raw-window");
        let title = wide("raw_window");
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(window_proc);
            wc.hInstance = instance;
            wc.lpszClassName = class.as_ptr();
            if RegisterClassW(&wc) == 0 {
                return Err(format!("RegisterClassW: {}", std::io::Error::last_os_error()));
            }
            let null = std::ptr::null_mut();
            let hwnd = CreateWindowExW(0, class.as_ptr(), title.as_ptr(), WS_OVERLAPPEDWINDOW, 0, 0, 240, 120, null, null, instance, null);
            if hwnd.is_null() {
                return Err(format!("CreateWindowExW: {}", std::io::Error::last_os_error()));
            }
            // Shown and activated, which only the private desktop sees.
            ShowWindow(hwnd, SW_SHOW);
            SetTimer(hwnd, 1, KEY_TIMEOUT.as_millis() as u32, None);
            let posted = env.input().and_then(|input| input.post_key(hwnd, VK_A));
            if posted.is_ok() {
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, null, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            DestroyWindow(hwnd);
            posted?;
        }
        if KEY_SEEN.load(Ordering::SeqCst) { Ok(()) } else { Err(format!("no WM_KEYDOWN within {KEY_TIMEOUT:?}")) }
    }
}
