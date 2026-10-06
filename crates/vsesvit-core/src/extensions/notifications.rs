//! Whether an extension may show notifications.
//!
//! Chrome keeps one switch per extension (`NotifierStateTracker`) as the list of extensions
//! the user turned off. So does one synced preference here, [`NOTIFICATIONS_OFF`]. Ids of
//! extensions this device does not have are kept, as in [`super::toolbar::TOOLBAR`]:
//! another device may have them, and the list syncs as one value.

use std::collections::BTreeSet;

use super::{ExtensionId, Extensions};
use crate::Error;
use crate::prefs::{Pref, Scope};

pub const NOTIFICATIONS_OFF: Pref<BTreeSet<ExtensionId>> = Pref { key: "extensions.notifications_off", scope: Scope::Synced, default: BTreeSet::new };

impl Extensions<'_> {
    /// On until the user turns it off.
    pub fn notifications_allowed(&mut self, id: &ExtensionId) -> bool {
        !self.p.prefs().get(&NOTIFICATIONS_OFF).contains(id)
    }

    /// Writes nothing when the switch is already so.
    pub fn set_notifications_allowed(&mut self, id: &ExtensionId, allowed: bool) -> Result<(), Error> {
        let mut off = self.p.prefs().get(&NOTIFICATIONS_OFF);
        let changed = if allowed { off.remove(id) } else { off.insert(id.clone()) };
        if changed { self.p.prefs().set(&NOTIFICATIONS_OFF, &off) } else { Ok(()) }
    }
}
