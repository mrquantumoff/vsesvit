//! Whether an extension runs in private windows.
//!
//! Chrome's "Allow in Incognito": off for every extension until the user turns it on, since an
//! extension could keep what it sees there. One synced preference, [`ALLOWED_IN_PRIVATE`], lists
//! the extensions the user allowed. Ids of extensions this device does not have are kept, as in
//! [`super::toolbar::TOOLBAR`]: another device may have them, and the list syncs as one value.

use std::collections::BTreeSet;

use super::{ExtensionId, Extensions};
use crate::Error;
use crate::prefs::{Pref, Scope};

pub const ALLOWED_IN_PRIVATE: Pref<BTreeSet<ExtensionId>> = Pref { key: "extensions.allowed_in_private", scope: Scope::Synced, default: BTreeSet::new };

impl Extensions<'_> {
    /// Off until the user turns it on.
    pub fn allowed_in_private(&mut self, id: &ExtensionId) -> bool {
        self.p.prefs().get(&ALLOWED_IN_PRIVATE).contains(id)
    }

    /// Writes nothing when the switch is already so.
    pub fn set_allowed_in_private(&mut self, id: &ExtensionId, allowed: bool) -> Result<(), Error> {
        let mut ids = self.p.prefs().get(&ALLOWED_IN_PRIVATE);
        let changed = if allowed { ids.insert(id.clone()) } else { ids.remove(id) };
        if changed { self.p.prefs().set(&ALLOWED_IN_PRIVATE, &ids) } else { Ok(()) }
    }
}
