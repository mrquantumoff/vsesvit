//! `--check-for-updates` and `--update`: the updater without a window or a display, for scripts
//! and the packaging end-to-end test. Each prints one JSON line on stdout, or `{"error": ...}`
//! and exits 1. Neither reads `updates.automatic`.

use std::fs;
use std::process::ExitCode;

use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use vsesvit_update::{
    Available, Config, Error, Installation, Installed, Release, Update, Updater,
    remove_stale_downloads,
};

use super::{current_version, download_dir};

pub(crate) fn check() -> ExitCode {
    let installation = Installation::detect();
    finish(
        updater(&installation)
            .and_then(|updater| updater.check())
            .map(|available| {
                let release = available.as_ref().map(Available::release);
                report(&installation, "available", release.map(release_json))
            }),
    )
}

/// Check, download (progress on stderr) and install.
pub(crate) fn update() -> ExitCode {
    let installation = Installation::detect();
    let result = (|| {
        let Some(available) = updater(&installation)?.check()? else {
            return Ok(report(&installation, "installed", None));
        };
        let update = available.into_update()?;
        let dir = download_dir();
        fs::create_dir_all(&dir)?;
        remove_stale_downloads(&dir, Some(&update.release.version))?;
        let downloaded = update.download(&dir, progress(&update))?;
        let next = downloaded.install(&installation, &[])?;
        let installed =
            json!({"version": update.release.version.to_string(), "next": outcome(next)});
        Ok(report(&installation, "installed", Some(installed)))
    })();
    finish(result)
}

fn updater(installation: &Installation) -> Result<Updater, Error> {
    Updater::new(Config::builtin()?, current_version(), installation.clone())
}

fn report(installation: &Installation, key: &str, value: Option<Value>) -> Value {
    json!({
        "installation": installation.variant().unwrap_or("unpackaged"),
        "current": current_version().to_string(),
        key: value,
    })
}

fn release_json(release: &Release) -> Value {
    json!({
        "version": release.version.to_string(),
        "notes": release.notes,
        "pub_date": release.pub_date.and_then(|date| date.format(&Rfc3339).ok()),
    })
}

fn outcome(installed: Installed) -> &'static str {
    match installed {
        Installed::ExitNow => "exit_now",
        Installed::Relaunch => "relaunch",
        Installed::NextLaunch => "next_launch",
    }
}

/// Prints a line each time another tenth of the file (or, without a length, another MiB)
/// arrives.
fn progress(update: &Update) -> impl FnMut(u64, Option<u64>) {
    let version = update.release.version.clone();
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
            Some(total) if total > 0 => eprintln!(
                "vsesvit: downloading {version}: {}% of {total} bytes",
                received * 100 / total
            ),
            _ => eprintln!("vsesvit: downloading {version}: {received} bytes"),
        }
    }
}

fn finish(result: Result<Value, Error>) -> ExitCode {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_name_the_installation_and_version() {
        assert_eq!(
            report(&Installation::Unpackaged, "available", None),
            json!({"installation": "unpackaged", "current": env!("CARGO_PKG_VERSION"), "available": null})
        );
        let appimage = Installation::AppImage {
            image: "/opt/Vsesvit.AppImage".into(),
        };
        let installed = json!({"version": "9.0.0", "next": outcome(Installed::NextLaunch)});
        assert_eq!(
            report(&appimage, "installed", Some(installed)),
            json!({
                "installation": "appimage",
                "current": env!("CARGO_PKG_VERSION"),
                "installed": {"version": "9.0.0", "next": "next_launch"},
            })
        );
    }
}
