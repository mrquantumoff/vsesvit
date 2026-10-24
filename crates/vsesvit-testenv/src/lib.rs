//! Owned by W0.5.
//! The test-environment guard (design section 7.3). Every self-test, example and window-opening test
//! binary calls `require()` before it opens a window; it exits 3 unless the process runs on the private
//! display or desktop that `xtask`'s launcher created. Until W0.5 builds the check, it refuses every run.

use std::process;

/// A validated test environment. Only `require` builds one.
pub struct TestEnv {
    _private: (),
}

/// Exits 3 unless this process runs in a launcher-made test environment. Not built yet: refuses
/// every run, so nothing opens a window.
pub fn require() -> TestEnv {
    eprintln!("vsesvit-testenv: the test-environment guard is not built yet; refusing to open a window");
    process::exit(3)
}
