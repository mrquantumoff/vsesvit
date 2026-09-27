//! The self-test's `report.json` (docs/design/self-test.md).

use std::path::Path;

use serde_json::{Value, json};

/// The checks, in the order the contract lists and runs them.
pub(crate) const CHECKS: [&str; 14] = [
    "profile_open",
    "install_crx",
    "engine_loaded_extension",
    "navigate",
    "history_recorded",
    "content_script",
    "dnr_blocked",
    "bookmark",
    "tabs",
    "tab_layout",
    "popup",
    "omnibox",
    "session",
    "screenshot",
];

/// Only with `--network`.
pub(crate) const NETWORK_CHECK: &str = "cws_install";

pub(crate) fn expected(network: bool) -> Vec<&'static str> {
    let mut names = CHECKS.to_vec();
    if network {
        names.push(NETWORK_CHECK);
    }
    names
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub ms: u128,
    pub detail: String,
}

#[derive(Debug, Default)]
pub(crate) struct Report {
    checks: Vec<Check>,
}

impl Report {
    pub fn push(&mut self, check: Check) {
        self.checks.push(check);
    }

    #[cfg(test)]
    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// Adds a failed entry for each expected check that did not run, so a run that stopped
    /// early can never look complete.
    pub fn complete(&mut self, network: bool, reason: &str) {
        for name in expected(network) {
            if !self.checks.iter().any(|c| c.name == name) {
                self.checks.push(Check {
                    name,
                    ok: false,
                    ms: 0,
                    detail: format!("not run: {reason}"),
                });
            }
        }
    }

    /// Every expected check ran and passed.
    pub fn ok(&self, network: bool) -> bool {
        expected(network)
            .iter()
            .all(|name| self.checks.iter().any(|c| c.name == *name && c.ok))
            && self.checks.iter().all(|c| c.ok)
    }

    pub fn to_json(&self, network: bool) -> Value {
        json!({
            "platform": "windows",
            "ok": self.ok(network),
            "checks": self.checks.iter().map(|c| json!({
                "name": c.name,
                "ok": c.ok,
                "ms": c.ms,
                "detail": c.detail,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn write(&self, out_dir: &Path, network: bool) -> std::io::Result<()> {
        let text =
            serde_json::to_string_pretty(&self.to_json(network)).map_err(std::io::Error::other)?;
        std::fs::write(out_dir.join("report.json"), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passed(name: &'static str) -> Check {
        Check {
            name,
            ok: true,
            ms: 1,
            detail: String::new(),
        }
    }

    #[test]
    fn ok_needs_every_expected_check_to_pass() {
        let mut report = Report::default();
        for name in CHECKS {
            report.push(passed(name));
        }
        assert!(report.ok(false));
        assert!(!report.ok(true), "cws_install is expected with --network");
        report.push(passed(NETWORK_CHECK));
        assert!(report.ok(true));
        report.push(Check {
            ok: false,
            ..passed("extra")
        });
        assert!(!report.ok(true));
    }

    #[test]
    fn missing_checks_are_reported_as_failures() {
        let mut report = Report::default();
        report.push(Check {
            ok: false,
            ..passed("profile_open")
        });
        report.complete(true, "the profile did not open");
        assert_eq!(report.checks().len(), CHECKS.len() + 1);
        assert!(
            report.checks()[1..]
                .iter()
                .all(|c| !c.ok && c.detail.starts_with("not run"))
        );
        let json = report.to_json(true);
        assert_eq!(json["platform"], "windows");
        assert_eq!(json["ok"], false);
        assert_eq!(json["checks"][1]["name"], "install_crx");
    }
}
