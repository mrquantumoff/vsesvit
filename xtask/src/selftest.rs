//! Owned by W0.5.
//! `cargo xtask selftest [--flavour gpui|winui] [--network]`: builds, runs `--self-test OUT_DIR` inside
//! a fresh test environment, prints `report.json` and fails unless the report lists every check of the
//! flavour, each passed (design section 6.2). W0.5 builds the `winui` branch; the `gpui` branch is
//! W1.8's and exits 4, "not available yet", until then.

use std::ffi::OsString;
use std::time::Duration;

use serde_json::Value;

use crate::testenv::{self, Run};
use crate::{Result, Status, workspace_root};

const USAGE: &str = "usage: cargo xtask selftest [--flavour gpui|winui] [--network]";
/// The longest WinUI check budget is a few minutes; `--network` downloads a real extension.
const TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// WinUI's check list, until C.0 moves it into core (design section 7.3).
const WINUI_REPORT_RS: &str = include_str!("../../crates/vsesvit-winui/src/report.rs");

#[derive(Debug, PartialEq, Eq)]
enum Flavour {
    Gpui,
    Winui,
}

#[derive(Debug, PartialEq, Eq)]
struct Options {
    flavour: Flavour,
    network: bool,
}

fn parse(args: &[String]) -> Result<Options> {
    let mut flavour = None;
    let mut network = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--network" if !network => network = true,
            "--flavour" if flavour.is_none() => {
                flavour = Some(match args.next().map(String::as_str) {
                    Some("gpui") => Flavour::Gpui,
                    Some("winui") => Flavour::Winui,
                    _ => return Err(USAGE.to_owned()),
                })
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    let flavour = flavour.unwrap_or(if cfg!(windows) { Flavour::Winui } else { Flavour::Gpui });
    Ok(Options { flavour, network })
}

pub fn run(args: &[String]) -> Result<Status> {
    let options = parse(args)?;
    match options.flavour {
        Flavour::Gpui => Ok(Status::NotAvailable),
        Flavour::Winui if !cfg!(windows) => Err("the WinUI self-test runs on Windows only".into()),
        Flavour::Winui => winui(options.network),
    }
}

fn winui(network: bool) -> Result<Status> {
    let expected = winui_checks(WINUI_REPORT_RS, network)?;
    let program = testenv::cargo_build(&["-p", "vsesvit"], "vsesvit")?;
    let out_dir = crate::probe::fresh(workspace_root().join("target").join("selftest").join("winui"))?;
    let mut args: Vec<OsString> = vec!["--self-test".into(), out_dir.clone().into_os_string()];
    if network {
        args.push("--network".into());
    }
    let code = testenv::run(&Run { program, args, out_dir: out_dir.clone(), timeout: TIMEOUT })?;
    let report = std::fs::read_to_string(out_dir.join("report.json")).ok();
    if let Some(report) = &report {
        println!("{report}");
    }
    let passed = validate(report.as_deref(), &expected, code)?;
    eprintln!("selftest: all {passed} WinUI checks passed; report in {}", out_dir.join("report.json").display());
    Ok(Status::Done)
}

/// WinUI's `CHECKS` array and, with `--network`, its `NETWORK_CHECK`, read from its source.
fn winui_checks(source: &'static str, network: bool) -> Result<Vec<&'static str>> {
    let unreadable = || "crates/vsesvit-winui/src/report.rs: no `CHECKS: [&str; N] = [...]`".to_owned();
    let (_, rest) = source.split_once("const CHECKS: [&str; ").ok_or_else(unreadable)?;
    let (len, rest) = rest.split_once("] = [").ok_or_else(unreadable)?;
    let (body, rest) = rest.split_once("];").ok_or_else(unreadable)?;
    let mut checks: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
    if len.trim().parse() != Ok(checks.len()) {
        return Err(format!("report.rs declares {len} checks and lists {}", checks.len()));
    }
    if network {
        let (_, rest) = rest.split_once("const NETWORK_CHECK: &str = \"").ok_or_else(unreadable)?;
        checks.push(rest.split('"').next().ok_or_else(unreadable)?);
    }
    Ok(checks)
}

/// Passes only a report that lists every expected check, each passed, from a run that exited 0.
/// Returns how many checks passed.
fn validate(report: Option<&str>, expected: &[&str], exit_code: i32) -> Result<usize> {
    let report = report.ok_or(format!("the self-test wrote no report.json (exit code {exit_code})"))?;
    let report: Value = serde_json::from_str(report).map_err(|e| format!("report.json is not JSON: {e}"))?;
    let checks = report["checks"].as_array().ok_or("report.json has no `checks` list")?;
    if checks.is_empty() {
        return Err("report.json lists zero checks".into());
    }
    let named = |name: &str| checks.iter().find(|c| c["name"] == name);
    let missing: Vec<&str> = expected.iter().copied().filter(|name| named(name).is_none()).collect();
    if !missing.is_empty() {
        return Err(format!("report.json lacks {} of {} checks: {}", missing.len(), expected.len(), missing.join(", ")));
    }
    let failed: Vec<&str> = checks.iter().filter(|c| c["ok"] != true).map(|c| c["name"].as_str().unwrap_or("?")).collect();
    if !failed.is_empty() {
        return Err(format!("{} of {} checks failed: {}", failed.len(), checks.len(), failed.join(", ")));
    }
    if report["ok"] != true || exit_code != 0 {
        return Err(format!("the run reports ok={} and exited with {exit_code}", report["ok"]));
    }
    Ok(checks.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    /// A report as `vsesvit_core::testkit::report` writes it.
    fn report(checks: &[(&str, bool)]) -> String {
        let checks: Vec<Value> = checks
            .iter()
            .map(|(name, ok)| serde_json::json!({ "name": name, "ok": ok, "ms": 3, "detail": "" }))
            .collect();
        let ok = !checks.is_empty() && checks.iter().all(|c| c["ok"] == true);
        serde_json::json!({ "platform": "windows", "ok": ok, "checks": checks }).to_string()
    }

    const EXPECTED: [&str; 3] = ["profile_open", "install_crx", "navigate"];

    #[test]
    fn gpui_is_not_available_yet() {
        assert!(matches!(run(&args(&["--flavour", "gpui"])), Ok(Status::NotAvailable)));
    }

    #[test]
    fn options() {
        let default = if cfg!(windows) { Flavour::Winui } else { Flavour::Gpui };
        assert_eq!(parse(&[]), Ok(Options { flavour: default, network: false }));
        assert_eq!(parse(&args(&["--network", "--flavour", "winui"])), Ok(Options { flavour: Flavour::Winui, network: true }));
        for bad in [&["--flavour"][..], &["--flavour", "gtk"], &["--network", "--network"], &["--fast"]] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn winuis_list_has_47_checks_and_one_more_with_network() {
        let checks = winui_checks(WINUI_REPORT_RS, false).unwrap();
        assert_eq!(checks.len(), 47);
        assert_eq!(checks[0], "profile_open");
        assert_eq!(checks.last(), Some(&"screenshot"));
        assert_eq!(winui_checks(WINUI_REPORT_RS, true).unwrap().last(), Some(&"cws_install"));
    }

    #[test]
    fn a_list_whose_length_disagrees_is_refused() {
        assert!(winui_checks(r#"const CHECKS: [&str; 2] = ["a"];"#, false).unwrap_err().contains("declares 2"));
        assert!(winui_checks("no list here", false).is_err());
    }

    #[test]
    fn a_complete_passing_report_passes() {
        let all: Vec<(&str, bool)> = EXPECTED.iter().map(|n| (*n, true)).collect();
        assert_eq!(validate(Some(&report(&all)), &EXPECTED, 0), Ok(3));
    }

    #[test]
    fn a_missing_report_fails() {
        assert!(validate(None, &EXPECTED, 3).unwrap_err().contains("no report.json (exit code 3)"));
    }

    #[test]
    fn a_report_with_zero_checks_fails() {
        assert!(validate(Some(&report(&[])), &EXPECTED, 0).unwrap_err().contains("zero checks"));
        assert!(validate(Some(r#"{"ok":true}"#), &EXPECTED, 0).is_err());
    }

    #[test]
    fn a_report_missing_a_check_fails() {
        let partial = report(&[("profile_open", true), ("navigate", true)]);
        assert!(validate(Some(&partial), &EXPECTED, 0).unwrap_err().contains("lacks 1 of 3 checks: install_crx"));
    }

    #[test]
    fn a_failed_check_fails() {
        let failed = report(&[("profile_open", true), ("install_crx", false), ("navigate", true)]);
        assert!(validate(Some(&failed), &EXPECTED, 1).unwrap_err().contains("1 of 3 checks failed: install_crx"));
    }

    #[test]
    fn a_passing_report_from_a_failed_exit_fails() {
        let all: Vec<(&str, bool)> = EXPECTED.iter().map(|n| (*n, true)).collect();
        assert!(validate(Some(&report(&all)), &EXPECTED, 70).unwrap_err().contains("exited with 70"));
    }

    /// The first lines of a real run's report.json (W0.5's private-desktop run, 2026-10-24), cut
    /// to two checks.
    #[test]
    fn a_recorded_report_parses() {
        let recorded = r#"{
  "platform": "windows",
  "ok": false,
  "checks": [
    { "name": "profile_open", "ok": true, "ms": 0, "detail": "C:\\out\\profile" },
    { "name": "screenshot", "ok": false, "ms": 565, "detail": "capture: The parameter is incorrect. (0x80070057)" }
  ]
}"#;
        let err = validate(Some(recorded), &["profile_open", "screenshot"], 1).unwrap_err();
        assert_eq!(err, "1 of 2 checks failed: screenshot");
    }
}
