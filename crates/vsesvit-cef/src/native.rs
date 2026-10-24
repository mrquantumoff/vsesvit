//! Owned by W1.3.
//! Native window parameters per OS: `window_info_child` for a browser created into a parent handle, the
//! same fix-up for a popup's `WindowInfo` out-parameter, and the conversions between the contract's
//! `NativeHandle` and CEF's window handle. CEF wants a child's bounds in DIP on Linux (SC section 5.3)
//! and in physical pixels on Windows.

use cef::{Rect, RuntimeStyle, WindowInfo, sys};
use vsesvit_engine::{NativeHandle, PhysRect};

use crate::registry::Style;

/// The bounds CEF wants for a child at `rect` (physical pixels) under a parent of `scale`.
pub fn child_bounds(rect: PhysRect, scale: f32, linux: bool) -> Rect {
    let scale = if linux && scale > 0.0 { scale } else { 1.0 };
    let dip = |v: f64| (v / scale as f64).round() as i32;
    Rect {
        x: dip(rect.x as f64),
        y: dip(rect.y as f64),
        width: dip(rect.width as f64).max(1),
        height: dip(rect.height as f64).max(1),
    }
}

fn runtime(style: Style) -> RuntimeStyle {
    match style {
        Style::Chrome => RuntimeStyle::CHROME,
        Style::Alloy => RuntimeStyle::ALLOY,
    }
}

/// CEF's handle for a contract handle.
pub fn cef_handle(handle: NativeHandle) -> sys::cef_window_handle_t {
    #[cfg(windows)]
    return sys::HWND(handle.hwnd().cast());
    #[cfg(not(windows))]
    return handle.raw() as sys::cef_window_handle_t;
}

/// The contract handle for a CEF handle.
pub fn native_handle(handle: sys::cef_window_handle_t) -> NativeHandle {
    #[cfg(windows)]
    return NativeHandle::from_hwnd(handle.0.cast());
    #[cfg(not(windows))]
    return NativeHandle::from_raw(handle as u64);
}

/// A windowed child of `parent` at `rect`.
pub fn window_info_child(parent: NativeHandle, rect: PhysRect, scale: f32, style: Style) -> WindowInfo {
    WindowInfo { runtime_style: runtime(style), ..Default::default() }
        .set_as_child(cef_handle(parent), &child_bounds(rect, scale, cfg!(target_os = "linux")))
}

/// Re-parents a popup into the shell (`on_before_popup`'s `WindowInfo`). Field by field: replacing the
/// struct would also write back `window_name`, which the out-parameter glue empties (G3).
pub fn fix_popup(info: &mut WindowInfo, parent: NativeHandle, rect: PhysRect, scale: f32) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_TABSTOP, WS_VISIBLE};
        info.style = WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS | WS_TABSTOP | WS_VISIBLE;
        info.ex_style = 0;
    }
    info.parent_window = cef_handle(parent);
    info.bounds = child_bounds(rect, scale, cfg!(target_os = "linux"));
    info.runtime_style = RuntimeStyle::CHROME;
    info.windowless_rendering_enabled = 0;
}

/// A top-level window of its own, for DevTools.
pub fn window_info_toplevel() -> WindowInfo {
    #[cfg(windows)]
    return WindowInfo { runtime_style: RuntimeStyle::CHROME, ..Default::default() }
        .set_as_popup(sys::HWND(std::ptr::null_mut()), "DevTools");
    #[cfg(not(windows))]
    return WindowInfo { runtime_style: RuntimeStyle::CHROME, ..Default::default() };
}

/// Destroys an Alloy browser's own window (Windows `do_close`; spike-ext-win decision 4).
#[cfg(windows)]
pub fn destroy_window(handle: NativeHandle) {
    // SAFETY: a window this process created; DestroyWindow fails harmlessly if it is already gone.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(handle.hwnd()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_children_are_placed_in_dip_windows_in_pixels() {
        let rect = PhysRect { x: 30, y: 90, width: 1500, height: 900 };
        let dip = child_bounds(rect, 1.5, true);
        assert_eq!((dip.x, dip.y, dip.width, dip.height), (20, 60, 1000, 600));
        let px = child_bounds(rect, 1.5, false);
        assert_eq!((px.x, px.y, px.width, px.height), (30, 90, 1500, 900));
        let empty = child_bounds(PhysRect::default(), 2.0, true);
        assert_eq!((empty.width, empty.height), (1, 1), "CEF refuses a zero-sized child");
        let bad_scale = child_bounds(rect, 0.0, true);
        assert_eq!(bad_scale.width, 1500);
    }

    #[test]
    fn handles_round_trip() {
        let h = NativeHandle::from_raw(0x0420_0001);
        assert_eq!(native_handle(cef_handle(h)), h);
    }
}
