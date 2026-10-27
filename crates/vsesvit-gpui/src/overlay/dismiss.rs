//! Owned by W2.1.
//! The overlays' bookkeeping (UA section 1.7): which overlays are open, how they nest, where each was
//! last drawn, and which of them a press, a window change or a close takes down. Pure, so the rules
//! have plain unit tests; `OverlayHost` keeps one [`Book`] and does what it answers.
//!
//! Overlays form a forest. A submenu is the child of the menu it opened from; closing an overlay
//! closes everything opened from it, newest first. Presses and window changes are judged per
//! root: a press inside any overlay of a family, or on the root's anchor, leaves the family open.

use std::time::Duration;

use gpui::{Bounds, Pixels, Point, px, size};

/// One open overlay, unique for the life of its host.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OverlayId(u64);

impl OverlayId {
    pub(crate) fn raw(self) -> u64 {
        self.0
    }
}

/// What closes an overlay besides its owner. The owner can always close it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Dismiss {
    /// Escape while the overlay has the keyboard.
    pub escape: bool,
    /// A press outside the overlay, its descendants and its anchor: in the window or anywhere on the
    /// screen (`Desktop::pointer_presses`), the page included.
    pub outside_press: bool,
    /// The window moves, resizes or stops being active.
    pub window_change: bool,
    /// Closes by itself this long after it opened.
    pub timeout: Option<Duration>,
}

impl Dismiss {
    /// Menus, bubbles and popovers: Escape, an outside press and a window change all close them.
    pub const TRANSIENT: Dismiss = Dismiss { escape: true, outside_press: true, window_change: true, timeout: None };
    /// Only the owner closes it: a list an entry drives, a persistent prompt.
    pub const OWNER: Dismiss = Dismiss { escape: false, outside_press: false, window_change: false, timeout: None };
}

impl Default for Dismiss {
    fn default() -> Dismiss {
        Dismiss::TRANSIENT
    }
}

/// Why an overlay closed. Every overlay closes exactly once, with one of these.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DismissReason {
    /// Its owner closed it (`OverlayHost::close`, `close_kind`).
    Owner,
    /// A menu item was chosen; the whole menu closes before the item runs.
    Chosen,
    Escape,
    OutsidePress,
    /// The window moved, resized or was deactivated.
    WindowChanged,
    Timeout,
    /// Another overlay took its place (`BubbleSlot`, a second menu of the same owner).
    Replaced,
    /// The overlay it was opened from closed.
    ParentClosed,
    /// The window and its host went.
    WindowClosed,
}

/// How far around a point anchor a press still counts as a press on the anchor: the right-click that
/// opened a context menu is not an outside press, whichever way the menu was flipped.
pub const POINT_ANCHOR_SLOP: Pixels = px(4.);

/// One open overlay, with what the host keeps for it in `T`.
#[derive(Debug)]
pub(crate) struct Entry<T> {
    pub id: OverlayId,
    pub parent: Option<OverlayId>,
    /// In window pixels. A point anchor is a rectangle of zero size.
    pub anchor: Bounds<Pixels>,
    pub dismiss: Dismiss,
    /// Where it was drawn in the last frame, in window pixels; `None` before its first frame.
    pub rect: Option<Bounds<Pixels>>,
    pub payload: T,
}

/// The open overlays in the order they opened, which is also the order they paint in.
#[derive(Debug)]
pub(crate) struct Book<T> {
    next: u64,
    entries: Vec<Entry<T>>,
}

impl<T> Default for Book<T> {
    fn default() -> Book<T> {
        Book { next: 1, entries: Vec::new() }
    }
}

impl<T> Book<T> {
    /// Opens an overlay. A parent that is no longer open makes it a root.
    pub fn open(&mut self, parent: Option<OverlayId>, anchor: Bounds<Pixels>, dismiss: Dismiss, payload: T) -> OverlayId {
        let id = OverlayId(self.next);
        self.next += 1;
        let parent = parent.filter(|p| self.get(*p).is_some());
        self.entries.push(Entry { id, parent, anchor, dismiss, rect: None, payload });
        id
    }

    pub fn get(&self, id: OverlayId) -> Option<&Entry<T>> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: OverlayId) -> Option<&mut Entry<T>> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Entry<T>> {
        self.entries.iter()
    }

    /// Closes `id` and every overlay opened from it, newest first; `id` itself comes last.
    pub fn close(&mut self, id: OverlayId) -> Vec<Entry<T>> {
        if self.get(id).is_none() {
            return Vec::new();
        }
        let doomed: Vec<OverlayId> = self.entries.iter().rev().filter(|e| self.descends_from(e.id, id)).map(|e| e.id).collect();
        doomed.into_iter().filter_map(|d| self.entries.iter().position(|e| e.id == d).map(|i| self.entries.remove(i))).collect()
    }

    /// Whether `id` is `ancestor` or was opened from it, at any depth.
    fn descends_from(&self, id: OverlayId, ancestor: OverlayId) -> bool {
        let mut at = Some(id);
        while let Some(current) = at {
            if current == ancestor {
                return true;
            }
            at = self.get(current).and_then(|e| e.parent);
        }
        false
    }

    pub fn root_of(&self, id: OverlayId) -> OverlayId {
        let mut at = id;
        while let Some(parent) = self.get(at).and_then(|e| e.parent) {
            at = parent;
        }
        at
    }

    /// Records where each overlay was drawn this frame.
    pub fn set_rects(&mut self, rects: &[(OverlayId, Bounds<Pixels>)]) {
        for entry in &mut self.entries {
            entry.rect = rects.iter().find(|(id, _)| *id == entry.id).map(|&(_, r)| r);
        }
    }

    /// The roots a press at `at` (window pixels) closes. A family stays open when the press is on
    /// any of its overlays or on its root's anchor; a root not drawn yet stays open too, since the
    /// press cannot have been aimed at it.
    pub fn outside_press(&self, at: Point<Pixels>) -> Vec<OverlayId> {
        self.roots()
            .filter(|root| root.dismiss.outside_press && root.rect.is_some())
            .filter(|root| !on_anchor(root.anchor, at))
            .filter(|root| !self.entries.iter().any(|e| self.descends_from(e.id, root.id) && e.rect.is_some_and(|r| r.contains(&at))))
            .map(|root| root.id)
            .collect()
    }

    /// The roots a window move, resize or deactivation closes.
    pub fn window_changed(&self) -> Vec<OverlayId> {
        self.roots().filter(|root| root.dismiss.window_change).map(|root| root.id).collect()
    }

    fn roots(&self) -> impl Iterator<Item = &Entry<T>> {
        self.entries.iter().filter(|e| e.parent.is_none())
    }
}

fn on_anchor(anchor: Bounds<Pixels>, at: Point<Pixels>) -> bool {
    if anchor.size.width > px(0.) && anchor.size.height > px(0.) {
        return anchor.contains(&at);
    }
    let slop = POINT_ANCHOR_SLOP;
    Bounds { origin: anchor.origin - Point::new(slop, slop), size: size(slop * 2., slop * 2.) }.contains(&at)
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) }
    }

    fn at(x: f32, y: f32) -> Point<Pixels> {
        point(px(x), px(y))
    }

    /// A menu under a button at (10, 10, 30, 20), drawn at (10, 34, 200, 300), with a submenu beside
    /// its second row.
    fn menu_with_submenu() -> (Book<&'static str>, OverlayId, OverlayId) {
        let mut book = Book::default();
        let menu = book.open(None, rect(10., 10., 30., 20.), Dismiss::TRANSIENT, "menu");
        let sub = book.open(Some(menu), rect(10., 66., 200., 28.), Dismiss::TRANSIENT, "sub");
        book.set_rects(&[(menu, rect(10., 34., 200., 300.)), (sub, rect(210., 62., 180., 120.))]);
        (book, menu, sub)
    }

    #[test]
    fn closing_an_overlay_closes_what_opened_from_it_newest_first() {
        let (mut book, menu, sub) = menu_with_submenu();
        let deeper = book.open(Some(sub), rect(0., 0., 1., 1.), Dismiss::TRANSIENT, "deeper");
        let other = book.open(None, rect(0., 0., 1., 1.), Dismiss::OWNER, "other");
        let closed: Vec<_> = book.close(menu).into_iter().map(|e| e.id).collect();
        assert_eq!(closed, [deeper, sub, menu]);
        assert!(book.get(other).is_some());
        assert!(book.close(menu).is_empty(), "a second close finds nothing, so nothing closes twice");
    }

    #[test]
    fn closing_a_child_leaves_its_parent_open() {
        let (mut book, menu, sub) = menu_with_submenu();
        assert_eq!(book.close(sub).len(), 1);
        assert!(book.get(menu).is_some());
    }

    #[test]
    fn a_parent_that_is_gone_makes_a_root() {
        let mut book = Book::default();
        let gone = book.open(None, rect(0., 0., 1., 1.), Dismiss::TRANSIENT, ());
        book.close(gone);
        let orphan = book.open(Some(gone), rect(0., 0., 1., 1.), Dismiss::TRANSIENT, ());
        assert_eq!(book.get(orphan).unwrap().parent, None);
        assert_eq!(book.root_of(orphan), orphan);
    }

    #[test]
    fn a_press_inside_the_family_or_on_the_anchor_keeps_it_open() {
        let (book, menu, sub) = menu_with_submenu();
        assert_eq!(book.root_of(sub), menu);
        assert!(book.outside_press(at(50., 100.)).is_empty(), "in the menu");
        assert!(book.outside_press(at(300., 100.)).is_empty(), "in the submenu");
        assert!(book.outside_press(at(20., 15.)).is_empty(), "on the button that opened it");
        assert_eq!(book.outside_press(at(600., 500.)), [menu], "anywhere else");
    }

    #[test]
    fn a_point_anchor_keeps_the_press_that_opened_the_menu() {
        let mut book = Book::default();
        let menu = book.open(None, rect(300., 200., 0., 0.), Dismiss::TRANSIENT, ());
        // Flipped up and left: the right-click point is the menu's bottom-right corner, outside it.
        book.set_rects(&[(menu, rect(100., 50., 200., 150.))]);
        assert!(book.outside_press(at(300., 200.)).is_empty());
        assert!(book.outside_press(at(302., 203.)).is_empty());
        assert_eq!(book.outside_press(at(310., 200.)), [menu]);
    }

    #[test]
    fn presses_spare_overlays_that_ignore_them_or_are_not_drawn_yet() {
        let mut book = Book::default();
        let list = book.open(None, rect(0., 0., 100., 20.), Dismiss::OWNER, ());
        let fresh = book.open(None, rect(0., 0., 100., 20.), Dismiss::TRANSIENT, ());
        book.set_rects(&[(list, rect(0., 24., 100., 100.))]);
        assert!(book.outside_press(at(500., 500.)).is_empty());
        book.set_rects(&[(list, rect(0., 24., 100., 100.)), (fresh, rect(0., 24., 100., 100.))]);
        assert_eq!(book.outside_press(at(500., 500.)), [fresh]);
    }

    #[test]
    fn a_window_change_closes_only_the_roots_that_ask_for_it() {
        let mut book = Book::default();
        let menu = book.open(None, rect(0., 0., 1., 1.), Dismiss::TRANSIENT, ());
        let _sub = book.open(Some(menu), rect(0., 0., 1., 1.), Dismiss::TRANSIENT, ());
        let _list = book.open(None, rect(0., 0., 1., 1.), Dismiss::OWNER, ());
        assert_eq!(book.window_changed(), [menu]);
    }
}
