//! Favicons of bookmarked pages: kept in the profile as PNG when a tab shows one (core
//! decides whether the page or its site is bookmarked), shown by the bookmarks bar and the
//! Bookmarks window in place of the generic page icon.

use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use gtk::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::{Profile, Url};

/// Icons are kept at most this many pixels on a side, the size the background fetch keeps.
const SIZE: i32 = 32;

/// The stored icon of `url`'s page, or of its site.
pub(crate) fn stored(profile: &mut Profile, url: &Url) -> Option<gdk::Texture> {
    match profile.favicons().get(url) {
        Ok(png) => gdk::Texture::from_bytes(&glib::Bytes::from_owned(png?)).ok(),
        Err(e) => {
            log::warn!("favicon of {url}: {e}");
            None
        }
    }
}

/// Keeps `icon` as the icon of the page at `uri`. Returns whether the stored icon changed.
pub(crate) fn record(profile: &mut Profile, uri: &str, icon: &gdk::Texture) -> bool {
    let Ok(url) = Url::parse(uri) else { return false };
    // Most pages are not bookmarked: they cost no scale or encode.
    if !profile.favicons().wanted(&url) {
        return false;
    }
    match profile.favicons().record(&url, &fitted(icon).save_to_png_bytes()) {
        Ok(changed) => changed,
        Err(e) => {
            log::warn!("favicon of {url}: {e}");
            false
        }
    }
}

/// `icon` scaled down to fit [`SIZE`], so that a page's huge icon costs only a small encode.
fn fitted(icon: &gdk::Texture) -> gdk::Texture {
    let (width, height) = (icon.width(), icon.height());
    if width <= SIZE && height <= SIZE {
        return icon.clone();
    }
    let mut downloader = gdk::TextureDownloader::new(icon);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (pixels, stride) = downloader.download_bytes();
    let full = Pixbuf::from_bytes(&pixels, Colorspace::Rgb, true, 8, width, height, stride as i32);
    let scale = f64::from(SIZE) / f64::from(width.max(height));
    let fit = |side: i32| ((f64::from(side) * scale).round() as i32).max(1);
    let Some(small) = full.scale_simple(fit(width), fit(height), InterpType::Bilinear) else {
        return icon.clone();
    };
    let stride = small.rowstride() as usize;
    gdk::MemoryTexture::new(small.width(), small.height(), gdk::MemoryFormat::R8g8b8a8, &small.read_pixel_bytes(), stride).upcast()
}

/// An image of the stored icon, or of the generic `fallback` icon.
pub(crate) fn image(icon: Option<&gdk::Texture>, fallback: &str) -> gtk::Image {
    let image = gtk::Image::new();
    show(&image, icon, fallback);
    image
}

/// Makes `image` show the stored icon, or the generic `fallback` icon.
pub(crate) fn show(image: &gtk::Image, icon: Option<&gdk::Texture>, fallback: &str) {
    match icon {
        Some(texture) => image.set_paintable(Some(texture)),
        None => image.set_icon_name(Some(fallback)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::scratch_dir;
    use vsesvit_core::OpenOptions;
    use vsesvit_core::bookmarks::{BookmarkId, InsertAt};

    /// A `side`-pixel square of noise, which compresses badly.
    fn noise(side: i32) -> gdk::Texture {
        let mut seed = 1u32;
        let pixels: Vec<u8> = (0..side * side * 4)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as u8
            })
            .collect();
        let stride = side as usize * 4;
        gdk::MemoryTexture::new(side, side, gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_owned(pixels), stride).upcast()
    }

    #[test]
    fn a_large_icon_is_kept_at_the_size_the_bookmarks_show() {
        let mut profile = Profile::open(&scratch_dir("favicon-size"), OpenOptions::default()).unwrap();
        let mut sizes = Vec::new();
        for (side, page) in [(64, "https://a.test/"), (512, "https://b.test/")] {
            let url = Url::parse(page).unwrap();
            profile.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Page", &url).unwrap();
            record(&mut profile, page, &noise(side));
            sizes.push(stored(&mut profile, &url).map(|icon| (icon.width(), icon.height())));
        }
        assert_eq!(sizes, [Some((32, 32)), Some((32, 32))]);
    }
}
