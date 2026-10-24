//! Owned by W1.3.
//! `cargo xtask bundle-sandbox`: lays out the sandboxed Windows image (renamed bootstrap, `vsesvit.dll`,
//! the CEF runtime) in `target/gpui/bundle` (design section 6.2). Not built yet.

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
