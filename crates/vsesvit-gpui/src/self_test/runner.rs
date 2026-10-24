//! Owned by W1.8.

use std::path::PathBuf;

use gpui::App;

use crate::app::quit::{self, ExitIntent};

/// `vsesvit --self-test OUT_DIR [--network] [--phase NAME]`, once boot's step 10 has the first window
/// and the maintenance browser (design section 7.2). Until W1.8 lands the run ends at once with 4,
/// "not available yet", which no gate accepts.
pub fn start(out_dir: PathBuf, network: bool, phase: Option<String>, cx: &mut App) {
    log::error!("the self-test runner is not built yet (OUT_DIR {}, network {network}, phase {phase:?})", out_dir.display());
    quit::request_with_code(ExitIntent::Quit, 4, cx);
}
