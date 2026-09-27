//! Which `runtime.onInstalled` / `runtime.onStartup` event a load fires. Pure: the
//! decision is a function of the previous install marker, the install being loaded and
//! why the shell loads it, so the rule is testable on every host.

/// Why the shell is loading an extension.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LoadReason {
    /// Browser startup, or the load right after an install or update.
    Startup,
    /// The user re-enabled an installed extension. Chrome fires no lifecycle event.
    Enable,
}

/// What the background context is told once it has loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallEvent {
    /// `runtime.onInstalled({reason: "install"})`
    Installed,
    /// `runtime.onInstalled({reason: "update", previousVersion})`
    Updated { previous: String },
    /// `runtime.onStartup`
    Startup,
    /// Nothing (a re-enable).
    Nothing,
}

/// The marker records the last version loaded and a stamp of the install it came from
/// (`<version>\n<stamp>`). Returns the event and, when the marker must change, its new
/// text. The stamp is what makes an uninstall followed by a reinstall of the same
/// version an install again: core removes the extension's files on uninstall, so the
/// reinstalled directory is a new one, while the marker (runtime state outside core's
/// layout) survives. A marker without a stamp is from an older runtime and is upgraded
/// silently.
pub fn install_event(marker: Option<&str>, version: &str, stamp: &str, reason: LoadReason) -> (InstallEvent, Option<String>) {
    let fresh = format!("{version}\n{stamp}");
    let Some(marker) = marker else { return (InstallEvent::Installed, Some(fresh)) };
    let mut lines = marker.lines().map(str::trim);
    let previous = lines.next().unwrap_or_default();
    let previous_stamp = lines.next();
    if previous != version {
        return (InstallEvent::Updated { previous: previous.to_owned() }, Some(fresh));
    }
    let same_load = match reason {
        LoadReason::Startup => InstallEvent::Startup,
        LoadReason::Enable => InstallEvent::Nothing,
    };
    match previous_stamp {
        None => (same_load, Some(fresh)),
        Some(s) if s == stamp => (same_load, None),
        Some(_) => (InstallEvent::Installed, Some(fresh)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_load_installs_and_writes_the_marker() {
        assert_eq!(install_event(None, "1.2", "a", LoadReason::Startup), (InstallEvent::Installed, Some("1.2\na".into())));
        assert_eq!(install_event(None, "1.2", "a", LoadReason::Enable), (InstallEvent::Installed, Some("1.2\na".into())));
    }

    #[test]
    fn later_loads_of_the_same_install_are_startups_and_enables_fire_nothing() {
        assert_eq!(install_event(Some("1.2\na"), "1.2", "a", LoadReason::Startup), (InstallEvent::Startup, None));
        assert_eq!(install_event(Some("1.2\na\n"), "1.2", "a", LoadReason::Enable), (InstallEvent::Nothing, None));
    }

    #[test]
    fn a_new_version_is_an_update() {
        let (event, marker) = install_event(Some("1.2\na"), "1.3", "b", LoadReason::Enable);
        assert_eq!(event, InstallEvent::Updated { previous: "1.2".into() });
        assert_eq!(marker.as_deref(), Some("1.3\nb"));
    }

    /// Uninstall wipes `storage.local`; the reinstall must run first-run setup again.
    #[test]
    fn a_reinstall_of_the_same_version_is_an_install_again() {
        let (event, marker) = install_event(Some("1.2\na"), "1.2", "b", LoadReason::Startup);
        assert_eq!(event, InstallEvent::Installed);
        assert_eq!(marker.as_deref(), Some("1.2\nb"));
    }

    #[test]
    fn a_marker_without_a_stamp_is_upgraded_without_an_event() {
        assert_eq!(install_event(Some("1.2"), "1.2", "a", LoadReason::Startup), (InstallEvent::Startup, Some("1.2\na".into())));
        assert_eq!(install_event(Some("1.2\n"), "1.2", "a", LoadReason::Enable), (InstallEvent::Nothing, Some("1.2\na".into())));
    }
}
