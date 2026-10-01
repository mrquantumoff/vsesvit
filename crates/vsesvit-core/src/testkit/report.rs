//! The self-test's `report.json` (docs/design/self-test.md), the same for both shells.
//! Each shell lists the checks it runs; a run that stops before one of them fails.

use std::path::Path;

use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub ms: u128,
    pub detail: String,
}

#[derive(Debug)]
pub struct Report {
    platform: &'static str,
    expected: Vec<&'static str>,
    checks: Vec<Check>,
}

impl Report {
    /// `platform` is `"windows"` or `"linux"`; `expected` names every check the run must
    /// pass, in the order it runs them.
    pub fn new(platform: &'static str, expected: Vec<&'static str>) -> Self {
        Report { platform, expected, checks: Vec::new() }
    }

    pub fn push(&mut self, check: Check) {
        self.checks.push(check);
    }

    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// Adds a failed entry for each expected check that did not run, so a run that stopped
    /// early can never look complete.
    pub fn complete(&mut self, reason: &str) {
        for &name in &self.expected {
            if !self.checks.iter().any(|c| c.name == name) {
                self.checks.push(Check { name, ok: false, ms: 0, detail: format!("not run: {reason}") });
            }
        }
    }

    /// Every expected check ran and passed, and nothing else failed.
    pub fn ok(&self) -> bool {
        let passed = |name: &&str| self.checks.iter().any(|c| c.name == *name && c.ok);
        self.expected.iter().all(passed) && self.checks.iter().all(|c| c.ok)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "platform": self.platform,
            "ok": self.ok(),
            "checks": self.checks.iter().map(|c| json!({
                "name": c.name,
                "ok": c.ok,
                "ms": u64::try_from(c.ms).unwrap_or(u64::MAX),
                "detail": c.detail,
            })).collect::<Vec<_>>(),
        })
    }

    /// Writes `<out_dir>/report.json`.
    pub fn write(&self, out_dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(&self.to_json()).map_err(std::io::Error::other)?;
        std::fs::write(out_dir.join("report.json"), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED: [&str; 3] = ["profile_open", "install_crx", "navigate"];

    fn passed(name: &'static str) -> Check {
        Check { name, ok: true, ms: 1, detail: String::new() }
    }

    #[test]
    fn ok_needs_every_expected_check_to_pass() {
        let mut report = Report::new("linux", EXPECTED.to_vec());
        report.push(passed("profile_open"));
        assert!(!report.ok(), "a run that stopped early is not a pass");
        report.push(passed("install_crx"));
        report.push(passed("navigate"));
        assert!(report.ok());
        report.push(Check { ok: false, ..passed("extra") });
        assert!(!report.ok());
    }

    #[test]
    fn missing_checks_are_reported_as_failures() {
        let mut report = Report::new("windows", EXPECTED.to_vec());
        report.push(passed("profile_open"));
        report.complete("the self-test stopped");
        assert_eq!(report.checks().len(), EXPECTED.len());
        assert!(report.checks()[1..].iter().all(|c| !c.ok && c.detail == "not run: the self-test stopped"));
        let json = report.to_json();
        assert_eq!(json["platform"], "windows");
        assert_eq!(json["ok"], false);
        assert_eq!(json["checks"][1]["name"], "install_crx");
        assert_eq!(json["checks"][0]["ms"], 1);
    }
}
