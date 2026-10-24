//! Owned by W0.4.
//! The colours that are not theme keys (UA section 3.1): the private window's top bars, tab group
//! colours and profile colours. Group and profile colours come from core, so every shell paints the
//! same ones.

use gpui::{Hsla, Rgba};
use vsesvit_core::profiles::ProfileColor;
use vsesvit_core::tab_groups::GroupColor;

/// The top bars of a private window: toolbar, banner, tab strip, bookmarks bar, find bar. The page
/// and the sidebar keep the theme.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ChromePalette {
    pub background: Hsla,
    pub foreground: Hsla,
    /// The bars of a window that is not active.
    pub inactive_background: Hsla,
    pub border: Hsla,
}

/// The GTK shell's private header bar, in light and dark mode alike.
pub fn private_chrome() -> ChromePalette {
    ChromePalette {
        background: hex(0x2e2e32ff),
        foreground: hex(0xffffffff),
        inactive_background: hex(0x222226ff),
        border: hex(0xffffff17),
    }
}

/// A tab group's colour: Chrome's shade of it for the mode.
pub fn group(color: GroupColor, dark: bool) -> Hsla {
    rgb(color.rgb(dark))
}

/// The text on a group chip, which is filled with [`group`].
pub fn group_chip_text(dark: bool) -> Hsla {
    if dark { hex(0x202124ff) } else { hex(0xffffffff) }
}

/// A profile's colour. White text reads on every one.
pub fn profile(color: ProfileColor) -> Hsla {
    rgb(color.rgb())
}

fn rgb([r, g, b]: [u8; 3]) -> Hsla {
    hex(u32::from_be_bytes([r, g, b, 0xff]))
}

/// `0xRRGGBBAA`.
pub(crate) fn hex(rgba: u32) -> Hsla {
    gpui::rgba(rgba).into()
}

/// `color` as `0xRRGGBBAA`, each channel rounded.
pub fn to_hex(color: Hsla) -> u32 {
    let Rgba { r, g, b, a } = color.to_rgb();
    let byte = |c: f32| (c.clamp(0., 1.) * 255.).round() as u32;
    byte(r) << 24 | byte(g) << 16 | byte(b) << 8 | byte(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_colours_are_cores_table_in_both_modes() {
        let light: Vec<u32> = GroupColor::ALL.iter().map(|&c| to_hex(group(c, false))).collect();
        let dark: Vec<u32> = GroupColor::ALL.iter().map(|&c| to_hex(group(c, true))).collect();
        assert_eq!(
            light,
            [0x5f6368ff, 0x1a73e8ff, 0xd93025ff, 0xf9ab00ff, 0x1e8e3eff, 0xd01884ff, 0x9334e6ff, 0x007b83ff, 0xfa903eff]
        );
        assert_eq!(
            dark,
            [0xbdc1c6ff, 0x8ab4f8ff, 0xf28b82ff, 0xfdd663ff, 0x81c995ff, 0xff8bcbff, 0xc58af9ff, 0x78d9ecff, 0xfcad70ff]
        );
        assert_eq!(to_hex(group_chip_text(false)), 0xffffffff);
        assert_eq!(to_hex(group_chip_text(true)), 0x202124ff);
    }

    #[test]
    fn profile_colours_are_cores_table() {
        let all: Vec<u32> = ProfileColor::ALL.iter().map(|&c| to_hex(profile(c))).collect();
        assert_eq!(
            all,
            [0x3584e4ff, 0x2190a4ff, 0x3a944aff, 0xc88800ff, 0xed5b00ff, 0xe62d42ff, 0xd56199ff, 0x9141acff, 0x6f8396ff]
        );
    }

    #[test]
    fn the_private_bars_are_the_gtk_header_bar() {
        let p = private_chrome();
        assert_eq!(
            [p.background, p.foreground, p.inactive_background, p.border].map(to_hex),
            [0x2e2e32ff, 0xffffffff, 0x222226ff, 0xffffff17]
        );
    }
}
