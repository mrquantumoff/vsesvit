//! Owned by W1.2.
//! Where the content host meets the window frame. GPUI's own `WM_NCHITTEST` answers the caption
//! buttons (`WindowControlArea::{Close, Max, Min}`; `HTMAXBUTTON` is what opens Windows 11's snap
//! layouts), the drag area, and, for a window that draws its own title bar, a resize band at the top of
//! the client area. The system sends that message only to the window under the pointer, so a child over
//! the band would take resizing away. The content host is therefore clipped out of the band. The other
//! three edges are non-client area, which no child can cover.

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetClientRect, GetWindowLongPtrW, GetWindowRect, IsZoomed, SM_CXPADDEDBORDER, SM_CYSIZEFRAME,
    WS_THICKFRAME,
};

use super::PhysRect;

/// The rows at the top of `window`'s client area where GPUI answers a resize edge, in client pixels.
/// None when GPUI answers none there: the window is maximized, fullscreen (no `WS_THICKFRAME`), or
/// the system draws its title bar (the client area starts below the frame).
pub(crate) fn resize_band(window: HWND) -> Option<PhysRect> {
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
        if style & WS_THICKFRAME == 0 || IsZoomed(window) != 0 {
            return None;
        }
        let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut client = frame;
        let mut origin = POINT { x: 0, y: 0 };
        if GetWindowRect(window, &mut frame) == 0
            || GetClientRect(window, &mut client) == 0
            || ClientToScreen(window, &mut origin) == 0
        {
            return None;
        }
        if origin.y != frame.top {
            return None;
        }
        let dpi = GetDpiForWindow(window);
        // GPUI's `handle_hit_test_msg`: `0 <= y <= frame_y`, frame_y = size frame + padded border.
        let frame_y = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        Some(PhysRect { x: 0, y: 0, width: client.right.max(0) as u32, height: (frame_y + 1).max(0) as u32 })
    }
}

/// The parts of the host at `content` (client pixels) that must not take the mouse, in host pixels:
/// each interactive hole and the resize band, clipped to the host.
pub(crate) fn host_cutouts(content: PhysRect, band: Option<PhysRect>, holes: &[PhysRect]) -> Vec<PhysRect> {
    band.iter()
        .chain(holes)
        .filter_map(|r| intersect(*r, content))
        .map(|r| PhysRect { x: r.x - content.x, y: r.y - content.y, ..r })
        .collect()
}

fn intersect(a: PhysRect, b: PhysRect) -> Option<PhysRect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);
    (right > left && bottom > top).then(|| PhysRect { x: left, y: top, width: (right - left) as u32, height: (bottom - top) as u32 })
}

#[cfg(test)]
mod tests {
    use super::{PhysRect, host_cutouts};

    fn rect(x: i32, y: i32, width: u32, height: u32) -> PhysRect {
        PhysRect { x, y, width, height }
    }

    #[test]
    fn a_host_below_the_band_keeps_all_of_itself() {
        let content = rect(0, 80, 800, 500);
        assert!(host_cutouts(content, Some(rect(0, 0, 800, 9)), &[]).is_empty());
    }

    #[test]
    fn a_host_that_reaches_the_top_loses_the_band() {
        let content = rect(0, 0, 800, 500);
        assert_eq!(host_cutouts(content, Some(rect(0, 0, 800, 9)), &[]), vec![rect(0, 0, 800, 9)]);
    }

    #[test]
    fn holes_are_host_relative_and_clipped_to_the_host() {
        let content = rect(10, 100, 400, 300);
        let holes = [rect(300, 50, 200, 100), rect(20, 120, 10, 10), rect(500, 500, 5, 5)];
        assert_eq!(host_cutouts(content, None, &holes), vec![rect(290, 0, 110, 50), rect(10, 20, 10, 10)]);
    }
}
