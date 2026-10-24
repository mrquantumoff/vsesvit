//! Owned by W3.7.

use vsesvit_update::Downloaded;

use crate::app::quit::{RelaunchArgs, Spawner};

/// "Restart to update" (design section 3.6 step 10): installs `downloaded`, then does what its
/// `Installed` result says, relaunching through `spawner` where that is a relaunch. Until W3.7 lands
/// nothing downloads an update, so nothing reaches this.
pub fn install_and_relaunch(downloaded: Downloaded, relaunch: &RelaunchArgs, spawner: &dyn Spawner) {
    let _ = (relaunch, spawner);
    log::error!("an update reached the exit before the updater was built; it stays downloaded: {downloaded:?}");
}

/// Windows, quitting with an update ready (design section 3.6 step 10). Until W3.7 lands nothing
/// downloads an update, so nothing reaches this.
pub fn install_on_exit(downloaded: Downloaded) {
    log::error!("an update reached the exit before the updater was built; it stays downloaded: {downloaded:?}");
}
