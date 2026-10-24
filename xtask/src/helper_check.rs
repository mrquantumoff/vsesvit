//! Owned by W1.3.
//! `cargo xtask helper-check`: `readelf -d` on the built `vsesvit-helper`; fails if `NEEDED` lists anything
//! beyond libcef, libc, libm and libgcc_s (design sections 1.1 and 6.2). Not built yet.

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
