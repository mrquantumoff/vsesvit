//! Where a tab is in its window, and which other tabs the tab menu's Close Other Tabs and Close
//! Tabs to the Right close there, the same in both shells. Pinned tabs lead a window's tabs.

/// A tab's `index` among its window's `count` tabs, the first `pinned` of which are pinned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabPlace {
    pub index: usize,
    pub count: usize,
    pub pinned: usize,
}

impl TabPlace {
    pub fn is_pinned(self) -> bool {
        self.index < self.pinned
    }

    /// The indices Close Other Tabs closes, last first. As in Chrome, pinned tabs stay open.
    pub fn closes_others(self) -> Vec<usize> {
        (self.pinned..self.count).rev().filter(|&i| i != self.index).collect()
    }

    /// The indices Close Tabs to the Right closes, last first; pinned tabs stay open.
    pub fn closes_after(self) -> Vec<usize> {
        ((self.index + 1).max(self.pinned)..self.count).rev().collect()
    }

    /// Move Tab to New Window only takes a tab that leaves others behind.
    pub fn can_move_out(self) -> bool {
        self.count > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(index: usize, count: usize, pinned: usize) -> TabPlace {
        TabPlace { index, count, pinned }
    }

    #[test]
    fn closing_others_and_after_leaves_pinned_tabs() {
        assert_eq!(place(1, 4, 0).closes_others(), [3, 2, 0]);
        assert_eq!(place(2, 4, 2).closes_others(), [3]);
        assert_eq!(place(0, 4, 2).closes_others(), [3, 2]);
        assert_eq!(place(1, 4, 0).closes_after(), [3, 2]);
        assert_eq!(place(0, 4, 2).closes_after(), [3, 2]);
        assert_eq!(place(3, 4, 0).closes_after(), Vec::<usize>::new());
    }

    #[test]
    fn a_tab_moves_to_a_new_window_only_if_others_stay() {
        assert!(!place(0, 1, 0).can_move_out());
        assert!(place(0, 2, 0).can_move_out());
    }
}
