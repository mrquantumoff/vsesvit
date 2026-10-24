//! Owned by W1.1.

pub mod default_browser;
#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod files;
pub mod shell_open;
#[cfg(windows)]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod x11;
