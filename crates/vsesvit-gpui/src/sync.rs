//! Owned by W3.6.

use gpui::{App, Task};

/// The final sync round at shutdown (design section 3.6 step 4), capped at 3 s by the caller. Until
/// W3.6 lands there is no sync.
pub fn final_round(_cx: &mut App) -> Task<()> {
    Task::ready(())
}
