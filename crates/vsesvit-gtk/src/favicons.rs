//! Favicons of bookmarked pages: kept in the profile as PNG when a tab shows one (core
//! decides whether the page or its site is bookmarked), shown by the bookmarks bar and the
//! Bookmarks dialog in place of the generic page icon.

use gtk::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::{Profile, Url};

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
    match profile.favicons().record(&url, &icon.save_to_png_bytes()) {
        Ok(changed) => changed,
        Err(e) => {
            log::warn!("favicon of {url}: {e}");
            false
        }
    }
}

/// An image of the stored icon, or of the generic `fallback` icon.
pub(crate) fn image(icon: Option<&gdk::Texture>, fallback: &str) -> gtk::Image {
    match icon {
        Some(texture) => gtk::Image::from_paintable(Some(texture)),
        None => gtk::Image::from_icon_name(fallback),
    }
}
