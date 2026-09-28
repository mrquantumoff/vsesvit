//! Dragging bookmarks within the bookmarks bar and the Bookmarks dialog. The payload is the
//! dragged node's id as a string, and a drop is one core `move_to`, so the bar, the dialog
//! and every other device agree on the result.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, InsertAt, NodeKind};

use crate::profile::Core;

const ZONE_CLASSES: [&str; 3] = ["drop-before", "drop-into", "drop-after"];

/// Which part of a target the pointer is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zone {
    Before,
    Into,
    After,
}

impl Zone {
    fn class(self) -> &'static str {
        match self {
            Zone::Before => ZONE_CLASSES[0],
            Zone::Into => ZONE_CLASSES[1],
            Zone::After => ZONE_CLASSES[2],
        }
    }
}

/// The zone at `offset` along a target `extent` long. A folder takes its middle half as
/// "into" and keeps its outer quarters for reordering; a root only takes drops into it.
pub(crate) fn zone(target: &BookmarkNode, offset: f64, extent: f64) -> Zone {
    let at = if extent > 0.0 { offset / extent } else { 0.5 };
    match target.kind {
        NodeKind::Folder if target.id.is_root() || (0.25..0.75).contains(&at) => Zone::Into,
        _ if at < 0.5 => Zone::Before,
        _ => Zone::After,
    }
}

/// The parent and index `dragged` moves to when dropped on `zone` of `target`, or `None`
/// when it would stay where it is. Core counts the index among the parent's children
/// without the moved node, so a node moving down within its own parent lands one below
/// the gap it was dropped in.
pub(crate) fn destination(dragged: &BookmarkNode, target: &BookmarkNode, zone: Zone) -> Option<(BookmarkId, InsertAt)> {
    if dragged.id == target.id {
        return None;
    }
    let gap = match zone {
        Zone::Into => return Some((target.id, InsertAt::End)),
        Zone::Before => target.index,
        Zone::After => target.index + 1,
    };
    let same_parent = dragged.parent == target.parent;
    let index = if same_parent && dragged.index < gap { gap - 1 } else { gap };
    if same_parent && index == dragged.index {
        return None;
    }
    Some((target.parent, InsertAt::Index(index)))
}

/// Moves `id` to `zone` of `target`. Returns whether anything moved.
pub(crate) fn apply(core: &Core, id: BookmarkId, target: &BookmarkNode, zone: Zone) -> Result<bool, vsesvit_core::Error> {
    let mut profile = core.borrow_mut();
    let mut bookmarks = profile.bookmarks();
    let Some(dragged) = bookmarks.get(id) else { return Ok(false) };
    let Some((parent, at)) = destination(&dragged, target, zone) else { return Ok(false) };
    bookmarks.move_to(id, parent, at).map(|()| true)
}

/// Makes its widget draggable as the bookmark `id` returns; `None` (a root) refuses the drag.
pub(crate) fn drag_source(id: impl Fn() -> Option<BookmarkId> + 'static) -> gtk::DragSource {
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    source.connect_prepare(move |source, x, y| {
        let id = id()?;
        if let Some(widget) = source.widget() {
            source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&widget))), x as i32, y as i32);
        }
        Some(gdk::ContentProvider::for_value(&id.0.to_string().to_value()))
    });
    source
}

/// Takes dropped bookmarks on its widget, which shows `target` (read at drop time, as list
/// rows are rebound). `axis` is the direction the target's siblings run in. `on_drop` runs
/// once the drop has finished, so it may rebuild the widgets the drag involved.
pub(crate) fn drop_target(
    axis: gtk::Orientation,
    target: impl Fn() -> Option<BookmarkNode> + 'static,
    on_drop: impl Fn(BookmarkId, BookmarkNode, Zone) + 'static,
) -> gtk::DropTarget {
    let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    let target = Rc::new(target);
    let zone_at = {
        let target = target.clone();
        move |drop: &gtk::DropTarget, x: f64, y: f64| {
            let widget = drop.widget()?;
            let node = target()?;
            let zone = match axis {
                gtk::Orientation::Horizontal => {
                    let width = f64::from(widget.width());
                    let x = if widget.direction() == gtk::TextDirection::Rtl { width - x } else { x };
                    zone(&node, x, width)
                }
                _ => zone(&node, y, f64::from(widget.height())),
            };
            Some((widget, node, zone))
        }
    };
    let zone_at = Rc::new(zone_at);
    drop.connect_motion({
        let zone_at = zone_at.clone();
        move |drop, x, y| {
            if let Some((widget, _, zone)) = zone_at(drop, x, y) {
                mark(&widget, Some(zone));
            }
            gdk::DragAction::MOVE
        }
    });
    drop.connect_leave(|drop| {
        if let Some(widget) = drop.widget() {
            mark(&widget, None);
        }
    });
    let on_drop = Rc::new(on_drop);
    drop.connect_drop(move |drop, value, x, y| {
        let id = value.get::<String>().ok().and_then(|s| s.parse().ok()).map(BookmarkId);
        let (Some(id), Some((widget, node, zone))) = (id, zone_at(drop, x, y)) else {
            return false;
        };
        mark(&widget, None);
        let on_drop = on_drop.clone();
        glib::idle_add_local_once(move || on_drop(id, node, zone));
        true
    });
    drop
}

fn mark(widget: &gtk::Widget, zone: Option<Zone>) {
    for class in ZONE_CLASSES {
        widget.remove_css_class(class);
    }
    if let Some(zone) = zone {
        widget.add_css_class(zone.class());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u128, kind: NodeKind, parent: BookmarkId, index: usize) -> BookmarkNode {
        BookmarkNode {
            id: BookmarkId(format!("00000000-0000-0000-0000-{id:012}").parse().unwrap()),
            kind,
            parent,
            index,
            title: String::new(),
            url: None,
            added_ms: 0,
        }
    }

    fn url(id: u128, parent: BookmarkId, index: usize) -> BookmarkNode {
        node(id + 100, NodeKind::Url, parent, index)
    }

    #[test]
    fn zones_split_leaves_in_halves_and_folders_in_quarters() {
        let leaf = url(1, BookmarkId::TOOLBAR, 0);
        assert_eq!(zone(&leaf, 10.0, 100.0), Zone::Before);
        assert_eq!(zone(&leaf, 60.0, 100.0), Zone::After);
        let folder = node(200, NodeKind::Folder, BookmarkId::TOOLBAR, 1);
        assert_eq!(zone(&folder, 10.0, 100.0), Zone::Before);
        assert_eq!(zone(&folder, 50.0, 100.0), Zone::Into);
        assert_eq!(zone(&folder, 90.0, 100.0), Zone::After);
        let root = BookmarkNode { id: BookmarkId::OTHER, ..folder };
        assert_eq!(zone(&root, 1.0, 100.0), Zone::Into);
    }

    #[test]
    fn indices_count_siblings_without_the_moved_node() {
        let bar = BookmarkId::TOOLBAR;
        // [A, B, C, D]
        let (a, b, c, d) = (url(1, bar, 0), url(2, bar, 1), url(3, bar, 2), url(4, bar, 3));
        assert_eq!(destination(&d, &a, Zone::Before), Some((bar, InsertAt::Index(0))));
        assert_eq!(destination(&a, &c, Zone::After), Some((bar, InsertAt::Index(2))));
        assert_eq!(destination(&a, &d, Zone::After), Some((bar, InsertAt::Index(3))));
        assert_eq!(destination(&a, &c, Zone::Before), Some((bar, InsertAt::Index(1))));
        assert_eq!(destination(&c, &a, Zone::After), Some((bar, InsertAt::Index(1))));
        assert_eq!(destination(&b, &a, Zone::After), None, "already right after A");
        assert_eq!(destination(&b, &c, Zone::Before), None, "already right before C");
        assert_eq!(destination(&b, &b, Zone::Before), None);
    }

    #[test]
    fn drops_into_other_folders_keep_the_gap() {
        let bar = BookmarkId::TOOLBAR;
        let folder = node(9, NodeKind::Folder, bar, 1);
        let dragged = url(1, bar, 0);
        let inside = url(2, folder.id, 0);
        assert_eq!(destination(&dragged, &folder, Zone::Into), Some((folder.id, InsertAt::End)));
        assert_eq!(destination(&dragged, &inside, Zone::After), Some((folder.id, InsertAt::Index(1))));
        assert_eq!(destination(&inside, &dragged, Zone::Before), Some((bar, InsertAt::Index(0))));
    }
}
