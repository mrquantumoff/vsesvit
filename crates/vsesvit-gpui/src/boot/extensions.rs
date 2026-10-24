//! Owned by W1.4.
//! Boot step 7 (design section 3.1): extensions, before the engine starts, thread-free and metadata
//! only. Chromium reads its External Extensions folder only at start (section 5.2), so the files must
//! be right before step 9. This pass reconciles core's desired set with what this device has, which
//! is idempotent, so a crash between an install's commit and the restart loses nothing.
//!
//! Still to come, each through its own unit: writing and removing the External Extensions files under
//! the publish rule of section 5.5 (W4.2, reached through the engine contract WX.1 freezes, since this
//! crate never names the CEF glue), seeding Chromium's storage for an id published for the first time
//! (W4.5), and marking records without an archive "needs restoring" (W4.5), which step 10a hands to
//! `extensions::restore`.

use vsesvit_core::Profile;
use vsesvit_core::extensions::ExtensionId;

/// What step 7 leaves for after the first window shows (step 10a).
#[derive(Debug, Default)]
pub struct AtStart {
    pub needs_restoring: Vec<ExtensionId>,
}

pub fn metadata_pass(profile: &mut Profile) -> AtStart {
    match profile.extensions().reconcile() {
        // The install jobs need the network; the shell's own reconcile after start runs them.
        Ok(reconciled) => log::info!(
            "extensions: {} to install after start, {} removed by another device",
            reconciled.install.len(),
            reconciled.removed.len()
        ),
        Err(e) => log::warn!("extensions: reconcile failed, the start goes on with the current set: {e}"),
    }
    AtStart::default()
}
