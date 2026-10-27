//! Owned by W1.8.
//! The GPUI shell's `report.json` (docs/design/self-test.md): core's report over the GPUI flavour's
//! checks so far (`vsesvit_core::testkit::checks`), one log line per check.

use std::path::Path;

use vsesvit_core::testkit::checks::{Flavour, expected};
use vsesvit_core::testkit::report::{Check, Report};

pub const PLATFORM: &str = if cfg!(windows) { "windows" } else { "linux" };

/// An empty report expecting every check this shell runs.
pub fn new(network: bool) -> Report {
    Report::new(PLATFORM, expected(Flavour::Gpui, network))
}

/// Adds `check` and logs it as WinUI's run does.
pub fn push(report: &mut Report, check: Check) {
    log::info!("self-test {}: ok={} ({} ms) {}", check.name, check.ok, check.ms, check.detail);
    report.push(check);
}

/// Completes `report` (a run that stopped early lists what it never ran as failed), writes it and
/// returns the exit code: 0 only if every expected check ran and passed.
pub fn finish(mut report: Report, stopped: Option<&str>, out_dir: &Path) -> u8 {
    if let Some(reason) = stopped {
        log::error!("self-test stopped: {reason}");
        report.complete(reason);
    }
    report.complete("the run never reached it");
    let ok = report.ok();
    let failed: Vec<&str> = report.checks().iter().filter(|c| !c.ok).map(|c| c.name).collect();
    match report.write(out_dir) {
        Ok(()) => log::info!("self-test: ok={ok}, {} checks, failed {failed:?}; report in {}", report.checks().len(), out_dir.display()),
        Err(e) => log::error!("self-test: writing report.json in {}: {e}", out_dir.display()),
    }
    u8::from(!ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_stopped_reports_what_it_never_ran() {
        let dir = std::env::temp_dir().join(format!("vsesvit-gpui-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut report = new(false);
        push(&mut report, Check { name: "profile_open", ok: true, ms: 1, detail: String::new() });
        assert_eq!(finish(report, Some("no window"), &dir), 1);
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
        let checks = written["checks"].as_array().unwrap();
        assert_eq!(checks.len(), expected(Flavour::Gpui, false).len());
        assert_eq!(checks[1]["detail"], "not run: no window");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
