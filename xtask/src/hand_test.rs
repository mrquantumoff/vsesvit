//! Owned by W0.5.
//! `cargo xtask hand-test`: starts the real browser with a scratch profile on the user's own desktop
//! for hand-tests.md, with no scripted input (design section 7.4). Not built yet.

use crate::{Result, Status};

pub fn run(_args: &[String]) -> Result<Status> {
    Ok(Status::NotAvailable)
}

#[cfg(test)]
mod tests {
    #[test]
    fn not_available_yet() {
        assert!(matches!(super::run(&[]), Ok(crate::Status::NotAvailable)));
    }
}
