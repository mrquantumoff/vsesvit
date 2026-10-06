//! The bounded stack behind "reopen closed tab" and tab search's recently closed tabs.

use std::collections::VecDeque;

/// Names one closed tab for as long as the stack keeps it, whatever is closed or reopened
/// meanwhile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClosedKey(u64);

pub(crate) struct ClosedTabs<T> {
    items: VecDeque<(ClosedKey, T)>,
    capacity: usize,
    next_key: u64,
}

impl<T> ClosedTabs<T> {
    pub(crate) fn new(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "a closed-tab stack must hold at least one tab"
        );
        ClosedTabs {
            items: VecDeque::with_capacity(capacity),
            capacity,
            next_key: 0,
        }
    }

    /// Remembers `item`, forgetting the oldest one when full.
    pub(crate) fn push(&mut self, item: T) -> ClosedKey {
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        let key = ClosedKey(self.next_key);
        self.next_key += 1;
        self.items.push_back((key, item));
        key
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The most recently closed tab.
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.items.pop_back().map(|(_, item)| item)
    }

    /// The tab `key` names, if the stack still has it.
    pub(crate) fn take(&mut self, key: ClosedKey) -> Option<T> {
        let at = self.items.iter().position(|(k, _)| *k == key)?;
        self.items.remove(at).map(|(_, item)| item)
    }

    pub(crate) fn get(&self, key: ClosedKey) -> Option<&T> {
        self.items.iter().find(|(k, _)| *k == key).map(|(_, item)| item)
    }

    /// Oldest first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (ClosedKey, &T)> {
        self.items.iter().map(|(key, item)| (*key, item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_most_recent_first() {
        let mut stack = ClosedTabs::new(3);
        stack.push(1);
        stack.push(2);
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn forgets_the_oldest_when_full() {
        let mut stack = ClosedTabs::new(2);
        for i in 1..=3 {
            stack.push(i);
        }
        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn a_key_keeps_naming_its_tab_while_others_come_and_go() {
        let mut stack = ClosedTabs::new(3);
        let first = stack.push("a");
        let second = stack.push("b");
        stack.push("c");
        assert_eq!(stack.take(second), Some("b"));
        stack.push("d");
        assert_eq!(stack.get(first), Some(&"a"));
        let last = stack.push("e");
        assert_eq!(stack.get(first), None, "the oldest went when the stack filled");
        assert_eq!(stack.take(second), None, "a key is never reused");
        let listed: Vec<&str> = stack.iter().map(|(_, item)| *item).collect();
        assert_eq!(listed, ["c", "d", "e"]);
        assert_eq!(stack.take(last), Some("e"));
        assert_eq!(stack.pop(), Some("d"));
    }
}
