//! Owned by W1.3.
//! `DisplayHandler` (UI thread): address, title, fullscreen, status text, load progress and an
//! extension popup's preferred size become `TabEvent`s. Favicons go to `favicon.rs` (W3.2) and media
//! capture to `permission.rs` (W3.1).

use std::sync::Arc;

use cef::*;
use vsesvit_engine::{TabEvent, TabId, Url, Zoom};

use crate::client::HandlerCtx;
use crate::registry::{self, BrowserId};
use crate::strings;

/// Sends an event about the tab a browser is; other browsers' events are dropped.
pub fn to_tab(browser: BrowserId, event: TabEvent) {
    let found = registry::with(|r| Some((r.tab_of(browser)?, r.sink.clone()))).flatten();
    if let Some((tab, sink)) = found {
        sink.tab(tab, event);
    }
}

/// `Engine::set_visible`: `was_hidden` only; the shell maps and unmaps the window.
pub fn set_visible(tab: TabId, visible: bool) {
    registry::on_tab(tab, |page| page.was_hidden(!visible));
}

/// `Engine::notify_moved`.
pub fn notify_moved(tab: TabId) {
    registry::on_tab(tab, |page| page.notify_moved());
}

/// `Engine::notify_screen_info_changed`.
pub fn notify_screen_info_changed(tab: TabId) {
    registry::on_tab(tab, |page| page.notify_screen_info_changed());
}

/// Chromium's logarithmic zoom level for a factor (factor = 1.2^level).
pub fn zoom_level_of(factor: f64) -> f64 {
    if factor > 0.0 { factor.ln() / 1.2f64.ln() } else { 0.0 }
}

/// The factor of Chromium's zoom level.
pub fn zoom_factor_of(level: f64) -> f64 {
    1.2f64.powf(level)
}

/// `Engine::set_zoom`.
pub fn set_zoom(tab: TabId, zoom: Zoom) {
    registry::on_tab(tab, |page| page.set_zoom_level(zoom_level_of(zoom.0)));
}

/// `Engine::zoom`.
pub fn zoom(tab: TabId) -> Zoom {
    Zoom(registry::on_tab(tab, |page| zoom_factor_of(page.zoom_level())).unwrap_or(1.0))
}

/// `Engine::set_audio_muted`.
pub fn set_audio_muted(tab: TabId, muted: bool) {
    registry::on_tab(tab, |page| page.set_audio_muted(muted));
}

/// `Engine::exit_fullscreen`.
pub fn exit_fullscreen(tab: TabId) {
    registry::on_tab(tab, |page| page.exit_fullscreen());
}

/// `Engine::print`: Chrome's print preview in the page's own window.
pub fn print(tab: TabId) {
    registry::on_tab(tab, |page| page.print());
}

/// The main frame's address changed.
pub fn address(browser: BrowserId, url: &str) {
    match Url::parse(url) {
        Ok(url) => to_tab(browser, TabEvent::AddressChanged(url)),
        Err(e) => log::warn!("vsesvit-cef: browser {browser} reported an address that is no URL ({e})"),
    }
}

wrap_display_handler! {
    pub struct Display {
        ctx: Arc<HandlerCtx>,
    }

    impl DisplayHandler {
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let (Some(browser), Some(frame)) = (browser, frame) else { return };
            if frame.is_main() != 0 {
                registry::seen(browser.identifier(), "on_address_change");
                address(browser.identifier(), &strings::text(url));
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::TitleChanged(strings::text(title)));
            }
        }

        fn on_favicon_urlchange(&self, browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            if let Some(browser) = browser {
                super::favicon::on_favicon_urlchange(browser, strings::take_list(icon_urls));
            }
        }

        fn on_fullscreen_mode_change(&self, browser: Option<&mut Browser>, fullscreen: i32) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::FullscreenRequested(fullscreen != 0));
            }
        }

        fn on_status_message(&self, browser: Option<&mut Browser>, value: Option<&CefString>) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::StatusText(strings::non_empty(value)));
            }
        }

        fn on_loading_progress_change(&self, browser: Option<&mut Browser>, progress: f64) {
            if let Some(browser) = browser {
                to_tab(browser.identifier(), TabEvent::LoadProgress(progress.clamp(0.0, 1.0) as f32));
            }
        }

        fn on_auto_resize(&self, browser: Option<&mut Browser>, new_size: Option<&Size>) -> i32 {
            let (Some(browser), Some(size)) = (browser, new_size) else { return 0 };
            let width = size.width.max(0) as u32;
            let height = size.height.max(0) as u32;
            to_tab(browser.identifier(), TabEvent::PreferredSize { width, height });
            1
        }

        fn on_media_access_change(&self, browser: Option<&mut Browser>, has_video_access: i32, has_audio_access: i32) {
            if let Some(browser) = browser {
                super::permission::on_media_access_change(browser.identifier(), has_video_access != 0, has_audio_access != 0);
            }
        }
    }
}

impl Display {
    /// The handler for a client.
    pub fn handler(ctx: &Arc<HandlerCtx>) -> DisplayHandler {
        Display::new(ctx.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_levels_are_logarithmic() {
        assert_eq!(zoom_level_of(1.0), 0.0);
        assert!((zoom_level_of(1.2) - 1.0).abs() < 1e-9);
        assert!((zoom_level_of(1.44) - 2.0).abs() < 1e-9);
        assert!((zoom_factor_of(-1.0) - 1.0 / 1.2).abs() < 1e-9);
        for factor in [0.25, 0.5, 0.9, 1.1, 1.75, 3.0, 5.0] {
            assert!((zoom_factor_of(zoom_level_of(factor)) - factor).abs() < 1e-9, "{factor}");
        }
        assert_eq!(zoom_level_of(0.0), 0.0, "no zoom of zero");
    }
}
