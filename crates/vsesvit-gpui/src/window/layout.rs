//! Owned by W1.7.
//! Where a window puts its tabs (UA sections 1.5 and 2.1): a sidebar on the left or the right, or a
//! strip on top, from the synced `tabs.position` pref; whether the sidebar shows; and the probe the
//! self-test's `tab_layout` reads the result back from, which the window records as it lays out.

use gpui::{Bounds, Pixels};
use vsesvit_core::prefs::TabsPosition;

use crate::theme::tokens;

/// The window's tab layout.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub position: TabsPosition,
    /// The user's choice for a window wide enough for a sidebar: shown unless toggled off.
    pub wide_shown: bool,
    /// The same for a window narrower than [`tokens::SIDEBAR_HIDE_BELOW`]: hidden unless toggled on,
    /// and then it pushes the page aside (decision D4).
    pub narrow_shown: bool,
}

impl Layout {
    pub fn new(position: TabsPosition) -> Layout {
        Layout { position, wide_shown: true, narrow_shown: false }
    }

    /// The sidebar's width in a window `width` wide, when it shows.
    pub fn sidebar_width(&self, width: Pixels) -> Option<Pixels> {
        if self.position == TabsPosition::Top {
            return None;
        }
        match tokens::sidebar_width(width) {
            Some(w) => self.wide_shown.then_some(w),
            None => self.narrow_shown.then_some(tokens::SIDEBAR_MIN_W),
        }
    }

    /// Ctrl+S and F9: show or hide the sidebar, for the window's current width.
    pub fn toggle_sidebar(&mut self, width: Pixels) {
        if tokens::sidebar_width(width).is_some() {
            self.wide_shown = !self.wide_shown;
        } else {
            self.narrow_shown = !self.narrow_shown;
        }
    }
}

/// Where the tab widgets and the page were last laid out, in window coordinates; `None` for one not
/// shown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutProbe {
    /// The window's width.
    pub width: f32,
    pub sidebar: Option<Bounds<Pixels>>,
    pub tab_strip: Option<Bounds<Pixels>>,
    pub page: Option<Bounds<Pixels>>,
}

/// The tabs position a settled layout shows, if it shows one cleanly: the sidebar wholly inside the
/// window and the tab widgets plus the page spanning its width, so a frame caught mid-change
/// classifies as nothing. Half a pixel of slack covers fractional positions on scaled displays.
pub fn classify(probe: &LayoutProbe) -> Option<TabsPosition> {
    const SLACK: f32 = 0.5;
    let shown = |b: &Option<Bounds<Pixels>>| b.filter(|b| f32::from(b.size.width) > 0. && f32::from(b.size.height) > 0.);
    let page = shown(&probe.page)?;
    let edges = |b: Bounds<Pixels>| -> (f32, f32, f32, f32) { (b.left().into(), b.right().into(), b.top().into(), b.bottom().into()) };
    let (left, right, top, _) = edges(page);
    let at_left = |x: f32| x.abs() <= SLACK;
    let at_right = |x: f32| (x - probe.width).abs() <= SLACK;
    match (shown(&probe.sidebar), shown(&probe.tab_strip)) {
        (Some(side), None) => {
            let (s_left, s_right, _, _) = edges(side);
            if at_left(s_left) && s_right <= left + SLACK && at_right(right) {
                Some(TabsPosition::Left)
            } else if at_left(left) && right <= s_left + SLACK && at_right(s_right) {
                Some(TabsPosition::Right)
            } else {
                None
            }
        }
        (None, Some(strip)) => {
            let (_, _, _, strip_bottom) = edges(strip);
            (strip_bottom <= top + SLACK && at_left(left) && at_right(right)).then_some(TabsPosition::Top)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size};

    use super::*;

    const WIDTH: f32 = 1280.;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Bounds<Pixels>> {
        Some(Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) })
    }

    fn probe(sidebar: Option<Bounds<Pixels>>, tab_strip: Option<Bounds<Pixels>>, page: Option<Bounds<Pixels>>) -> LayoutProbe {
        LayoutProbe { width: WIDTH, sidebar, tab_strip, page }
    }

    #[test]
    fn frames_caught_mid_change_or_overlapping_classify_as_nothing() {
        let sliding_in = probe(rect(-112., 46., 256., 700.), None, rect(144., 46., 1136., 700.));
        assert_eq!(classify(&sliding_in), None, "the sidebar is still outside the window");
        let growing = probe(None, rect(0., 46., 1280., 38.), rect(0., 84., 1147., 700.));
        assert_eq!(classify(&growing), None, "the page does not span the window yet");
        let overlay = probe(rect(0., 46., 256., 700.), None, rect(0., 46., 1280., 700.));
        assert_eq!(classify(&overlay), None, "a sidebar over the page");
        let both = probe(rect(0., 46., 256., 700.), rect(0., 46., 1280., 38.), rect(256., 84., 1024., 700.));
        assert_eq!(classify(&both), None);
        assert_eq!(classify(&probe(None, None, rect(0., 46., 1280., 700.))), None);
        assert_eq!(classify(&probe(rect(0., 46., 0., 700.), None, rect(0., 46., 1280., 700.))), None, "a zero-width sidebar");
    }

    #[test]
    fn the_sidebar_follows_its_toggle_for_the_width_it_was_toggled_at() {
        let mut layout = Layout::new(TabsPosition::Left);
        assert_eq!(layout.sidebar_width(px(1280.)), Some(px(256.)));
        assert_eq!(layout.sidebar_width(px(700.)), None, "hidden below 720 px");
        layout.toggle_sidebar(px(700.));
        assert_eq!(layout.sidebar_width(px(700.)), Some(tokens::SIDEBAR_MIN_W), "toggled on while narrow, it pushes the page");
        assert_eq!(layout.sidebar_width(px(1280.)), Some(px(256.)), "the wide choice stands");
        layout.toggle_sidebar(px(1280.));
        assert_eq!(layout.sidebar_width(px(1280.)), None);
        assert_eq!(Layout::new(TabsPosition::Top).sidebar_width(px(1280.)), None);
    }
}
