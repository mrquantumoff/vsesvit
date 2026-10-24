//! Owned by W0.5.
//! `cargo xtask selftest [--flavour gpui|winui] [--network]`: builds, runs `--self-test OUT_DIR` inside
//! a fresh test environment and fails unless the report lists every check of the flavour (design
//! section 6.2). W0.5 builds the `winui` branch and W1.8 the `gpui` branch; until then both are not
//! available.

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
