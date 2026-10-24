//! Owned by W1.2.
//! A screenshot of a window's client area, page included: `PrintWindow(PW_RENDERFULLCONTENT)` returns
//! GPUI's DirectComposition content and the CEF children (about 20 ms, spike-win §11).
//! Windows.Graphics.Capture is no alternative: it refuses windows off the input desktop.

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, ClientToScreen, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, HDC, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};
use windows_sys::core::BOOL;

use super::{PlatformError, error, last_error};

const PW_RENDERFULLCONTENT: u32 = 2;

// windows-sys has PrintWindow under Win32_Storage_Xps, a feature this crate does not enable.
#[link(name = "user32")]
unsafe extern "system" {
    fn PrintWindow(window: HWND, dc: HDC, flags: u32) -> BOOL;
}

/// The client area of `window`, in physical pixels.
pub(crate) fn client(window: HWND) -> Result<image::RgbaImage, PlatformError> {
    unsafe {
        let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut client = frame;
        let mut origin = POINT { x: 0, y: 0 };
        if GetWindowRect(window, &mut frame) == 0 || GetClientRect(window, &mut client) == 0 {
            return Err(last_error("GetWindowRect"));
        }
        ClientToScreen(window, &mut origin);
        let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
        let (cw, ch) = (client.right, client.bottom);
        if width <= 0 || height <= 0 || cw <= 0 || ch <= 0 {
            return Err(error("the window has no area to capture"));
        }
        let (ox, oy) = (origin.x - frame.left, origin.y - frame.top);
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return Err(last_error("CreateCompatibleDC"));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            },
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() {
            DeleteDC(dc);
            return Err(last_error("CreateDIBSection"));
        }
        let old = SelectObject(dc, bitmap);
        let printed = PrintWindow(window, dc, PW_RENDERFULLCONTENT) != 0;
        GdiFlush();
        let result = if printed {
            let src = std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize);
            Ok(crop_bgra(src, width as u32, ox, oy, cw as u32, ch as u32))
        } else {
            Err(last_error("PrintWindow"))
        };
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        result
    }
}

/// The `w`×`h` rectangle at (`x`, `y`) of a top-down BGRA image `stride` pixels wide, as opaque RGBA.
fn crop_bgra(src: &[u8], stride: u32, x: i32, y: i32, w: u32, h: u32) -> image::RgbaImage {
    let rows = src.len() as u32 / 4 / stride.max(1);
    image::RgbaImage::from_fn(w, h, |cx, cy| {
        let (sx, sy) = (x + cx as i32, y + cy as i32);
        if sx < 0 || sy < 0 || sx as u32 >= stride || sy as u32 >= rows {
            return image::Rgba([0, 0, 0, 255]);
        }
        let i = ((sy as u32 * stride + sx as u32) * 4) as usize;
        image::Rgba([src[i + 2], src[i + 1], src[i], 255])
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn crops_and_swaps_to_rgba() {
        // 2x2 BGRA: blue, green / red, white.
        let src = [255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0, 255, 255, 255, 0];
        let img = super::crop_bgra(&src, 2, 1, 0, 1, 2);
        assert_eq!(img.dimensions(), (1, 2));
        assert_eq!(img.get_pixel(0, 0).0, [0, 255, 0, 255]);
        assert_eq!(img.get_pixel(0, 1).0, [255, 255, 255, 255]);
    }
}
