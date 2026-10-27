//! Owned by W1.8 (from W0.5).
//! `cargo xtask selftest [--flavour gpui|winui] [--network] [--release] [--all-features]`: builds, runs
//! `--self-test OUT_DIR` inside a fresh test environment, prints `report.json` and fails unless the
//! report lists every check of the flavour, each passed (design section 6.2). OUT_DIR is emptied first,
//! so no run restores another's session. The GPUI flavour builds the GPUI browser (with its helper on
//! Linux); `--release` builds it optimized, and `--all-features` runs the binary of
//! `cargo build --workspace --all-features` instead (design section 1.4).

use std::ffi::OsString;
use std::time::Duration;

use serde_json::Value;

use crate::testenv::{self, Run};
use crate::{Result, Status, workspace_root};

/// The check registry the shells report against, compiled in from vsesvit-core rather than
/// linking all of it.
#[path = "../../crates/vsesvit-core/src/testkit/checks.rs"]
mod checks;

use checks::Flavour;

const USAGE: &str = "usage: cargo xtask selftest [--flavour gpui|winui] [--network] [--release] [--all-features]";
/// The longest WinUI check budget is a few minutes; `--network` downloads a real extension.
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, PartialEq, Eq)]
struct Options {
    flavour: Flavour,
    network: bool,
    build: Build,
}

/// Which GPUI binary runs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Build {
    Debug,
    Release,
    /// `cargo build --workspace --all-features`: every shell and every test feature linked in.
    AllFeatures,
}

fn parse(args: &[String]) -> Result<Options> {
    let mut flavour = None;
    let mut network = false;
    let mut build = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--network" if !network => network = true,
            "--release" if build.is_none() => build = Some(Build::Release),
            "--all-features" if build.is_none() => build = Some(Build::AllFeatures),
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
    let build = build.unwrap_or(Build::Debug);
    if flavour == Flavour::Winui && build != Build::Debug {
        return Err(format!("--release and --all-features are for the GPUI flavour
{USAGE}"));
    }
    Ok(Options { flavour, network, build })
}

pub fn run(args: &[String]) -> Result<Status> {
    let options = parse(args)?;
    match options.flavour {
        Flavour::Gpui => gpui(options.network, options.build),
        Flavour::Winui if !cfg!(windows) => Err("the WinUI self-test runs on Windows only".into()),
        Flavour::Winui => winui(options.network),
    }
}

fn gpui(network: bool, build: Build) -> Result<Status> {
    let program = match build {
        Build::Debug => testenv::cargo_build(&["-p", "vsesvit", "-p", "vsesvit-helper", "--no-default-features", "--features", "gpui"], "vsesvit")?,
        Build::Release => testenv::cargo_build(
            &["-p", "vsesvit", "-p", "vsesvit-helper", "--no-default-features", "--features", "gpui", "--release"],
            "vsesvit",
        )?,
        Build::AllFeatures => testenv::cargo_build(&["--workspace", "--all-features"], "vsesvit")?,
    };
    // cef-dll-sys puts libcef beside the binary; Windows finds DLLs on PATH.
    if let Some(bins) = program.parent() {
        crate::probe::dlls_from(bins)?;
    }
    let dir = match build {
        Build::Debug => "gpui",
        Build::Release => "gpui-release",
        Build::AllFeatures => "gpui-all-features",
    };
    let out_dir = crate::probe::fresh(workspace_root().join("target").join("selftest").join(dir))?;
    selftest(Flavour::Gpui, program, out_dir, network)
}

fn winui(network: bool) -> Result<Status> {
    let program = testenv::cargo_build(&["-p", "vsesvit"], "vsesvit")?;
    let out_dir = crate::probe::fresh(workspace_root().join("target").join("selftest").join("winui"))?;
    selftest(Flavour::Winui, program, out_dir, network)
}

/// Runs `program --self-test OUT_DIR` in a fresh test environment and checks its report.
fn selftest(flavour: Flavour, program: std::path::PathBuf, out_dir: std::path::PathBuf, network: bool) -> Result<Status> {
    let expected = checks::expected(flavour, network);
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
    eprintln!("selftest: all {passed} {flavour:?} checks passed; report in {}", out_dir.join("report.json").display());
    Ok(Status::Done)
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
    fn options() {
        let default = if cfg!(windows) { Flavour::Winui } else { Flavour::Gpui };
        assert_eq!(parse(&[]), Ok(Options { flavour: default, network: false, build: Build::Debug }));
        assert_eq!(
            parse(&args(&["--network", "--flavour", "winui"])),
            Ok(Options { flavour: Flavour::Winui, network: true, build: Build::Debug })
        );
        assert_eq!(
            parse(&args(&["--flavour", "gpui", "--release"])),
            Ok(Options { flavour: Flavour::Gpui, network: false, build: Build::Release })
        );
        assert_eq!(
            parse(&args(&["--all-features", "--flavour", "gpui"])),
            Ok(Options { flavour: Flavour::Gpui, network: false, build: Build::AllFeatures })
        );
        for bad in [
            &["--flavour"][..],
            &["--flavour", "gtk"],
            &["--network", "--network"],
            &["--fast"],
            &["--release", "--all-features"],
            &["--flavour", "winui", "--release"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
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

    /// A real run's report.json (W0.5's private-desktop run, 2026-10-24), cut to two checks with
    /// shortened details.
    #[test]
    fn a_recorded_report_parses() {
        let recorded = r#"{
  "platform": "windows",
  "ok": false,
  "checks": [
    { "name": "profile_open", "ok": true, "ms": 23, "detail": "opened C:\\out\\profile" },
    { "name": "screenshot", "ok": false, "ms": 564, "detail": "capture: The parameter is incorrect. (0x80070057)" }
  ]
}"#;
        let err = validate(Some(recorded), &["profile_open", "screenshot"], 1).unwrap_err();
        assert_eq!(err, "1 of 2 checks failed: screenshot");
    }
}
