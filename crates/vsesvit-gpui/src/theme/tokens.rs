//! Owned by W0.4.
//! Layout metrics in logical pixels (UA section 3.3). The values follow the GTK shell's CSS and
//! libadwaita, so both shells measure the same. Durations live in `motion.rs`.

use gpui::{Edges, Pixels, Size, px, size};

/// Padding as CSS writes it: `vertical horizontal`.
const fn pad(vertical: f32, horizontal: f32) -> Edges<Pixels> {
    Edges { top: px(vertical), right: px(horizontal), bottom: px(vertical), left: px(horizontal) }
}

/// Padding as CSS writes it: `top right bottom left`.
const fn pad4(top: f32, right: f32, bottom: f32, left: f32) -> Edges<Pixels> {
    Edges { top: px(top), right: px(right), bottom: px(bottom), left: px(left) }
}

// Text sizes (UA section 3.1). `TEXT` is also the theme's base size.
pub const TEXT: Pixels = px(14.);
pub const TEXT_CAPTION: Pixels = px(12.);
/// Semibold.
pub const TEXT_HEADING: Pixels = px(15.);
/// The profile picker's title.
pub const TEXT_TITLE: Pixels = px(26.);
/// `0.85em` of [`TEXT`]: zoom chip, group chips.
pub const TEXT_SMALL: Pixels = px(14. * 0.85);

pub const WINDOW_DEFAULT: Size<Pixels> = size(px(1280.), px(820.));

// The toolbar row, which is also the title bar.
pub const TOOLBAR_H: Pixels = px(46.);
pub const TOOLBAR_BUTTON: Pixels = px(34.);
pub const TOOLBAR_ICON: Pixels = px(16.);
pub const TOOLBAR_SPACING: Pixels = px(6.);
pub const TOOLBAR_EDGE: Pixels = px(6.);

pub const OMNIBOX_H: Pixels = px(34.);
pub const OMNIBOX_RADIUS: Pixels = px(8.);
/// A compact omnibox is at most this wide and stays within [`OMNIBOX_CENTRE_SLACK`] of the
/// window's centre.
pub const OMNIBOX_COMPACT_MAX_W: Pixels = px(720.);
pub const OMNIBOX_CENTRE_SLACK: Pixels = px(40.);

/// The load progress line along the omnibox's bottom edge; it fades from transparent to the
/// foreground colour.
pub const PROGRESS_H: Pixels = px(2.);
pub const PROGRESS_INSET_X: Pixels = px(10.);
pub const PROGRESS_BOTTOM: Pixels = px(1.);
pub const PROGRESS_RADIUS: Pixels = px(1.);

pub const ZOOM_CHIP_MIN_H: Pixels = px(20.);
pub const ZOOM_CHIP_PADDING: Edges<Pixels> = pad(0., 6.);
pub const ZOOM_CHIP_RADIUS: Pixels = px(10.);

/// The omnibox's in-use button and security icon, each square.
pub const OMNIBOX_ICON_BUTTON: Pixels = px(24.);
pub const ICON_GAP: Pixels = px(2.);

pub const BANNER_H: Pixels = px(40.);

pub const TAB_STRIP_H: Pixels = px(38.);
pub const TAB_H: Pixels = px(34.);
pub const TAB_MIN_W: Pixels = px(64.);
pub const TAB_MAX_W: Pixels = px(220.);
/// A pinned tab shows its icon only.
pub const TAB_PINNED_W: Pixels = px(46.);
pub const TAB_CLOSE: Pixels = px(22.);
pub const TAB_GAP: Pixels = px(4.);

pub const GROUP_CHIP_PADDING: Edges<Pixels> = pad(1., 8.);
pub const GROUP_CHIP_RADIUS: Pixels = px(9.);
/// An untitled group's chip is a dot.
pub const GROUP_DOT_UNTITLED: Pixels = px(12.);
/// The group dot on a strip tab, in the indicator slot.
pub const GROUP_DOT_TAB: Pixels = px(8.);

pub const BOOKMARKS_BAR_MIN_H: Pixels = px(28.);
pub const BOOKMARKS_BAR_PADDING: Edges<Pixels> = pad(2., 6.);
pub const BOOKMARK_BUTTON_MIN_H: Pixels = px(24.);
pub const BOOKMARK_BUTTON_PADDING: Edges<Pixels> = pad(2., 6.);
pub const BOOKMARK_SPACING: Pixels = px(1.);
pub const BOOKMARK_LABEL_MAX_CHARS: usize = 18;
/// Bold.
pub const BOOKMARK_CHEVRON_PADDING: Edges<Pixels> = pad(2., 8.);
/// Drop before or after: an accent line this thick. Drop into: accent at
/// [`BOOKMARK_DROP_INTO_ALPHA`] with [`BOOKMARK_DROP_INTO_RADIUS`].
pub const BOOKMARK_DROP_LINE: Pixels = px(2.);
pub const BOOKMARK_DROP_INTO_ALPHA: f32 = 0.25;
pub const BOOKMARK_DROP_INTO_RADIUS: Pixels = px(6.);

pub const FIND_BAR_H: Pixels = px(42.);
/// About 28 characters.
pub const FIND_ENTRY_W: Pixels = px(230.);

pub const SIDEBAR_MIN_W: Pixels = px(180.);
pub const SIDEBAR_MAX_W: Pixels = px(300.);
/// Below this window width the sidebar hides.
pub const SIDEBAR_HIDE_BELOW: Pixels = px(720.);

/// The sidebar's width in a window `window_width` wide: 20 % of it, clamped to
/// [`SIDEBAR_MIN_W`]..[`SIDEBAR_MAX_W`]. `None` when the window is too narrow for one.
pub fn sidebar_width(window_width: Pixels) -> Option<Pixels> {
    (window_width >= SIDEBAR_HIDE_BELOW).then(|| (window_width * 0.2).clamp(SIDEBAR_MIN_W, SIDEBAR_MAX_W))
}

/// The sidebar's header and footer buttons are full width.
pub const SIDEBAR_BUTTON_MARGIN: Pixels = px(6.);
pub const SIDEBAR_BUTTON_H: Pixels = px(34.);

pub const TAB_ROW_MIN_H: Pixels = px(30.);
pub const TAB_ROW_PADDING: Edges<Pixels> = pad4(4., 6., 4., 10.);
pub const TAB_ROW_SPACING: Pixels = px(8.);
pub const FAVICON: Pixels = px(16.);
/// The close button's opacity at rest; 1 on hover.
pub const TAB_ROW_CLOSE_OPACITY: f32 = 0.6;
/// Dim.
pub const TAB_ROW_PIN: Pixels = px(16.);

pub const GROUP_HEADER_MARGIN: Edges<Pixels> = pad4(4., 6., 0., 6.);
pub const GROUP_HEADER_PADDING: Edges<Pixels> = pad4(2., 6., 2., 10.);
pub const GROUP_HEADER_MIN_H: Pixels = px(24.);
/// The colour bar inset on a grouped row.
pub const GROUP_BAR_W: Pixels = px(3.);

pub const SHARING_BAR_H: Pixels = px(40.);

/// Suggestions are as wide as the omnibox.
pub const SUGGESTIONS_MAX_H: Pixels = px(440.);
pub const SUGGESTION_MARGIN: Edges<Pixels> = pad(6., 8.);
pub const SUGGESTION_SPACING: Pixels = px(12.);

pub const TAB_SEARCH_W: Pixels = px(360.);
pub const TAB_SEARCH_LIST_MAX_H: Pixels = px(440.);
pub const GROUP_EDITOR_W: Pixels = px(300.);
pub const SITE_INFO_W: Pixels = px(380.);
pub const SITE_INFO_MAX_H: Pixels = px(680.);
pub const PERMISSION_PROMPT_W: Pixels = px(320.);
pub const STAR_BUBBLE_W: Pixels = px(340.);
pub const DOWNLOAD_WARNING_W: Pixels = px(300.);

pub const MENU_MIN_W: Pixels = px(200.);
pub const MENU_ROW_MIN_H: Pixels = px(28.);
pub const BOOKMARK_MENU_ROW_MIN_H: Pixels = px(24.);
pub const BOOKMARK_MENU_ROW_PADDING: Edges<Pixels> = pad(4., 8.);
pub const BOOKMARK_MENU_LABEL_MAX_CHARS: usize = 40;
/// Of the monitor's height.
pub const BOOKMARK_MENU_MAX_H_FRACTION: f32 = 0.75;

pub const EXTENSION_POPUP_MIN: Size<Pixels> = size(px(200.), px(80.));
pub const EXTENSION_POPUP_MAX: Size<Pixels> = size(px(800.), px(600.));
pub const EXTENSION_POPUP_INITIAL: Size<Pixels> = size(px(320.), px(200.));

/// `0.65em` bold, in the accent colours.
pub const EXTENSION_BADGE_TEXT: Pixels = px(14. * 0.65);
pub const EXTENSION_BADGE_PADDING: Edges<Pixels> = pad(0., 3.);
pub const EXTENSION_BADGE_MIN: Size<Pixels> = size(px(10.), px(12.));
pub const EXTENSION_BADGE_RADIUS: Pixels = px(6.);

/// Toolbar button and menus.
pub const AVATAR: Pixels = px(22.);
/// The picker and other large places.
pub const AVATAR_LARGE: Pixels = px(72.);

pub const TOAST_MAX_W: Pixels = px(450.);
pub const TOAST_BOTTOM: Pixels = px(24.);
pub const TOAST_PADDING: Edges<Pixels> = pad(8., 12.);
/// A pill.
pub const TOAST_RADIUS: Pixels = px(18.);

/// Two lines, in the OSD colours.
pub const FULLSCREEN_NOTICE_PADDING: Edges<Pixels> = pad(12., 20.);
pub const FULLSCREEN_NOTICE_RADIUS: Pixels = px(12.);
pub const FULLSCREEN_NOTICE_TOP: Pixels = px(24.);

/// `0.9em`; the URL is middle-ellipsized to [`LINK_PREVIEW_MAX_CHARS`].
pub const LINK_PREVIEW_TEXT: Pixels = px(14. * 0.9);
pub const LINK_PREVIEW_PADDING: Edges<Pixels> = pad(3., 8.);
/// Top-right corner only.
pub const LINK_PREVIEW_RADIUS: Pixels = px(6.);
pub const LINK_PREVIEW_MAX_CHARS: usize = 90;

pub const SETTINGS_WINDOW: Size<Pixels> = size(px(1040.), px(760.));
pub const EXTENSIONS_WINDOW: Size<Pixels> = size(px(720.), px(760.));
/// Bookmarks and History.
pub const LIBRARY_WINDOW: Size<Pixels> = size(px(640.), px(680.));
pub const DOWNLOADS_WINDOW: Size<Pixels> = size(px(560.), px(560.));
pub const WELCOME_WINDOW: Size<Pixels> = size(px(560.), px(640.));
pub const PICKER_WINDOW: Size<Pixels> = size(px(760.), px(520.));
pub const MANAGE_PROFILES_WINDOW: Size<Pixels> = size(px(520.), px(480.));

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    #[test]
    fn the_sidebar_is_a_fifth_of_the_window_within_its_bounds() {
        assert_eq!(sidebar_width(px(719.)), None);
        assert_eq!(sidebar_width(px(720.)), Some(SIDEBAR_MIN_W));
        assert_eq!(sidebar_width(px(1200.)), Some(px(240.)));
        assert_eq!(sidebar_width(px(4000.)), Some(SIDEBAR_MAX_W));
    }
}
