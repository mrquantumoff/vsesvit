//! Owned by W0.4.
//! A profile's avatar, as both other shells draw it: the sync account picture in a circle, else a
//! circle in the profile's colour with its initial in white.

use std::path::PathBuf;

use gpui::{
    AnyElement, App, FontWeight, Hsla, IntoElement, ObjectFit, ParentElement as _, Pixels, RenderOnce, SharedString,
    Styled as _, StyledImage as _, Window, div, img, px,
};
use vsesvit_core::profiles::{ProfileColor, avatar_initial};

use super::{colors, tokens};

#[derive(IntoElement)]
pub struct Avatar {
    initial: SharedString,
    color: ProfileColor,
    picture: Option<PathBuf>,
    large: bool,
}

impl Avatar {
    /// [`tokens::AVATAR`] across, or [`tokens::AVATAR_LARGE`]. A picture that fails to load
    /// shows the initial.
    pub fn new(name: &str, color: ProfileColor, picture: Option<PathBuf>, large: bool) -> Self {
        Avatar { initial: avatar_initial(name).into(), color, picture, large }
    }
}

impl RenderOnce for Avatar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let size = if self.large { tokens::AVATAR_LARGE } else { tokens::AVATAR };
        let (initial, color) = (self.initial, colors::profile(self.color));
        match self.picture {
            Some(path) => img(path)
                .size(size)
                .flex_none()
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || initial_circle(size, color, initial.clone()))
                .into_any_element(),
            None => initial_circle(size, color, initial),
        }
    }
}

fn initial_circle(size: Pixels, color: Hsla, initial: SharedString) -> AnyElement {
    div()
        .size(size)
        .flex_none()
        .rounded_full()
        .bg(color)
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui::white())
        .font_weight(FontWeight::SEMIBOLD)
        // WinUI's ratio: 12 px in the small circle, 39 px in the large one.
        .text_size(px((f32::from(size) * 11. / 20.).floor()))
        .child(initial)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_initial_is_cores() {
        for (name, initial) in [("Work", "W"), ("  élan", "É"), ("--", "?"), ("Person 2", "P")] {
            assert_eq!(Avatar::new(name, ProfileColor::Blue, None, false).initial, initial);
        }
    }
}
