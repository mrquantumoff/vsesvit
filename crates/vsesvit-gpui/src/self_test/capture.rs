//! Owned by W1.8.
//! The self-test's window captures (design section 7.2): the platform surface's own capture, page
//! included. It runs between GPUI updates, never inside one: on Windows `PrintWindow` sends the
//! window a paint message, and GPUI's window procedure then updates the app, which an update already
//! holds ("RefCell already borrowed"). No surface borrow is kept past the call, so the window's own
//! layout and its close are never refused one.

use std::path::Path;

use gpui::{AsyncApp, Entity};
use image::RgbaImage;

use crate::window::BrowserWindow;

/// The whole of `window`'s client area, page included.
pub fn window(window: &Entity<BrowserWindow>, cx: &mut AsyncApp) -> Result<RgbaImage, String> {
    let surface = cx.update(|cx| window.read(cx).surface()).ok_or("the window has no surface")?;
    let shot = surface.try_borrow().map_err(|_| "the window's surface is in use".to_owned())?.capture().map_err(|e| e.to_string());
    drop(surface);
    shot
}

/// Writes `image` as a PNG.
pub fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
    image.save(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// How many distinct colours `image` holds, up to `cap`: one means nothing rendered.
pub fn colours(image: &RgbaImage, cap: usize) -> usize {
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels() {
        seen.insert(pixel.0);
        if seen.len() >= cap {
            break;
        }
    }
    seen.len()
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    #[test]
    fn a_flat_image_has_one_colour() {
        let mut image = RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255]));
        assert_eq!(super::colours(&image, 8), 1);
        image.put_pixel(3, 3, Rgba([9, 9, 9, 255]));
        assert_eq!(super::colours(&image, 8), 2);
        assert_eq!(super::colours(&image, 1), 1, "counting stops at the cap");
    }
}
