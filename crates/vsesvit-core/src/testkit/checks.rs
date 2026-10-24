//! Owned by C.0.
//! The self-test's check names: the list every flavour shares plus each flavour's extras
//! (docs/design/linux-cef/design.md section 4.3, row 14), so `Report::complete` never asks a
//! flavour for a check it lacks. The GTK shell keeps its own list until it is deleted.
//!
//! This file uses nothing outside itself: `xtask selftest` compiles it in with `#[path]` and
//! validates a run's `report.json` against the same list.

use Flavour::Winui;
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

/// Every check, in the order WinUI runs them (docs/design/self-test.md says what each proves).
const CHECKS: [(&str, Runs); 47] = [
    ("profile_open", Every),
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
    ("screenshot", Every),
];

/// Every flavour runs it, last, and only with `--network`.
pub const NETWORK_CHECK: &str = "cws_install";

/// The checks a `flavour` run must report, in the order it makes them.
pub fn expected(flavour: Flavour, network: bool) -> Vec<&'static str> {
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
    fn every_flavour_shares_43_checks_and_winui_adds_4() {
        let shared: Vec<_> = CHECKS.iter().filter(|(_, runs)| matches!(runs, Every)).map(|(name, _)| *name).collect();
        assert_eq!(shared.len(), 43);
        for flavour in [Flavour::Winui, Flavour::Gpui] {
            let checks = expected(flavour, false);
            assert!(shared.iter().all(|name| checks.contains(name)), "{flavour:?} lacks a shared check");
        }
        let extras: Vec<_> = expected(Flavour::Winui, false).into_iter().filter(|name| !shared.contains(name)).collect();
        assert_eq!(extras, ["bookmarks_bar_icons", "passwords_purged", "shortcuts_sync", "download_pause"]);
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
