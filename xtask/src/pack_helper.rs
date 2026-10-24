//! Owned by W4.2.
//! `cargo xtask pack-helper`: packs `crates/vsesvit-cef/ext/helper/` into `helper-extension.crx` with the
//! committed key (design section 5.3). Not built yet.

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
