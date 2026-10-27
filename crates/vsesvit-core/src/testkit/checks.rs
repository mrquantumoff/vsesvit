//! Owned by C.0.
//! The self-test's check names: the list every flavour shares plus each flavour's extras
//! (docs/design/linux-cef/design.md section 4.3, row 14), so `Report::complete` never asks a
//! flavour for a check it lacks. The GTK shell keeps its own list until it is deleted. The GPUI
//! shell's self-test grows with its waves, so its list is the checks it runs so far.
//!
//! This file uses nothing outside itself: `xtask selftest` compiles it in with `#[path]` and
//! validates a run's `report.json` against the same list.

use Flavour::{Gpui, Winui};
use Runs::{Every, Only};

/// A shell whose self-test reports these checks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Flavour {
    Winui,
    Gpui,
}

#[derive(Copy, Clone)]
enum Runs {
    Every,
    Only(Flavour),
}

/// Every check, in the order the shells run them (docs/design/self-test.md says what each proves;
/// docs/design/linux-cef/design.md sections 7.2 and 7.5 the GPUI shell's own).
const CHECKS: [(&str, Runs); 55] = [
    ("profile_open", Every),
    ("single_thread_init", Only(Gpui)),
    ("install_crx", Every),
    ("engine_loaded_extension", Every),
    ("navigate", Every),
    ("history_recorded", Every),
    ("content_script", Every),
    ("extension_port", Every),
    ("dnr_blocked", Every),
    ("dnr_site_allowed", Every),
    ("bookmark", Every),
    ("bookmarks_bar_icons", Only(Winui)),
    ("bookmark_export", Every),
    ("tabs", Every),
    ("tab_layout", Every),
    ("tab_menu", Every),
    ("tab_groups", Every),
    ("tab_search", Every),
    ("memory_saver", Every),
    ("zoom_is_remembered_per_site", Every),
    ("popup", Every),
    ("omnibox", Every),
    ("address_completion", Every),
    ("search_suggestions", Every),
    ("selection_search", Every),
    ("search_engines", Every),
    ("session", Every),
    ("private_window", Every),
    ("new_tab_page", Every),
    ("tracking_protection", Every),
    ("https_only", Every),
    ("cookies", Every),
    ("passwords_purged", Only(Winui)),
    ("shortcuts", Every),
    ("shortcuts_sync", Only(Winui)),
    ("sync_passphrase", Every),
    ("profiles", Every),
    ("save_page", Every),
    ("page_commands", Every),
    ("context_menus", Every),
    ("extension_commands", Every),
    ("extension_notifications", Every),
    ("extension_update", Every),
    ("download", Every),
    ("download_pause", Only(Winui)),
    ("download_speed", Every),
    ("download_safety", Every),
    ("keys", Only(Gpui)),
    ("page_focus_policy", Only(Gpui)),
    ("resources", Only(Gpui)),
    ("screenshot", Every),
    ("shutdown", Only(Gpui)),
    ("shutdown_delayed", Only(Gpui)),
    ("shutdown_stuck", Only(Gpui)),
    ("shutdown_late_creation", Only(Gpui)),
];

/// The checks the GPUI shell runs so far: wave 1's foundation (design section 8.3, W1.8). Each GPUI
/// unit that adds a check adds its name; once the GPUI shell runs the whole table, this list goes.
const GPUI_SO_FAR: [&str; 15] = [
    "profile_open",
    "single_thread_init",
    "navigate",
    "tabs",
    "tab_layout",
    "session",
    "shortcuts",
    "keys",
    "page_focus_policy",
    "resources",
    "screenshot",
    "shutdown",
    "shutdown_delayed",
    "shutdown_stuck",
    "shutdown_late_creation",
];

/// Every flavour runs it, last, and only with `--network`.
pub const NETWORK_CHECK: &str = "cws_install";

/// The checks a `flavour` run must report, in the order it makes them.
pub fn expected(flavour: Flavour, network: bool) -> Vec<&'static str> {
    table(flavour, network).into_iter().filter(|name| flavour == Winui || GPUI_SO_FAR.contains(name)).collect()
}

/// Every check of the table a `flavour` run makes once it is complete.
fn table(flavour: Flavour, network: bool) -> Vec<&'static str> {
    CHECKS
        .iter()
        .filter(|(_, runs)| match runs {
            Every => true,
            Only(only) => *only == flavour,
        })
        .map(|(name, _)| *name)
        .chain(network.then_some(NETWORK_CHECK))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winui_runs_47_checks_and_cws_install_with_network() {
        let checks = expected(Flavour::Winui, false);
        assert_eq!(checks.len(), 47);
        assert_eq!(checks.first(), Some(&"profile_open"));
        assert_eq!(checks.last(), Some(&"screenshot"));
        let with_network = expected(Flavour::Winui, true);
        assert_eq!(with_network[..47], checks[..]);
        assert_eq!(with_network[47..], [NETWORK_CHECK]);
    }

    #[test]
    fn every_flavour_shares_43_checks_and_adds_its_own() {
        let shared: Vec<_> = CHECKS.iter().filter(|(_, runs)| matches!(runs, Every)).map(|(name, _)| *name).collect();
        assert_eq!(shared.len(), 43);
        for flavour in [Flavour::Winui, Flavour::Gpui] {
            let checks = table(flavour, false);
            assert!(shared.iter().all(|name| checks.contains(name)), "{flavour:?} lacks a shared check");
        }
        let extras = |flavour| -> Vec<_> { table(flavour, false).into_iter().filter(|name| !shared.contains(name)).collect() };
        assert_eq!(extras(Flavour::Winui), ["bookmarks_bar_icons", "passwords_purged", "shortcuts_sync", "download_pause"]);
        assert_eq!(
            extras(Flavour::Gpui),
            [
                "single_thread_init",
                "keys",
                "page_focus_policy",
                "resources",
                "shutdown",
                "shutdown_delayed",
                "shutdown_stuck",
                "shutdown_late_creation"
            ]
        );
    }

    #[test]
    fn gpui_runs_its_foundation_so_far_in_table_order() {
        let checks = expected(Flavour::Gpui, false);
        assert_eq!(checks, GPUI_SO_FAR, "every name is in the table, in its order");
        assert_eq!(expected(Flavour::Gpui, true), checks, "the GPUI shell has no network check yet");
    }

    #[test]
    fn names_are_unique_snake_case() {
        let mut names: Vec<_> = CHECKS.iter().map(|(name, _)| *name).chain([NETWORK_CHECK]).collect();
        assert!(names.iter().all(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), CHECKS.len() + 1);
    }
}
