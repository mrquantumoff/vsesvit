//! `--check-for-updates` and `--update`, the same in both shells: the updater without a window,
//! for scripts. Each prints one JSON line on stdout, or `{"error": ...}` and exits 1:
//!
//! ```json
//! {"installation": "appimage", "current": "1.0.0",
//!  "available": {"version": "1.1.0", "notes": "...", "pub_date": "2026-09-01T00:00:00Z"},
//!  "installed": {"version": "1.1.0", "next": "exit_now" | "relaunch" | "next_launch"}}
//! ```
//!
//! `available` is null when this is the newest version; only `--update` adds `installed`, null
//! when there was nothing to install. The shell passes the channel: these commands open no
//! profile, so they follow the channel the build was released on.

use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use semver::Version;
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use crate::{Available, Config, Error, Installation, Installed, Release, Updater, remove_stale_downloads};

/// `--check-for-updates`.
pub fn check(installation: &Installation, current: &Version, channel: &str) -> Result<Value, Error> {
    let available = Updater::new(Config::builtin()?, current.clone(), installation.clone())?.check(channel)?;
    Ok(report(installation, current, available.as_ref().map(Available::release)))
}

/// `--update`: checks, downloads into `dir` (progress on stderr) and installs.
pub fn update(installation: &Installation, current: &Version, channel: &str, dir: &Path) -> Result<Value, Error> {
    let available = Updater::new(Config::builtin()?, current.clone(), installation.clone())?.check(channel)?;
    let mut report = report(installation, current, available.as_ref().map(Available::release));
    report["installed"] = Value::Null;
    let Some(available) = available else {
        return Ok(report);
    };
    let update = available.into_update()?;
    fs::create_dir_all(dir)?;
    remove_stale_downloads(dir, Some(&update.release.version))?;
    let downloaded = update.download(dir, progress(update.release.version.clone()))?;
    let next = downloaded.install(installation, &[])?;
    report["installed"] = installed(&update.release.version, next);
    Ok(report)
}

/// Prints `result` as one JSON line: success with the report, or failure with `{"error": ...}`.
pub fn print(result: Result<Value, impl Display>) -> ExitCode {
    match result {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", json!({"error": e.to_string()}));
            ExitCode::FAILURE
        }
    }
}

fn report(installation: &Installation, current: &Version, available: Option<&Release>) -> Value {
    json!({
        "installation": installation.variant().unwrap_or("unpackaged"),
        "current": current.to_string(),
        "available": available.map(|release| json!({
            "version": release.version.to_string(),
            "notes": release.notes,
            "pub_date": release.pub_date.and_then(|date| date.format(&Rfc3339).ok()),
        })),
    })
}

fn installed(version: &Version, next: Installed) -> Value {
    let next = match next {
        Installed::ExitNow => "exit_now",
        Installed::Relaunch => "relaunch",
        Installed::NextLaunch => "next_launch",
    };
    json!({"version": version.to_string(), "next": next})
}

/// Prints a line each time another tenth of the file (or, without a length, another MiB)
/// arrives.
fn progress(version: Version) -> impl FnMut(u64, Option<u64>) {
    let mut shown = None;
    move |received, total| {
        let step = match total {
            Some(total) if total > 0 => received * 10 / total,
            _ => received >> 20,
        };
        if shown.replace(step) == Some(step) {
            return;
        }
        match total {
            Some(total) if total > 0 => eprintln!("vsesvit: downloading {version}: {}% of {total} bytes", received * 100 / total),
            _ => eprintln!("vsesvit: downloading {version}: {received} bytes"),
        }
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn reports_name_the_installation_and_both_versions() {
        assert_eq!(report(&Installation::Unpackaged, &v("1.0.0"), None), json!({"installation": "unpackaged", "current": "1.0.0", "available": null}));
        let release = Release { version: v("9.0.0"), notes: Some("Faster".to_owned()), pub_date: Some(OffsetDateTime::parse("2026-09-01T12:00:00Z", &Rfc3339).unwrap()) };
        let appimage = Installation::AppImage { image: "/opt/Vsesvit.AppImage".into() };
        let mut updated = report(&appimage, &v("1.0.0"), Some(&release));
        updated["installed"] = installed(&release.version, Installed::NextLaunch);
        assert_eq!(
            updated,
            json!({
                "installation": "appimage",
                "current": "1.0.0",
                "available": {"version": "9.0.0", "notes": "Faster", "pub_date": "2026-09-01T12:00:00Z"},
                "installed": {"version": "9.0.0", "next": "next_launch"},
            })
        );
        assert_eq!(installed(&release.version, Installed::ExitNow)["next"], "exit_now");
        assert_eq!(installed(&release.version, Installed::Relaunch)["next"], "relaunch");
    }
}
