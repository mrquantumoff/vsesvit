//! Owned by W2.1.
//! Where an in-window overlay goes (design section 0: overlays are in-window GPUI elements plus a
//! hole in the content host). [`place`] is the rule, pure; [`Placed`] is the element that measures
//! its child, puts it where [`place`] says and reports the rectangle, which becomes the overlay's
//! hole and its target for outside presses.

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    Point, Position, Size, Style, Window, point, px,
};

/// How close an overlay may come to the window's edges.
pub const WINDOW_MARGIN: Pixels = px(8.);
/// The space between an anchor and an overlay below or above it.
pub const GAP: Pixels = px(4.);
/// A submenu rises by its panel's padding, so its first row lines up with the row that opened it.
pub const SUBMENU_LIFT: Pixels = px(4.);

/// Where an overlay goes relative to its anchor. Every placement stays inside the window: one that
/// does not fit flips to the other side of its anchor if that fits, and is pushed in otherwise.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Placement {
    /// Under the anchor (above it when there is no room below), aligned with one of its edges.
    Below(Align),
    /// Beside the anchor, top edges lined up: right of it, or left when there is no room. Submenus.
    Beside,
    /// Its top-left corner at the anchor's origin, flipped left or up when there is no room.
    /// Context menus.
    AtPoint,
    /// Inside the anchor, `inset` from one of its edges: toasts, notices, the link preview over the
    /// page's rectangle.
    Inside { spot: Spot, inset: Pixels },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Align {
    /// Left edges lined up.
    Start,
    Centre,
    /// Right edges lined up.
    End,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    TopCentre,
    BottomCentre,
    BottomLeft,
}

/// The origin of an overlay of `size` placed against `anchor`, both in window pixels, inside
/// `limits` (the window less [`WINDOW_MARGIN`]).
pub fn place(anchor: Bounds<Pixels>, placement: Placement, size: Size<Pixels>, limits: Bounds<Pixels>) -> Point<Pixels> {
    let (w, h) = (size.width, size.height);
    let (left, right) = (limits.left(), limits.right());
    let (top, bottom) = (limits.top(), limits.bottom());
    match placement {
        Placement::Below(align) => {
            let x = match align {
                Align::Start => anchor.left(),
                Align::Centre => anchor.left() + (anchor.size.width - w) / 2.,
                Align::End => anchor.right() - w,
            };
            let y = fit(anchor.bottom() + GAP, anchor.top() - GAP - h, h, top, bottom);
            point(clamp(x, w, left, right), y)
        }
        Placement::Beside => {
            let x = fit(anchor.right(), anchor.left() - w, w, left, right);
            point(x, clamp(anchor.top() - SUBMENU_LIFT, h, top, bottom))
        }
        Placement::AtPoint => {
            let at = anchor.origin;
            point(fit(at.x, at.x - w, w, left, right), fit(at.y, at.y - h, h, top, bottom))
        }
        Placement::Inside { spot, inset } => {
            let centre = anchor.left() + (anchor.size.width - w) / 2.;
            let (x, y) = match spot {
                Spot::TopCentre => (centre, anchor.top() + inset),
                Spot::BottomCentre => (centre, anchor.bottom() - inset - h),
                Spot::BottomLeft => (anchor.left() + inset, anchor.bottom() - inset - h),
            };
            point(clamp(x, w, left, right), clamp(y, h, top, bottom))
        }
    }
}

/// `preferred` if a span of `len` fits there between `low` and `high`, else `other` if it fits,
/// else `preferred` pushed inside.
fn fit(preferred: Pixels, other: Pixels, len: Pixels, low: Pixels, high: Pixels) -> Pixels {
    let fits = |start: Pixels| start >= low && start + len <= high;
    if fits(preferred) {
        preferred
    } else if fits(other) {
        other
    } else {
        clamp(preferred, len, low, high)
    }
}

/// `start` moved so a span of `len` stays between `low` and `high`; a span longer than that starts at
/// `low`.
fn clamp(start: Pixels, len: Pixels, low: Pixels, high: Pixels) -> Pixels {
    start.min(high - len).max(low)
}

/// The window's area an overlay may use.
pub fn limits(window: &Window) -> Bounds<Pixels> {
    let size = window.viewport_size();
    Bounds { origin: point(WINDOW_MARGIN, WINDOW_MARGIN), size: Size { width: size.width - WINDOW_MARGIN * 2., height: size.height - WINDOW_MARGIN * 2. } }
}

type OnPlaced = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App)>;

/// Lays out its child at its natural size, moves it where [`place`] says, and hands the rectangle
/// it ended up in to `placed` during prepaint.
pub struct Placed {
    anchor: Bounds<Pixels>,
    placement: Placement,
    child: AnyElement,
    placed: Option<OnPlaced>,
}

impl Placed {
    pub fn new(
        anchor: Bounds<Pixels>,
        placement: Placement,
        child: impl IntoElement,
        placed: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Placed {
        Placed { anchor, placement, child: child.into_any_element(), placed: Some(Box::new(placed)) }
    }
}

impl IntoElement for Placed {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Placed {
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, LayoutId) {
        let child = self.child.request_layout(window, cx);
        let style = Style { position: Position::Absolute, ..Style::default() };
        (window.request_layout(style, [child], cx), child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut LayoutId,
        window: &mut Window,
        cx: &mut App,
    ) {
        let natural = window.layout_bounds(*child);
        let origin = place(self.anchor, self.placement, natural.size, limits(window));
        let origin = point(origin.x.round(), origin.y.round());
        window.with_element_offset(origin - natural.origin, |window| self.child.prepaint(window, cx));
        if let Some(placed) = self.placed.take() {
            placed(Bounds { origin, size: natural.size }, window, cx);
        }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut LayoutId, _: &mut (), window: &mut Window, cx: &mut App) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::*;

    const WINDOW: Bounds<Pixels> = Bounds { origin: point(px(8.), px(8.)), size: size(px(784.), px(584.)) };

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) }
    }

    fn at(placement: Placement, anchor: Bounds<Pixels>, w: f32, h: f32) -> (f32, f32) {
        let p = place(anchor, placement, size(px(w), px(h)), WINDOW);
        (f32::from(p.x), f32::from(p.y))
    }

    #[test]
    fn below_aligns_with_the_anchor_and_flips_above_when_there_is_no_room() {
        let button = rect(100., 10., 30., 30.);
        assert_eq!(at(Placement::Below(Align::Start), button, 200., 100.), (100., 44.));
        assert_eq!(at(Placement::Below(Align::End), button, 200., 100.), (8., 44.), "pushed in from the left edge");
        assert_eq!(at(Placement::Below(Align::Centre), rect(300., 10., 100., 30.), 200., 100.), (250., 44.));
        let low = rect(100., 500., 30., 30.);
        assert_eq!(at(Placement::Below(Align::Start), low, 200., 150.), (100., 346.));
    }

    #[test]
    fn below_pushes_in_when_neither_side_fits() {
        assert_eq!(at(Placement::Below(Align::Start), rect(700., 300., 30., 30.), 200., 400.), (592., 192.));
    }

    #[test]
    fn beside_goes_right_and_flips_left() {
        let row = rect(10., 100., 200., 28.);
        assert_eq!(at(Placement::Beside, row, 180., 120.), (210., 96.));
        let right_row = rect(600., 100., 180., 28.);
        assert_eq!(at(Placement::Beside, right_row, 180., 120.), (420., 96.));
        assert_eq!(at(Placement::Beside, rect(10., 560., 200., 28.), 180., 120.), (210., 472.), "raised to fit");
    }

    #[test]
    fn at_point_flips_on_each_axis_on_its_own() {
        let click = rect(700., 500., 0., 0.);
        assert_eq!(at(Placement::AtPoint, click, 200., 300.), (500., 200.));
        assert_eq!(at(Placement::AtPoint, rect(50., 50., 0., 0.), 200., 300.), (50., 50.));
        assert_eq!(at(Placement::AtPoint, rect(50., 500., 0., 0.), 200., 300.), (50., 200.));
    }

    #[test]
    fn inside_spots_sit_on_their_edge() {
        let page = rect(0., 80., 800., 520.);
        let inset = px(24.);
        assert_eq!(at(Placement::Inside { spot: Spot::BottomCentre, inset }, page, 300., 40.), (250., 536.));
        assert_eq!(at(Placement::Inside { spot: Spot::TopCentre, inset }, page, 300., 40.), (250., 104.));
        assert_eq!(at(Placement::Inside { spot: Spot::BottomLeft, inset: px(0.) }, page, 300., 20.), (8., 572.), "kept off the window's edge");
    }
}
