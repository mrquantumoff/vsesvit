//! Win32 side of the shell: content host window, geometry, show/hide, window regions,
//! keyboard focus, in-process screenshots, window tree dumps, and message injection
//! into windows this process owns. Nothing here reaches another process's windows.
use anyhow::{Result, anyhow};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::System::SystemServices::MK_LBUTTON;
use windows_sys::Win32::System::Threading::{GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources, GetProcessHandleCount};
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// An HWND as an integer, so it can live in atomics and logs.
pub type Hwnd = isize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

fn h(hwnd: Hwnd) -> HWND {
    hwnd as *mut _
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const HOST_CLASS: &str = "VsesvitSpikeHost";
static HOST_CLASS_ONCE: Once = Once::new();

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Create the content host: a plain child window between GPUI's toplevel and the CEF
/// children. Solid background so it is visible before the first tab maps.
pub fn create_host(parent: Hwnd, r: PRect) -> Result<Hwnd> {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide(HOST_CLASS);
        HOST_CLASS_ONCE.call_once(|| {
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(host_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: CreateSolidBrush(0x37291f), // COLORREF is 0x00BBGGRR: #1f2937
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc);
        });
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS | WS_VISIBLE,
            r.x,
            r.y,
            r.w.max(1),
            r.h.max(1),
            h(parent),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(anyhow!("CreateWindowExW(host) failed"));
        }
        Ok(hwnd as Hwnd)
    }
}

pub fn set_rect(hwnd: Hwnd, r: PRect) {
    unsafe {
        SetWindowPos(h(hwnd), std::ptr::null_mut(), r.x, r.y, r.w.max(1), r.h.max(1), SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn show(hwnd: Hwnd, visible: bool) {
    unsafe {
        ShowWindow(h(hwnd), if visible { SW_SHOWNA } else { SW_HIDE });
    }
}

pub fn is_visible(hwnd: Hwnd) -> bool {
    unsafe { IsWindowVisible(h(hwnd)) != 0 }
}

pub fn is_window(hwnd: Hwnd) -> bool {
    unsafe { IsWindow(h(hwnd)) != 0 }
}

/// Window rectangle in screen physical pixels.
pub fn window_rect(hwnd: Hwnd) -> PRect {
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(h(hwnd), &mut r);
        PRect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    }
}

/// Client rectangle size in physical pixels.
pub fn client_size(hwnd: Hwnd) -> (i32, i32) {
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(h(hwnd), &mut r);
        (r.right - r.left, r.bottom - r.top)
    }
}

/// Screen position of the client origin.
pub fn client_origin(hwnd: Hwnd) -> (i32, i32) {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        ClientToScreen(h(hwnd), &mut p);
        (p.x, p.y)
    }
}

/// Move a toplevel without activating or resizing it (used to park the spike off-screen).
pub fn move_to(hwnd: Hwnd, x: i32, y: i32) {
    unsafe {
        SetWindowPos(h(hwnd), std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn set_size(hwnd: Hwnd, w: i32, h_: i32) {
    unsafe {
        SetWindowPos(h(hwnd), std::ptr::null_mut(), 0, 0, w, h_, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn show_noactivate(hwnd: Hwnd) {
    unsafe {
        ShowWindow(h(hwnd), SW_SHOWNOACTIVATE);
    }
}

/// Cut `holes` out of `hwnd` (window-relative physical px). Children are clipped too.
pub fn region_exclude(hwnd: Hwnd, full: (i32, i32), holes: &[PRect]) {
    unsafe {
        let rgn = CreateRectRgn(0, 0, full.0, full.1);
        for hole in holes {
            let hr = CreateRectRgn(hole.x, hole.y, hole.x + hole.w, hole.y + hole.h);
            CombineRgn(rgn, rgn, hr, RGN_DIFF);
            DeleteObject(hr as _);
        }
        SetWindowRgn(h(hwnd), rgn, 1); // the system owns rgn from here on
    }
}

pub fn region_reset(hwnd: Hwnd) {
    unsafe {
        SetWindowRgn(h(hwnd), std::ptr::null_mut(), 1);
    }
}

// ---------------------------------------------------------------- desktop guard

static SAFE_DESKTOP: AtomicBool = AtomicBool::new(false);

fn desktop_name(desk: windows_sys::Win32::System::StationsAndDesktops::HDESK) -> Option<String> {
    use windows_sys::Win32::System::StationsAndDesktops::*;
    if desk.is_null() {
        return None;
    }
    let mut buf = [0u16; 256];
    let mut need = 0u32;
    let ok = unsafe { GetUserObjectInformationW(desk as _, UOI_NAME, buf.as_mut_ptr() as _, (buf.len() * 2) as u32, &mut need) };
    (ok != 0).then(|| String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)]))
}

/// Decide once, at startup, whether this process may activate or focus its windows:
/// only when its thread desktop is not the input desktop (the one the user sees).
/// When the input desktop cannot be read, assume it is ours. Returns (ours, input, safe).
pub fn init_desktop_guard() -> (String, String, bool) {
    use windows_sys::Win32::System::StationsAndDesktops::*;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    let ours = desktop_name(unsafe { GetThreadDesktop(GetCurrentThreadId()) }).unwrap_or_default();
    let input_h = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    let input = desktop_name(input_h);
    if !input_h.is_null() {
        unsafe { CloseDesktop(input_h) };
    }
    // SPIKE_STRICT=1 applies the user-desktop policy on a hidden desktop too.
    let strict = std::env::var("SPIKE_STRICT").is_ok_and(|v| v == "1");
    let safe = !strict && matches!(&input, Some(i) if !ours.is_empty() && !i.eq_ignore_ascii_case(&ours));
    SAFE_DESKTOP.store(safe, Ordering::SeqCst);
    (ours, input.unwrap_or_else(|| "?".into()), safe)
}

/// True when the spike runs on a desktop the user cannot see (see `init_desktop_guard`).
pub fn may_activate() -> bool {
    SAFE_DESKTOP.load(Ordering::SeqCst)
}

/// A reason the spike could be disturbing the user, or None: our process owns the
/// foreground window, or (on the input desktop) one of our toplevels is on-screen.
pub fn safety_violation() -> Option<String> {
    let fg = foreground_window();
    if fg != 0 && thread_of(fg).1 == std::process::id() {
        return Some(format!("foreground window 0x{fg:x} belongs to the spike"));
    }
    if may_activate() {
        return None;
    }
    let (vx, vy, vw, vh) = unsafe {
        (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN))
    };
    for t in process_toplevels() {
        let r = window_rect(t);
        if is_visible(t) && r.x < vx + vw && r.x + r.w > vx && r.y < vy + vh && r.y + r.h > vy && r.w > 1 && r.h > 1 {
            return Some(format!("visible toplevel on-screen: {}", describe(t)));
        }
    }
    None
}

/// The deepest visible descendant of `top` containing the client point (physical px),
/// walking ChildWindowFromPointEx the way the system routes a mouse press. Read-only.
pub fn deepest_child_at(top: Hwnd, x: i32, y: i32) -> Hwnd {
    unsafe {
        let mut cur = h(top);
        let mut p = POINT { x, y };
        loop {
            let c = ChildWindowFromPointEx(cur, p, CWP_SKIPINVISIBLE | CWP_SKIPTRANSPARENT);
            if c.is_null() || c == cur {
                return cur as Hwnd;
            }
            MapWindowPoints(cur, c, &mut p, 1);
            cur = c;
        }
    }
}

/// WindowFromPoint for a client point of `top` (physical px): the system's own hit test,
/// which honours window regions. Read-only.
pub fn window_from_client_point(top: Hwnd, x: i32, y: i32) -> Hwnd {
    unsafe {
        let mut p = POINT { x, y };
        ClientToScreen(h(top), &mut p);
        WindowFromPoint(p) as Hwnd
    }
}

/// Dispatch this thread's Win32 messages for `d` (after GPUI's loop has returned).
pub fn drain_messages(d: std::time::Duration) -> usize {
    let end = std::time::Instant::now() + d;
    let mut n = 0;
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while std::time::Instant::now() < end {
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                n += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    n
}

pub fn root_of(hwnd: Hwnd) -> Hwnd {
    unsafe { GetAncestor(h(hwnd), GA_ROOT) as Hwnd }
}

pub fn focus() -> Hwnd {
    unsafe { GetFocus() as Hwnd }
}

/// SetFocus, refused when it could activate a toplevel on the user's desktop: there
/// SetFocus on an inactive toplevel activates it and can make it the foreground window.
pub fn set_focus(hwnd: Hwnd) -> Hwnd {
    if !may_activate() && (active_window() == 0 || active_window() != root_of(hwnd)) {
        crate::log(&format!("focus: refused SetFocus(0x{hwnd:x}): input desktop and our toplevel is not active"));
        return 0;
    }
    unsafe { SetFocus(h(hwnd)) as Hwnd }
}

pub fn active_window() -> Hwnd {
    unsafe { GetActiveWindow() as Hwnd }
}

/// Read-only: which window the system considers foreground. Never set by this spike.
pub fn foreground_window() -> Hwnd {
    unsafe { GetForegroundWindow() as Hwnd }
}

pub fn is_within(hwnd: Hwnd, ancestor: Hwnd) -> bool {
    hwnd == ancestor || unsafe { IsChild(h(ancestor), h(hwnd)) != 0 }
}

pub fn parent(hwnd: Hwnd) -> Hwnd {
    unsafe { GetParent(h(hwnd)) as Hwnd }
}

pub fn owner(hwnd: Hwnd) -> Hwnd {
    unsafe { GetWindow(h(hwnd), GW_OWNER) as Hwnd }
}

pub fn set_owner(hwnd: Hwnd, owner: Hwnd) {
    unsafe {
        SetWindowLongPtrW(h(hwnd), GWLP_HWNDPARENT, owner);
    }
}

pub fn ex_style(hwnd: Hwnd) -> u32 {
    unsafe { GetWindowLongPtrW(h(hwnd), GWL_EXSTYLE) as u32 }
}

pub fn style(hwnd: Hwnd) -> u32 {
    unsafe { GetWindowLongPtrW(h(hwnd), GWL_STYLE) as u32 }
}

pub fn set_topmost(hwnd: Hwnd, topmost: bool) {
    unsafe {
        let after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
        SetWindowPos(h(hwnd), after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn class_name(hwnd: Hwnd) -> String {
    unsafe {
        let mut buf = [0u16; 128];
        let n = GetClassNameW(h(hwnd), buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

pub fn title(hwnd: Hwnd) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(h(hwnd), buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

pub fn thread_of(hwnd: Hwnd) -> (u32, u32) {
    unsafe {
        let mut pid = 0u32;
        let tid = GetWindowThreadProcessId(h(hwnd), &mut pid);
        (tid, pid)
    }
}

pub fn dpi_for_window(hwnd: Hwnd) -> u32 {
    unsafe { GetDpiForWindow(h(hwnd)) }
}

/// Process DPI awareness: "pmv2" (default) or "unaware" (SPIKE_DPI), set before CEF or
/// GPUI create anything. Chromium sets per-monitor v2 itself if nothing was set.
pub fn set_dpi_awareness(mode: &str) -> bool {
    unsafe {
        let ctx = match mode {
            "unaware" => DPI_AWARENESS_CONTEXT_UNAWARE,
            "system" => DPI_AWARENESS_CONTEXT_SYSTEM_AWARE,
            _ => DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        SetProcessDpiAwarenessContext(ctx) != 0
    }
}

/// This thread's DPI awareness: 0 unaware, 1 system, 2 per-monitor (v1 or v2).
pub fn dpi_awareness() -> i32 {
    unsafe { GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext()) }
}

/// Direct children of `hwnd`, in z-order (top first).
pub fn children(hwnd: Hwnd) -> Vec<Hwnd> {
    let mut out = Vec::new();
    unsafe {
        let mut c = GetWindow(h(hwnd), GW_CHILD);
        while !c.is_null() {
            out.push(c as Hwnd);
            c = GetWindow(c, GW_HWNDNEXT);
        }
    }
    out
}

pub fn describe(hwnd: Hwnd) -> String {
    let r = window_rect(hwnd);
    let (tid, pid) = thread_of(hwnd);
    format!(
        "0x{hwnd:x} {:?} {:?} {}x{}+{}+{} {} tid={tid} pid={pid} style=0x{:x} ex=0x{:x}",
        class_name(hwnd),
        title(hwnd),
        r.w,
        r.h,
        r.x,
        r.y,
        if is_visible(hwnd) { "visible" } else { "hidden" },
        style(hwnd),
        ex_style(hwnd)
    )
}

pub fn tree(hwnd: Hwnd, depth: usize, out: &mut Vec<String>) {
    out.push(format!("{}{}", "  ".repeat(depth), describe(hwnd)));
    for c in children(hwnd) {
        tree(c, depth + 1, out);
    }
}

pub fn count_descendants(hwnd: Hwnd) -> usize {
    children(hwnd).into_iter().map(|c| 1 + count_descendants(c)).sum()
}

/// All toplevel windows owned by this process's threads, for leak checks and for
/// listing Chrome-native windows that appear next to ours.
pub fn process_toplevels() -> Vec<Hwnd> {
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> i32 {
        let out = unsafe { &mut *(lp as *mut Vec<Hwnd>) };
        let (_, pid) = thread_of(hwnd as Hwnd);
        if pid == std::process::id() {
            out.push(hwnd as Hwnd);
        }
        1
    }
    let mut out: Vec<Hwnd> = Vec::new();
    unsafe {
        EnumWindows(Some(cb), &mut out as *mut _ as LPARAM);
    }
    out
}

pub struct Resources {
    pub handles: u32,
    pub gdi: u32,
    pub user: u32,
}

pub fn resources() -> Resources {
    unsafe {
        let mut handles = 0u32;
        GetProcessHandleCount(GetCurrentProcess(), &mut handles);
        Resources { handles, gdi: GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS), user: GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS) }
    }
}

// ---------------------------------------------------------------- screenshots

pub const PW_FULL: u32 = 2; // PW_RENDERFULLCONTENT (undocumented, used by DWM-aware capture)

/// Capture `hwnd` with PrintWindow into RGBA. `flags`: 0 = classic, PW_FULL = DWM content.
pub fn capture(hwnd: Hwnd, flags: u32) -> Result<(u32, u32, Vec<u8>)> {
    unsafe {
        let r = window_rect(hwnd);
        let (w, hh) = (r.w.max(1), r.h.max(1));
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -hh,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bmp.is_null() {
            DeleteDC(hdc);
            return Err(anyhow!("CreateDIBSection failed"));
        }
        let old = SelectObject(hdc, bmp as _);
        let ok = PrintWindow(h(hwnd), hdc, flags);
        GdiFlush();
        let n = (w * hh * 4) as usize;
        let src = std::slice::from_raw_parts(bits as *const u8, n);
        let mut rgba = vec![0u8; n];
        for (d, s) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = 255;
        }
        SelectObject(hdc, old);
        DeleteObject(bmp as _);
        DeleteDC(hdc);
        if ok == 0 {
            return Err(anyhow!("PrintWindow returned 0"));
        }
        Ok((w as u32, hh as u32, rgba))
    }
}

pub fn write_png(path: &str, w: u32, h_: u32, rgba: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h_);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Fraction of pixels in `rect` (image-relative) that are not the given colour.
pub fn fraction_not(rgba: &[u8], w: u32, rect: PRect, color: (u8, u8, u8)) -> f32 {
    let mut n = 0u32;
    let mut diff = 0u32;
    for y in rect.y.max(0)..(rect.y + rect.h) {
        for x in rect.x.max(0)..(rect.x + rect.w) {
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            if i + 3 >= rgba.len() {
                continue;
            }
            n += 1;
            let p = (rgba[i], rgba[i + 1], rgba[i + 2]);
            if (p.0 as i32 - color.0 as i32).abs() > 8 || (p.1 as i32 - color.1 as i32).abs() > 8 || (p.2 as i32 - color.2 as i32).abs() > 8 {
                diff += 1;
            }
        }
    }
    if n == 0 { 0. } else { diff as f32 / n as f32 }
}

pub fn pixel(rgba: &[u8], w: u32, x: i32, y: i32) -> (u8, u8, u8) {
    let i = ((y.max(0) as u32 * w + x.max(0) as u32) * 4) as usize;
    if i + 2 < rgba.len() { (rgba[i], rgba[i + 1], rgba[i + 2]) } else { (0, 0, 0) }
}

// ---------------------------------------------------------------- input into our own windows

/// Modifier state for this thread only (`SetKeyboardState` is per message queue), so
/// `GetKeyState(VK_CONTROL)` reads as pressed while we dispatch a synthetic key. Nothing
/// reaches other processes.
pub struct ModGuard {
    saved: [u8; 256],
}

impl ModGuard {
    pub fn press(ctrl: bool, shift: bool, alt: bool) -> Self {
        unsafe {
            let mut saved = [0u8; 256];
            GetKeyboardState(saved.as_mut_ptr());
            let mut st = saved;
            let set = |st: &mut [u8; 256], vk: u16, on: bool| {
                st[vk as usize] = if on { 0x80 } else { 0 };
            };
            set(&mut st, VK_CONTROL, ctrl);
            set(&mut st, VK_LCONTROL, ctrl);
            set(&mut st, VK_SHIFT, shift);
            set(&mut st, VK_LSHIFT, shift);
            set(&mut st, VK_MENU, alt);
            set(&mut st, VK_LMENU, alt);
            SetKeyboardState(st.as_ptr());
            Self { saved }
        }
    }
}

impl Drop for ModGuard {
    fn drop(&mut self) {
        unsafe {
            SetKeyboardState(self.saved.as_ptr());
        }
    }
}

fn key_lparam(vk: u16, down: bool, alt: bool) -> isize {
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as isize;
    let extended = matches!(vk, VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_INSERT | VK_DELETE | VK_RCONTROL | VK_RMENU);
    let mut lp = 1isize | (scan << 16);
    if extended {
        lp |= 1 << 24;
    }
    if alt {
        lp |= 1 << 29;
    }
    if !down {
        lp |= (1 << 30) | (1 << 31);
    }
    lp
}

/// Post a key press (modifiers, down, up) to one of our own windows. Unlike `send_key`
/// the messages go through the thread's queue, so they meet GPUI's message loop
/// (`translate_accelerator`) and `TranslateMessage` exactly like typed keys do. The
/// returned guard holds the modifier state; drop it after the messages are processed.
pub fn post_key(hwnd: Hwnd, vk: u16, ctrl: bool, shift: bool, alt: bool) -> ModGuard {
    let g = ModGuard::press(ctrl, shift, alt);
    unsafe {
        let (down, up) = if alt { (WM_SYSKEYDOWN, WM_SYSKEYUP) } else { (WM_KEYDOWN, WM_KEYUP) };
        for mvk in [(ctrl, VK_CONTROL), (shift, VK_SHIFT), (alt, VK_MENU)] {
            if mvk.0 {
                PostMessageW(h(hwnd), if mvk.1 == VK_MENU { WM_SYSKEYDOWN } else { WM_KEYDOWN }, mvk.1 as usize, key_lparam(mvk.1, true, alt) as LPARAM);
            }
        }
        PostMessageW(h(hwnd), down, vk as usize, key_lparam(vk, true, alt) as LPARAM);
        PostMessageW(h(hwnd), up, vk as usize, key_lparam(vk, false, alt) as LPARAM);
        for mvk in [(alt, VK_MENU), (shift, VK_SHIFT), (ctrl, VK_CONTROL)] {
            if mvk.0 {
                PostMessageW(h(hwnd), if mvk.1 == VK_MENU { WM_SYSKEYUP } else { WM_KEYUP }, mvk.1 as usize, key_lparam(mvk.1, false, alt) as LPARAM);
            }
        }
    }
    g
}

/// A plain, non-topmost toplevel of this process standing in for "another app's window"
/// in z-order checks. Created off-screen; activated only where `may_activate`.
pub fn create_other_toplevel(x: i32, y: i32) -> Hwnd {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("VsesvitSpikeOther");
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(host_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: CreateSolidBrush(0x00ff00),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc);
        });
        let hwnd = CreateWindowExW(0, class.as_ptr(), wide("other app").as_ptr(), WS_OVERLAPPEDWINDOW, x, y, 600, 400, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
        if may_activate() {
            ShowWindow(hwnd, SW_SHOW);
            SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        } else {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        hwnd as Hwnd
    }
}

/// Z-order position of each of `hwnds` among all toplevels of this desktop (0 = top).
pub fn z_positions(hwnds: &[Hwnd]) -> Vec<Option<usize>> {
    let mut order = Vec::new();
    unsafe {
        let mut w = GetTopWindow(std::ptr::null_mut());
        while !w.is_null() {
            order.push(w as Hwnd);
            w = GetWindow(w, GW_HWNDNEXT);
        }
    }
    hwnds.iter().map(|x| order.iter().position(|o| o == x)).collect()
}

pub fn destroy(hwnd: Hwnd) {
    unsafe {
        DestroyWindow(h(hwnd));
    }
}

/// Dispatch a key press (down + char + up) synchronously to one of our own windows.
pub fn send_key(hwnd: Hwnd, vk: u16, ctrl: bool, shift: bool, alt: bool) {
    let _m = ModGuard::press(ctrl, shift, alt);
    unsafe {
        let (down, up) = if alt { (WM_SYSKEYDOWN, WM_SYSKEYUP) } else { (WM_KEYDOWN, WM_KEYUP) };
        for mvk in [(ctrl, VK_CONTROL), (shift, VK_SHIFT), (alt, VK_MENU)] {
            if mvk.0 {
                SendMessageW(h(hwnd), if mvk.1 == VK_MENU { WM_SYSKEYDOWN } else { WM_KEYDOWN }, mvk.1 as usize, key_lparam(mvk.1, true, alt) as LPARAM);
            }
        }
        SendMessageW(h(hwnd), down, vk as usize, key_lparam(vk, true, alt) as LPARAM);
        if !ctrl && !alt {
            let ch = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_CHAR);
            if ch != 0 && vk != VK_RETURN && vk != VK_TAB && vk != VK_ESCAPE && vk != VK_BACK {
                let c = if shift { (ch as u8).to_ascii_uppercase() as u32 } else { (ch as u8).to_ascii_lowercase() as u32 };
                SendMessageW(h(hwnd), WM_CHAR, c as usize, key_lparam(vk, true, alt) as LPARAM);
            }
        }
        SendMessageW(h(hwnd), up, vk as usize, key_lparam(vk, false, alt) as LPARAM);
        for mvk in [(alt, VK_MENU), (shift, VK_SHIFT), (ctrl, VK_CONTROL)] {
            if mvk.0 {
                SendMessageW(h(hwnd), if mvk.1 == VK_MENU { WM_SYSKEYUP } else { WM_KEYUP }, mvk.1 as usize, key_lparam(mvk.1, false, alt) as LPARAM);
            }
        }
    }
}

/// Left click at client coordinates (physical px) of one of our own windows.
pub fn send_click(hwnd: Hwnd, x: i32, y: i32) {
    unsafe {
        let lp = ((y as isize) << 16) | (x as isize & 0xffff);
        SendMessageW(h(hwnd), WM_MOUSEMOVE, 0, lp as LPARAM);
        SendMessageW(h(hwnd), WM_LBUTTONDOWN, MK_LBUTTON as usize, lp as LPARAM);
        SendMessageW(h(hwnd), WM_LBUTTONUP, 0, lp as LPARAM);
    }
}

/// Ask a toplevel to close the way its close button does.
pub fn post_close(hwnd: Hwnd) {
    unsafe {
        PostMessageW(h(hwnd), WM_CLOSE, 0, 0);
    }
}

/// Virtual-key code for a key name used by the control channel.
pub fn vk_from_name(name: &str) -> Option<u16> {
    Some(match name.to_ascii_lowercase().as_str() {
        "tab" => VK_TAB,
        "enter" | "return" => VK_RETURN,
        "esc" | "escape" => VK_ESCAPE,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "f4" => VK_F4,
        "f5" => VK_F5,
        "f6" => VK_F6,
        "f11" => VK_F11,
        "f12" => VK_F12,
        s if s.len() == 1 => s.as_bytes()[0].to_ascii_uppercase() as u16,
        _ => return None,
    })
}

/// Put a child window at the top of its siblings' z-order, without activation.
pub fn raise(hwnd: Hwnd) {
    unsafe {
        SetWindowPos(h(hwnd), HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}
