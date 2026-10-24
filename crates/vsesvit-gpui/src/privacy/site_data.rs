//! Owned by W3.1.

use gpui::{App, Task};

/// The start-up clear (design section 3.7): runs on the maintenance browser before session restore's
/// first load, capped at 5 s by the caller. Until W3.1 lands there is nothing to clear.
pub fn clear_at_start(_cx: &mut App) -> Task<()> {
    Task::ready(())
}

/// Clear on exit (design section 3.6 step 4): the user's browsers are closed and the maintenance
/// browser still lives. Capped at 3 s by the caller. Until W3.1 lands there is nothing to clear.
pub fn clear_on_exit(_cx: &mut App) -> Task<()> {
    Task::ready(())
}
