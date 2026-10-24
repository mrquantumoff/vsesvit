//! Owned by W0.4.
//! The shell's look (UA section 3): gpui-component's `Theme` with the two palettes of
//! `assets/themes/`, which follow libadwaita as the GTK shell does, chosen by the user's theme pref
//! or by the desktop. Everything paints from `cx.theme()`; the few colours that are not theme keys
//! are in `colors`, sizes in `tokens`, icons in `icons`.

pub mod avatar;
pub mod colors;
pub mod icons;
pub mod motion;
pub mod tokens;

use std::rc::Rc;

use gpui::{App, Global, Window, WindowAppearance};
use gpui_component::{Theme, ThemeConfig, ThemeMode, ThemeSet};
use vsesvit_core::prefs;

pub use colors::{ChromePalette, private_chrome};

const LIGHT: &str = include_str!("../../assets/themes/vsesvit-light.json");
const DARK: &str = include_str!("../../assets/themes/vsesvit-dark.json");

/// The theme pref [`apply`] was last given.
struct Chosen(prefs::Theme);

impl Global for Chosen {}

/// Initializes gpui-component (the shell calls this instead of `gpui_component::init`), installs
/// the two palettes and follows the desktop until [`apply`] is told otherwise.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    let theme = Theme::global_mut(cx);
    theme.light_theme = palette(LIGHT);
    theme.dark_theme = palette(DARK);
    apply(prefs::Theme::System, cx);
}

/// Applies the user's theme pref. `System` takes the desktop's appearance now, and again whenever
/// a window that [`follow_desktop`] watches sees it change.
pub fn apply(pref: prefs::Theme, cx: &mut App) {
    cx.set_global(Chosen(pref));
    Theme::change(mode(pref, cx.window_appearance()), None, cx);
}

/// Follows `window`'s appearance (the portal's `color-scheme` on Linux, the apps mode on Windows)
/// while the pref is `System`. Call once for each new window.
pub fn follow_desktop(window: &Window) {
    window
        .observe_window_appearance(|window, cx| {
            let wanted = mode(cx.global::<Chosen>().0, window.appearance());
            if Theme::global(cx).mode != wanted {
                Theme::change(wanted, None, cx);
            }
        })
        .detach();
}

/// Whether the active palette is the dark one.
pub fn is_dark(cx: &App) -> bool {
    Theme::global(cx).is_dark()
}

fn mode(pref: prefs::Theme, desktop: WindowAppearance) -> ThemeMode {
    match pref {
        prefs::Theme::System => desktop.into(),
        prefs::Theme::Light => ThemeMode::Light,
        prefs::Theme::Dark => ThemeMode::Dark,
    }
}

/// The one theme in a bundled theme file.
fn palette(json: &str) -> Rc<ThemeConfig> {
    let set: ThemeSet = serde_json::from_str(json).expect("a bundled theme file parses (tested)");
    Rc::new(set.themes.into_iter().next().expect("a bundled theme file holds a theme (tested)"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use gpui::{Hsla, TestAppContext};
    use gpui_component::ThemeColor;
    use serde_json::Value;

    use super::colors::to_hex;
    use super::*;

    /// The keys a theme file sets.
    fn keys(json: &str) -> BTreeSet<String> {
        let file: Value = serde_json::from_str(json).unwrap();
        file["themes"][0]["colors"].as_object().unwrap().keys().cloned().collect()
    }

    #[test]
    fn every_key_is_one_gpui_component_reads_and_both_files_set_the_same() {
        for json in [LIGHT, DARK] {
            let read = serde_json::to_value(&palette(json).colors).unwrap();
            let read: BTreeSet<String> =
                read.as_object().unwrap().iter().filter(|(_, v)| !v.is_null()).map(|(k, _)| k.clone()).collect();
            assert_eq!(keys(json), read, "a key gpui-component does not know is ignored silently");
            for (key, value) in serde_json::from_str::<Value>(json).unwrap()["themes"][0]["colors"].as_object().unwrap() {
                let value = value.as_str().unwrap();
                assert!(value.len() == 7 || value.len() == 9, "{key}: {value} is not #rrggbb[aa]");
                gpui_component::try_parse_color(value).unwrap_or_else(|e| panic!("{key}: {e}"));
            }
        }
        assert_eq!(keys(LIGHT), keys(DARK));
        assert_eq!(palette(LIGHT).mode, ThemeMode::Light);
        assert_eq!(palette(DARK).mode, ThemeMode::Dark);
    }

    #[gpui::test]
    fn the_prefs_choose_the_ua_palettes(cx: &mut TestAppContext) {
        /// A key, where gpui-component puts it, its light value and its dark value.
        type Row = (&'static str, fn(&ThemeColor) -> Hsla, u32, u32);
        // UA section 3.2.
        let table: [Row; 21] = [
            ("background", |t| t.background, 0xfafafbff, 0x222226ff),
            ("foreground", |t| t.foreground, 0x2e3436ff, 0xffffffff),
            ("title_bar.background", |t| t.title_bar, 0xffffffff, 0x2e2e32ff),
            ("title_bar.border", |t| t.title_bar_border, 0x00000017, 0xffffff17),
            ("border", |t| t.border, 0x00000017, 0xffffff17),
            ("window.border", |t| t.window_border, 0x00000017, 0xffffff17),
            ("sidebar.background", |t| t.sidebar, 0xebebedff, 0x2e2e32ff),
            ("sidebar.accent.background", |t| t.sidebar_accent, 0x0000001a, 0xffffff1a),
            ("tab_bar.background", |t| t.tab_bar, 0xebebedff, 0x2e2e32ff),
            ("tab.active.background", |t| t.tab_active, 0xffffffff, 0x3a3a3eff),
            ("popover.background", |t| t.popover, 0xffffffff, 0x36363aff),
            ("primary.background", |t| t.primary, 0x3584e4ff, 0x3584e4ff),
            ("accent.background", |t| t.accent, 0x3584e4ff, 0x3584e4ff),
            ("primary.foreground", |t| t.primary_foreground, 0xffffffff, 0xffffffff),
            ("link", |t| t.link, 0x1c71d8ff, 0x78aeedff),
            ("muted.foreground", |t| t.muted_foreground, 0x2e343699, 0xffffff99),
            ("input.border", |t| t.input, 0x0000001f, 0xffffff1f),
            ("ring", |t| t.ring, 0x3584e480, 0x78aeed80),
            ("danger.background", |t| t.danger, 0xe01b24ff, 0xc01c28ff),
            ("list.hover.background", |t| t.list_hover, 0x0000000f, 0xffffff0f),
            ("overlay", |t| t.overlay, 0x000000b3, 0x000000b3),
        ];
        cx.update(|cx| {
            init(cx);
            for (pref, dark) in [(prefs::Theme::Light, false), (prefs::Theme::Dark, true)] {
                apply(pref, cx);
                assert_eq!(is_dark(cx), dark);
                let theme = Theme::global(cx);
                assert_eq!(theme.font_size, tokens::TEXT);
                for (key, field, light, dark_value) in table {
                    let want = if dark { dark_value } else { light };
                    assert_eq!(to_hex(field(&theme.colors)), want, "{key} in {pref:?}: {:08x}", to_hex(field(&theme.colors)));
                }
                let [success, warning] = if dark { [0x26a269ff, 0xcd9309ff] } else { [0x2ec27eff, 0xe5a50aff] };
                assert_eq!([to_hex(theme.success), to_hex(theme.warning)], [success, warning]);
            }
        });
    }

    #[test]
    fn system_follows_the_desktop_and_a_forced_mode_does_not() {
        use WindowAppearance::*;
        for desktop in [Light, VibrantLight, Dark, VibrantDark] {
            let dark = matches!(desktop, Dark | VibrantDark);
            assert_eq!(mode(prefs::Theme::System, desktop).is_dark(), dark);
            assert_eq!(mode(prefs::Theme::Light, desktop), ThemeMode::Light);
            assert_eq!(mode(prefs::Theme::Dark, desktop), ThemeMode::Dark);
        }
    }
}
