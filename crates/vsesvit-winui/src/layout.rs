//! Where the tab list goes (`tabs.position`), and reading that back from widget geometry.

use vsesvit_core::prefs::TabsPosition;

/// Which of the window's two tab lists is live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StripKind {
    /// The `TabView` strip in the title bar.
    Top,
    /// The vertical pane beside the web content.
    Side,
}

impl StripKind {
    pub fn of(position: TabsPosition) -> Self {
        match position {
            TabsPosition::Top => Self::Top,
            TabsPosition::Left | TabsPosition::Right => Self::Side,
        }
    }
}

/// An element's bounds in window coordinates (DIPs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// Layout rounding can leave a fraction of a pixel between neighbours.
const SLACK: f64 = 1.0;

/// The layout the geometry shows: exactly one of the vertical pane (`pane`) and the horizontal
/// strip (`strip`) is shown (`None` when collapsed), and it sits beside or above `view`.
pub(crate) fn observed(
    pane: Option<Rect>,
    strip: Option<Rect>,
    view: Rect,
) -> Option<TabsPosition> {
    let pane = pane.filter(|r| !r.is_empty());
    let strip = strip.filter(|r| !r.is_empty());
    if view.is_empty() {
        return None;
    }
    match (pane, strip) {
        (Some(pane), None) if pane.right() <= view.x + SLACK => Some(TabsPosition::Left),
        (Some(pane), None) if pane.x >= view.right() - SLACK => Some(TabsPosition::Right),
        (None, Some(strip)) if strip.bottom() <= view.y + SLACK => Some(TabsPosition::Top),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    const VIEW_RIGHT_OF_PANE: Rect = Rect {
        x: 240.0,
        y: 72.0,
        width: 1040.0,
        height: 700.0,
    };

    #[test]
    fn pane_left_or_right_of_the_view() {
        let pane = rect(0.0, 72.0, 240.0, 700.0);
        assert_eq!(
            observed(Some(pane), None, VIEW_RIGHT_OF_PANE),
            Some(TabsPosition::Left)
        );
        let view = rect(0.0, 72.0, 1040.0, 700.0);
        let pane = rect(1040.0, 72.0, 240.0, 700.0);
        assert_eq!(observed(Some(pane), None, view), Some(TabsPosition::Right));
    }

    #[test]
    fn strip_above_the_view() {
        let strip = rect(0.0, 0.0, 1280.0, 40.0);
        let view = rect(0.0, 112.0, 1280.0, 660.0);
        assert_eq!(observed(None, Some(strip), view), Some(TabsPosition::Top));
        assert_eq!(
            observed(Some(rect(0.0, 0.0, 0.0, 0.0)), Some(strip), view),
            Some(TabsPosition::Top)
        );
    }

    #[test]
    fn overlaps_and_ambiguity_are_not_a_layout() {
        let pane = rect(0.0, 72.0, 300.0, 700.0);
        assert_eq!(observed(Some(pane), None, VIEW_RIGHT_OF_PANE), None);
        let strip = rect(0.0, 0.0, 1280.0, 40.0);
        assert_eq!(
            observed(
                Some(rect(0.0, 72.0, 240.0, 700.0)),
                Some(strip),
                VIEW_RIGHT_OF_PANE
            ),
            None
        );
        assert_eq!(observed(None, None, VIEW_RIGHT_OF_PANE), None);
        assert_eq!(observed(None, Some(strip), rect(0.0, 0.0, 0.0, 0.0)), None);
    }

    #[test]
    fn strip_kind_follows_the_position() {
        assert_eq!(StripKind::of(TabsPosition::Left), StripKind::Side);
        assert_eq!(StripKind::of(TabsPosition::Right), StripKind::Side);
        assert_eq!(StripKind::of(TabsPosition::Top), StripKind::Top);
    }
}
