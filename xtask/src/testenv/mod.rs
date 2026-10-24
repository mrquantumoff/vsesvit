//! Owned by W0.5.
//! The launcher half of the test environment (design section 7.3): a private Xvfb display on Linux, a
//! private desktop on Windows. `selftest` and `probe` use it; it has no command of its own. Not built yet.

mod linux;
mod windows;
