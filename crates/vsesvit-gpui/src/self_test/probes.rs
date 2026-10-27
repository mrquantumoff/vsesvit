//! Owned by W1.8.
//! What the self-test reads from the operating system rather than from the shell: this process's
//! threads, handles and child processes and their CPU time (design section 7.5's standing checks),
//! and where the native keyboard focus is (section 1.5's page-focus policy). Main thread only.

use std::sync::OnceLock;
use std::time::Duration;

use vsesvit_engine::NativeHandle;

/// A process, told apart from a later one that reuses its pid by when it started.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Process {
    pub pid: u32,
    start: u64,
}

/// What the resources check compares before and after its cycles.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub threads: usize,
    pub children: usize,
    /// Open file descriptors on Linux, kernel handles on Windows.
    pub handles: usize,
    /// Windows only: GDI and USER objects.
    pub gui: Option<(u32, u32)>,
}

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let handles = if cfg!(windows) { "handles" } else { "fds" };
        write!(f, "{} threads, {} child processes, {} {handles}", self.threads, self.children, self.handles)?;
        if let Some((gdi, user)) = self.gui {
            write!(f, ", {gdi} GDI and {user} USER objects")?;
        }
        Ok(())
    }
}

static THREADS_BEFORE_ENGINE: OnceLock<usize> = OnceLock::new();

/// Boot step 9 is about to start the engine: how many threads this process runs now. The Linux
/// sandbox wants one (design section 3.1).
pub fn note_threads_before_engine() {
    let _ = THREADS_BEFORE_ENGINE.set(threads());
}

/// What [`note_threads_before_engine`] counted, if boot called it.
pub fn threads_before_engine() -> Option<usize> {
    THREADS_BEFORE_ENGINE.get().copied()
}

pub fn usage() -> Usage {
    let me = std::process::id();
    Usage { threads: threads(), children: children_of(me).len(), handles: os::handles(), gui: os::gui_objects() }
}

/// The threads of this process.
pub fn threads() -> usize {
    os::threads()
}

/// The processes `pid` started, and theirs.
pub fn descendants(pid: u32) -> Vec<Process> {
    let table = os::table();
    let mut found: Vec<u32> = Vec::new();
    let mut parents = vec![pid];
    while let Some(parent) = parents.pop() {
        for &(child, ppid) in &table {
            if ppid == parent && child != pid && !found.contains(&child) {
                found.push(child);
                parents.push(child);
            }
        }
    }
    found.into_iter().filter_map(process).collect()
}

/// The processes `pid` started itself.
pub fn children_of(pid: u32) -> Vec<Process> {
    os::table().into_iter().filter(|&(_, ppid)| ppid == pid).filter_map(|(child, _)| process(child)).collect()
}

/// This process.
pub fn me() -> Option<Process> {
    process(std::process::id())
}

/// The running process with `pid`.
fn process(pid: u32) -> Option<Process> {
    os::start(pid).map(|start| Process { pid, start })
}

/// Whether `process` still runs; a pid reused by a newer process does not count.
pub fn alive(process: Process) -> bool {
    os::start(process.pid) == Some(process.start)
}

/// User and kernel time `process` has used so far, if it still runs.
pub fn cpu_time(process: Process) -> Option<Duration> {
    os::cpu_time(process)
}

/// The native window with the keyboard: X11's input focus, or the focus window of this thread
/// (Chromium's browser windows live on it, design section 1.6).
pub fn native_focus() -> Option<NativeHandle> {
    os::native_focus()
}

/// Whether `window` is `ancestor` or inside it.
pub fn within(window: NativeHandle, ancestor: NativeHandle) -> bool {
    os::within(window, ancestor)
}

/// Gives `window` the native keyboard focus directly, as a page's own focus call does, without the
/// shell: X11 `SetInputFocus`, Windows `SetFocus` (our own window, on our own thread).
pub fn force_native_focus(window: NativeHandle) -> Result<(), String> {
    os::force_native_focus(window)
}

/// `(x, y)` in `window`'s own physical pixels, on the screen.
pub fn to_screen(window: NativeHandle, x: i32, y: i32) -> Result<(i32, i32), String> {
    os::to_screen(window, x, y)
}

#[cfg(target_os = "linux")]
mod os {
    use std::time::Duration;

    use vsesvit_engine::NativeHandle;
    use x11rb::protocol::xproto::ConnectionExt as _;

    use super::Process;
    use crate::platform::x11::{conn, focus};

    /// Linux reports CPU time in clock ticks of 1/100 s (`USER_HZ`) on every architecture we build.
    const TICK: Duration = Duration::from_millis(10);

    /// The fields of `/proc/<pid>/stat` after the command name: state, ppid, … utime is the 12th.
    struct Stat {
        zombie: bool,
        ppid: u32,
        cpu_ticks: u64,
        start: u64,
    }

    fn stat(pid: u32) -> Option<Stat> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = &text[text.rfind(')')? + 1..];
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let field = |i: usize| fields.get(i)?.parse::<u64>().ok();
        Some(Stat {
            zombie: fields.first() == Some(&"Z"),
            ppid: u32::try_from(field(1)?).ok()?,
            cpu_ticks: field(11)? + field(12)?,
            start: field(19)?,
        })
    }

    /// Every process with its parent.
    pub fn table() -> Vec<(u32, u32)> {
        let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
        dir.filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| stat(pid).map(|s| (pid, s.ppid)))
            .collect()
    }

    /// When `pid` started, while it runs; an exited process waiting to be reaped does not.
    pub fn start(pid: u32) -> Option<u64> {
        stat(pid).filter(|s| !s.zombie).map(|s| s.start)
    }

    pub fn cpu_time(process: Process) -> Option<Duration> {
        stat(process.pid).filter(|s| s.start == process.start).map(|s| TICK * u32::try_from(s.cpu_ticks).unwrap_or(u32::MAX))
    }

    pub fn threads() -> usize {
        std::fs::read_dir("/proc/self/task").map_or(0, Iterator::count)
    }

    pub fn handles() -> usize {
        std::fs::read_dir("/proc/self/fd").map_or(0, Iterator::count)
    }

    pub fn gui_objects() -> Option<(u32, u32)> {
        None
    }

    pub fn native_focus() -> Option<NativeHandle> {
        let x = conn().ok()?;
        let focused = focus::current(&x);
        (focused > 1).then(|| NativeHandle::from_xid(focused))
    }

    pub fn within(window: NativeHandle, ancestor: NativeHandle) -> bool {
        conn().is_ok_and(|x| focus::is_within(&x, window.xid(), ancestor.xid()))
    }

    pub fn force_native_focus(window: NativeHandle) -> Result<(), String> {
        let x = conn().map_err(|e| e.to_string())?;
        focus::set(&x, window.xid())
    }

    pub fn to_screen(window: NativeHandle, x: i32, y: i32) -> Result<(i32, i32), String> {
        let c = conn().map_err(|e| e.to_string())?;
        let (x, y) = (i16::try_from(x).map_err(|e| e.to_string())?, i16::try_from(y).map_err(|e| e.to_string())?);
        let reply = c
            .conn
            .translate_coordinates(window.xid(), c.root, x, y)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        Ok((i32::from(reply.dst_x), i32::from(reply.dst_y)))
    }

    /// The keycode the private server's keymap gives `keysym`.
    pub fn keycode(keysym: u32) -> Result<u8, String> {
        use x11rb::connection::Connection as _;
        let x = conn().map_err(|e| e.to_string())?;
        let setup = x.conn.setup();
        let count = setup.max_keycode - setup.min_keycode + 1;
        let map = x.conn.get_keyboard_mapping(setup.min_keycode, count).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let per = usize::from(map.keysyms_per_keycode.max(1));
        let index = map.keysyms.chunks(per).position(|syms| syms.first() == Some(&keysym)).ok_or(format!("no keycode types keysym {keysym:#x}"))?;
        Ok(setup.min_keycode + u8::try_from(index).map_err(|e| e.to_string())?)
    }
}

#[cfg(windows)]
mod os {
    use std::time::Duration;

    use vsesvit_engine::NativeHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HWND, INVALID_HANDLE_VALUE, POINT, STILL_ACTIVE};
    use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetExitCodeProcess, GetGuiResources, GetProcessHandleCount, GetProcessTimes, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, ChildWindowFromPointEx, GetWindowThreadProcessId, IsChild,
    };

    use super::Process;
    use crate::platform::windows::{handle, hwnd};

    /// Every process with its parent and thread count.
    fn snapshot() -> Vec<(u32, u32, u32)> {
        let mut out = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snap, &mut entry) != 0;
            while more {
                out.push((entry.th32ProcessID, entry.th32ParentProcessID, entry.cntThreads));
                more = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        out
    }

    fn ticks(time: FILETIME) -> u64 {
        u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)
    }

    /// Creation time and CPU time (100 ns units) of `pid`, while it runs and this user may ask. An
    /// exited process stays queryable as long as anyone holds a handle to it.
    fn times(pid: u32) -> Option<(u64, u64)> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
            let mut code = 0u32;
            let running = GetExitCodeProcess(process, &mut code) != 0 && code == STILL_ACTIVE as u32;
            let ok = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0;
            CloseHandle(process);
            (running && ok).then(|| (ticks(created), ticks(kernel) + ticks(user)))
        }
    }

    /// Every process with its parent.
    pub fn table() -> Vec<(u32, u32)> {
        snapshot().into_iter().map(|(pid, ppid, _)| (pid, ppid)).collect()
    }

    /// When `pid` started, while it runs and this user may ask.
    pub fn start(pid: u32) -> Option<u64> {
        times(pid).map(|(start, _)| start)
    }

    pub fn cpu_time(process: Process) -> Option<Duration> {
        let (start, cpu) = times(process.pid)?;
        (start == process.start).then(|| Duration::from_nanos(cpu * 100))
    }

    pub fn threads() -> usize {
        let me = std::process::id();
        snapshot().into_iter().find(|(pid, _, _)| *pid == me).map_or(0, |(_, _, threads)| threads as usize)
    }

    pub fn handles() -> usize {
        let mut count = 0u32;
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
        count as usize
    }

    pub fn gui_objects() -> Option<(u32, u32)> {
        unsafe {
            let me = GetCurrentProcess();
            Some((GetGuiResources(me, GR_GDIOBJECTS), GetGuiResources(me, GR_USEROBJECTS)))
        }
    }

    pub fn native_focus() -> Option<NativeHandle> {
        let focused = unsafe { GetFocus() };
        (!focused.is_null()).then(|| handle(focused))
    }

    pub fn within(window: NativeHandle, ancestor: NativeHandle) -> bool {
        window == ancestor || unsafe { IsChild(hwnd(ancestor), hwnd(window)) != 0 }
    }

    pub fn force_native_focus(window: NativeHandle) -> Result<(), String> {
        let target = hwnd(window);
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(target, &mut owner) };
        if owner != std::process::id() {
            return Err(format!("window {target:?} is not ours"));
        }
        unsafe { SetFocus(target) };
        Ok(())
    }

    pub fn to_screen(window: NativeHandle, x: i32, y: i32) -> Result<(i32, i32), String> {
        let mut point = POINT { x, y };
        if unsafe { ClientToScreen(hwnd(window), &mut point) } == 0 {
            return Err(format!("ClientToScreen: {}", std::io::Error::last_os_error()));
        }
        Ok((point.x, point.y))
    }

    /// The window a real click at screen point (`x`, `y`) reaches inside `window`, the deepest shown
    /// child under it, with the point in that child's client pixels.
    pub fn target_at(window: NativeHandle, x: i32, y: i32) -> (HWND, i32, i32) {
        let mut target = hwnd(window);
        loop {
            let mut point = POINT { x, y };
            unsafe { ScreenToClient(target, &mut point) };
            let child = unsafe { ChildWindowFromPointEx(target, point, CWP_SKIPINVISIBLE | CWP_SKIPTRANSPARENT) };
            if child.is_null() || child == target {
                return (target, point.x, point.y);
            }
            target = child;
        }
    }
}

#[cfg(target_os = "linux")]
pub use os::keycode;
#[cfg(windows)]
pub use os::target_at;
