//! Owned by W0.4.
//! The shell's two animations (UA section 3.3), and none at all when the desktop asks for reduced
//! motion: gpui-component's init reads the portal's `reduced-motion` on Linux and the "Animation
//! effects" switch on Windows into `cx.reduce_motion()`.

use std::time::Duration;

use gpui::{Animation, App};

pub const TAB_ROW: Duration = Duration::from_millis(180);
pub const BAR_REVEAL: Duration = Duration::from_millis(150);

/// A tab row appearing or leaving: its height and opacity, ease-out cubic.
pub fn tab_row(cx: &App) -> Option<Animation> {
    (!cx.reduce_motion()).then(|| Animation::new(TAB_ROW).with_easing(ease_out_cubic))
}

/// A bar (find, banner, bookmarks) sliding in or out.
pub fn bar_reveal(cx: &App) -> Option<Animation> {
    (!cx.reduce_motion()).then(|| Animation::new(BAR_REVEAL).with_easing(ease_out_cubic))
}

fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn reduced_motion_turns_the_animations_off(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(tab_row(cx).is_some() && bar_reveal(cx).is_some());
            cx.set_reduce_motion(true);
            assert!(tab_row(cx).is_none() && bar_reveal(cx).is_none());
        });
    }

    #[test]
    fn the_easing_starts_fast_and_lands() {
        assert_eq!(ease_out_cubic(0.), 0.);
        assert_eq!(ease_out_cubic(1.), 1.);
        assert!(ease_out_cubic(0.5) > 0.8);
    }
}
