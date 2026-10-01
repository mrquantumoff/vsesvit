//! Which extension actions sit in the toolbar, and in what order.
//!
//! One synced preference, [`TOOLBAR`], holds an ordered list of `(id, pinned)` entries.
//! An extension with no entry is pinned after the others, so a new install shows up in the
//! toolbar. Entries of extensions this device does not have are kept: another device may
//! have them, and the list syncs as one value.
//!
//! The functions are pure. A shell reads the pref, calls [`layout`] to paint, and on a pin
//! toggle or a drag writes back what [`set_pinned`] or [`move_pinned`] returns.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::prefs::{Pref, Scope};

pub const TOOLBAR: Pref<Vec<Entry>> = Pref { key: "toolbar.extensions", scope: Scope::Synced, default: Vec::new };

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub pinned: bool,
}

/// What the toolbar shows: pinned actions in order, and the rest (reachable from the
/// extensions menu).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub pinned: Vec<String>,
    pub unpinned: Vec<String>,
}

/// `available`: ids of extensions that have a toolbar action, in install order.
pub fn layout(available: &[String], saved: &[Entry]) -> Layout {
    let (pinned, unpinned): (Vec<Entry>, Vec<Entry>) =
        entries(available, saved).into_iter().filter(|e| available.contains(&e.id)).partition(|e| e.pinned);
    Layout { pinned: pinned.into_iter().map(|e| e.id).collect(), unpinned: unpinned.into_iter().map(|e| e.id).collect() }
}

/// The list to save after pinning or unpinning `id`. An unpinned action keeps its place,
/// so pinning it again puts it back where it was.
pub fn set_pinned(available: &[String], saved: &[Entry], id: &str, pinned: bool) -> Vec<Entry> {
    let mut entries = entries(available, saved);
    match entries.iter_mut().find(|e| e.id == id) {
        Some(entry) => entry.pinned = pinned,
        None => entries.push(Entry { id: id.to_owned(), pinned }),
    }
    entries
}

/// The list to save after moving `id` to position `to` among the pinned actions (as
/// [`layout`] shows them; `to` past the end means last). Unpinned and absent entries keep
/// their places. Unchanged if `id` is not a shown pinned action.
pub fn move_pinned(available: &[String], saved: &[Entry], id: &str, to: usize) -> Vec<Entry> {
    let mut entries = entries(available, saved);
    let is_shown_pinned = |e: &Entry| e.pinned && available.contains(&e.id);
    let slots: Vec<usize> = (0..entries.len()).filter(|&i| is_shown_pinned(&entries[i])).collect();
    let mut order: Vec<Entry> = slots.iter().map(|&i| entries[i].clone()).collect();
    let Some(from) = order.iter().position(|e| e.id == id) else { return entries };
    let moved = order.remove(from);
    order.insert(to.min(order.len()), moved);
    for (slot, entry) in slots.into_iter().zip(order) {
        entries[slot] = entry;
    }
    entries
}

/// `saved` without repeated ids, then every available id it lacks, pinned, in install order.
/// Linear: the list comes from sync, so its length is not ours to choose.
fn entries(available: &[String], saved: &[Entry]) -> Vec<Entry> {
    let mut seen: HashSet<&str> = HashSet::with_capacity(saved.len() + available.len());
    let mut entries: Vec<Entry> = Vec::with_capacity(saved.len() + available.len());
    for entry in saved {
        if seen.insert(&entry.id) {
            entries.push(entry.clone());
        }
    }
    for id in available {
        if seen.insert(id) {
            entries.push(Entry { id: id.clone(), pinned: true });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(s: &[&str]) -> Vec<String> {
        s.iter().map(|&s| s.to_owned()).collect()
    }

    fn saved(s: &[(&str, bool)]) -> Vec<Entry> {
        s.iter().map(|&(id, pinned)| Entry { id: id.to_owned(), pinned }).collect()
    }

    fn layout_of(available: &[&str], saved: &[Entry]) -> (Vec<String>, Vec<String>) {
        let l = layout(&ids(available), saved);
        (l.pinned, l.unpinned)
    }

    #[test]
    fn with_nothing_saved_every_action_is_pinned_in_install_order() {
        assert_eq!(layout_of(&["a", "b", "c"], &[]), (ids(&["a", "b", "c"]), ids(&[])));
    }

    #[test]
    fn the_saved_order_and_pins_are_respected() {
        let s = saved(&[("c", true), ("a", false), ("b", true)]);
        assert_eq!(layout_of(&["a", "b", "c"], &s), (ids(&["c", "b"]), ids(&["a"])));
    }

    #[test]
    fn uninstalled_ids_are_not_shown_but_kept_in_the_saved_list() {
        let s = saved(&[("gone", true), ("b", true), ("gone-too", false), ("a", true)]);
        assert_eq!(layout_of(&["a", "b"], &s), (ids(&["b", "a"]), ids(&[])));
        assert_eq!(
            set_pinned(&ids(&["a", "b"]), &s, "a", false),
            saved(&[("gone", true), ("b", true), ("gone-too", false), ("a", false)])
        );
    }

    #[test]
    fn a_new_install_is_appended_pinned() {
        let s = saved(&[("b", true), ("a", false)]);
        assert_eq!(layout_of(&["a", "b", "new"], &s), (ids(&["b", "new"]), ids(&["a"])));
    }

    #[test]
    fn repeated_saved_ids_count_once() {
        let s = saved(&[("a", false), ("b", true), ("a", true)]);
        assert_eq!(layout_of(&["a", "b"], &s), (ids(&["b"]), ids(&["a"])));
    }

    #[test]
    fn a_huge_saved_list_lays_out_in_linear_time() {
        let s: Vec<Entry> = (0..100_000).map(|i| Entry { id: format!("x{i}"), pinned: true }).collect();
        let start = std::time::Instant::now();
        assert_eq!(layout_of(&["a"], &s), (ids(&["a"]), ids(&[])));
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
    }

    #[test]
    fn unpinning_keeps_the_place_so_pinning_again_restores_it() {
        let available = ids(&["a", "b", "c"]);
        let unpinned = set_pinned(&available, &[], "b", false);
        assert_eq!(unpinned, saved(&[("a", true), ("b", false), ("c", true)]));
        assert_eq!(layout(&available, &unpinned).pinned, ids(&["a", "c"]));
        let pinned = set_pinned(&available, &unpinned, "b", true);
        assert_eq!(layout(&available, &pinned).pinned, ids(&["a", "b", "c"]));
    }

    #[test]
    fn moving_reorders_pinned_actions_around_unpinned_and_absent_entries() {
        let available = ids(&["a", "b", "c", "d"]);
        let s = saved(&[("a", true), ("gone", true), ("b", false), ("c", true), ("d", true)]);
        let moved = move_pinned(&available, &s, "d", 0);
        assert_eq!(layout(&available, &moved).pinned, ids(&["d", "a", "c"]));
        assert_eq!(moved, saved(&[("d", true), ("gone", true), ("b", false), ("a", true), ("c", true)]));

        let moved = move_pinned(&available, &s, "a", 1);
        assert_eq!(layout(&available, &moved).pinned, ids(&["c", "a", "d"]));
    }

    #[test]
    fn moving_clamps_the_target_and_ignores_actions_that_are_not_shown_pinned() {
        let available = ids(&["a", "b", "c"]);
        let moved = move_pinned(&available, &[], "a", 99);
        assert_eq!(layout(&available, &moved).pinned, ids(&["b", "c", "a"]));

        let s = saved(&[("a", true), ("b", false)]);
        let all = saved(&[("a", true), ("b", false), ("c", true)]);
        assert_eq!(move_pinned(&available, &s, "b", 0), all, "unpinned");
        assert_eq!(move_pinned(&available, &s, "missing", 0), all, "not installed");
    }
}
