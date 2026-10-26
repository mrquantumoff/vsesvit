//! Owned by W1.7.
//! The window's frame: what a browser window asks GPUI for. The window system draws the title bar and
//! the caption buttons (server-side decorations) on both OSes; a frame of our own, with the toolbar as
//! the title bar (UA section 1.5), comes later.
//!
//! On Windows the window's background is transparent and nothing paints the page's rectangle, because
//! GPUI's DirectComposition target sits above the page's window (SW section 5, design section 1.4).

use gpui::{App, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, point, px, size};

use crate::state::WindowGeometry;
use crate::theme::tokens;

/// Every profile's windows carry one app id, the desktop file's name.
pub const APP_ID: &str = "dev.mrquantumoff.vsesvit";

/// What GPUI is asked for a browser window: the saved frame, else the default size centred; a
/// transparent background when `transparent` (`platform::Surface::TRANSPARENT_CONTENT`).
pub fn options(geometry: WindowGeometry, transparent: bool, cx: &App) -> WindowOptions {
    let bounds = match geometry.bounds {
        Some((x, y, width, height)) => {
            Bounds { origin: point(px(x as f32), px(y as f32)), size: size(px(width as f32), px(height as f32)) }
        }
        None => Bounds::centered(None, tokens::WINDOW_DEFAULT, cx),
    };
    WindowOptions {
        window_bounds: Some(if geometry.maximized { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) }),
        titlebar: Some(TitlebarOptions { title: Some("Vsesvit".into()), ..Default::default() }),
        window_background: background(transparent),
        app_id: Some(APP_ID.to_owned()),
        window_min_size: Some(size(px(360.), px(240.))),
        window_decorations: Some(WindowDecorations::Server),
        // Pages in an inactive window keep their frame rate: Chromium paints its own windows anyway.
        inactive_frame_interval: None,
        ..Default::default()
    }
}

pub fn background(transparent: bool) -> WindowBackgroundAppearance {
    if transparent { WindowBackgroundAppearance::Transparent } else { WindowBackgroundAppearance::Opaque }
}
