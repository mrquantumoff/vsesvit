//! Owned by W6.4.
//! `cargo xtask stamp-exe EXE --icon ICO --version VERSION`: writes our icon and version resources into
//! the renamed `bootstrap.exe` with `UpdateResourceW` (design section 6.2, Q15). Not built yet.

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
