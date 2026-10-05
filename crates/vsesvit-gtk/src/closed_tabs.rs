//! The bounded stack behind "reopen closed tab".

use std::collections::VecDeque;

pub(crate) struct ClosedTabs<T> {
    items: VecDeque<T>,
    capacity: usize,
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
        }
    }

    /// Remembers `item`, forgetting the oldest one when full.
    pub(crate) fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(item);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The most recently closed tab.
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.items.pop_back()
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
}
