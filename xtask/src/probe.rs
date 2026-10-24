//! Owned by W0.5.
//! `cargo xtask probe EXAMPLE [ARGS]...`: builds one example of `vsesvit-testenv`, `vsesvit-gpui` or
//! `vsesvit-cef` and runs it inside a fresh test environment (design section 6.2). Not built yet.

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
