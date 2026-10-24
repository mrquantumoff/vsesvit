//! Owned by W3.2.
//! `on_favicon_urlchange` → `download_image` → `get_as_png`; `display.rs` delegates here. The body
//! below is W1.3's dispatch stub (design section 8.1): favicons are not fetched yet.

/// The page's favicon URLs changed.
pub fn on_favicon_urlchange(_browser: &mut cef::Browser, _urls: Vec<String>) {}
