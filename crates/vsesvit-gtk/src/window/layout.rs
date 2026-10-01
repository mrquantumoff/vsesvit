//! The tab layout as a value: the window applies it, and the self-test reads it back from
//! widget geometry instead of trusting the widgets' own flags.

use gtk::PackType;
use vsesvit_core::prefs::TabsPosition;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Vertical tabs in the split view's sidebar, at the start or the end.
    Sidebar(PackType),
    /// A horizontal `AdwTabBar` above the content.
    TopBar,
}

impl Layout {
    pub(crate) fn for_position(position: TabsPosition) -> Layout {
        match position {
            TabsPosition::Left => Layout::Sidebar(PackType::Start),
            TabsPosition::Right => Layout::Sidebar(PackType::End),
            TabsPosition::Top => Layout::TopBar,
        }
    }
}

/// A widget's bounds in window coordinates.
#[cfg(any(test, feature = "self-test"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[cfg(any(test, feature = "self-test"))]
impl Rect {
    pub(crate) fn right(&self) -> f32 {
        self.x + self.width
    }

    pub(crate) fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// Where the tab widgets and the web view are on screen. `None` means the widget is hidden
/// or has no allocation.
#[cfg(any(test, feature = "self-test"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutProbe {
    /// The window's width; the sidebar slides in from outside it while it animates.
    pub(crate) width: f32,
    pub(crate) sidebar: Option<Rect>,
    pub(crate) tab_bar: Option<Rect>,
    pub(crate) web_view: Option<Rect>,
}

/// The tabs position a *settled* layout shows, if it shows one cleanly: the sidebar wholly
/// inside the window and the tab widgets plus the web view spanning its width, so a frame
/// caught mid-transition classifies as nothing. Half a pixel of slack covers fractional
/// positions from scaled displays.
#[cfg(any(test, feature = "self-test"))]
pub(crate) fn classify(probe: &LayoutProbe) -> Option<TabsPosition> {
    const SLACK: f32 = 0.5;
    let web = probe.web_view.filter(|r| r.width > 0.0 && r.height > 0.0)?;
    let sidebar = probe.sidebar.filter(|r| r.width > 0.0 && r.height > 0.0);
    let tab_bar = probe.tab_bar.filter(|r| r.width > 0.0 && r.height > 0.0);
    let at_left = |r: &Rect| r.x.abs() <= SLACK;
    let at_right = |r: &Rect| (r.right() - probe.width).abs() <= SLACK;
    match (sidebar, tab_bar) {
        (Some(side), None) if at_left(&side) && side.right() <= web.x + SLACK && at_right(&web) => {
            Some(TabsPosition::Left)
        }
        (Some(side), None) if at_left(&web) && web.right() <= side.x + SLACK && at_right(&side) => {
            Some(TabsPosition::Right)
        }
        (None, Some(bar)) if bar.bottom() <= web.y + SLACK && at_left(&web) && at_right(&web) => {
            Some(TabsPosition::Top)
        }
        _ => None,
    }
}

#[cfg(any(test, feature = "self-test"))]
impl std::fmt::Display for LayoutProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn rect(r: Option<Rect>) -> String {
            match r {
                Some(r) => format!("x={:.0}..{:.0} y={:.0}..{:.0}", r.x, r.right(), r.y, r.bottom()),
                None => "hidden".to_owned(),
            }
        }
        write!(
            f,
            "window width {:.0}; sidebar {}; tab bar {}; web view {}",
            self.width,
            rect(self.sidebar),
            rect(self.tab_bar),
            rect(self.web_view)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Option<Rect> {
        Some(Rect { x, y, width, height })
    }

    #[test]
    fn positions_map_to_layouts() {
        assert_eq!(Layout::for_position(TabsPosition::Left), Layout::Sidebar(PackType::Start));
        assert_eq!(Layout::for_position(TabsPosition::Right), Layout::Sidebar(PackType::End));
        assert_eq!(Layout::for_position(TabsPosition::Top), Layout::TopBar);
    }

    const WIDTH: f32 = 1280.0;

    fn probe(sidebar: Option<Rect>, tab_bar: Option<Rect>, web_view: Option<Rect>) -> LayoutProbe {
        LayoutProbe { width: WIDTH, sidebar, tab_bar, web_view }
    }

    #[test]
    fn geometry_classifies_each_layout() {
        let left = probe(rect(0.0, 100.0, 220.0, 700.0), None, rect(220.0, 100.0, 1060.0, 700.0));
        assert_eq!(classify(&left), Some(TabsPosition::Left));

        let right = probe(rect(1060.0, 100.0, 220.0, 700.0), None, rect(0.0, 100.0, 1060.0, 700.0));
        assert_eq!(classify(&right), Some(TabsPosition::Right));

        let top = probe(None, rect(0.0, 60.0, 1280.0, 40.0), rect(0.0, 100.0, 1280.0, 700.0));
        assert_eq!(classify(&top), Some(TabsPosition::Top));
    }

    #[test]
    fn frames_caught_mid_transition_classify_as_nothing() {
        let sliding_in = probe(rect(-112.0, 100.0, 256.0, 700.0), None, rect(144.0, 100.0, 1136.0, 700.0));
        assert_eq!(classify(&sliding_in), None, "the sidebar is still outside the window");
        let sliding_out = probe(rect(1200.0, 100.0, 256.0, 700.0), None, rect(0.0, 100.0, 1200.0, 700.0));
        assert_eq!(classify(&sliding_out), None, "the right sidebar is still outside the window");
        let content_growing = probe(None, rect(0.0, 60.0, 1280.0, 40.0), rect(0.0, 100.0, 1147.0, 700.0));
        assert_eq!(classify(&content_growing), None, "the web view does not span the window yet");
    }

    #[test]
    fn overlapping_or_missing_widgets_classify_as_nothing() {
        let web = rect(0.0, 100.0, 1280.0, 700.0);
        let overlay = probe(rect(0.0, 100.0, 220.0, 700.0), None, web);
        assert_eq!(classify(&overlay), None, "an overlay sidebar covers the web view");
        let both = probe(rect(0.0, 100.0, 220.0, 700.0), rect(0.0, 60.0, 1280.0, 40.0), rect(220.0, 100.0, 1060.0, 700.0));
        assert_eq!(classify(&both), None);
        let nothing = probe(None, None, web);
        assert_eq!(classify(&nothing), None);
        let zero = probe(rect(0.0, 0.0, 0.0, 700.0), None, web);
        assert_eq!(classify(&zero), None, "a zero-width sidebar is not shown");
    }
}
