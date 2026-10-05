//! The checks the Windows self-test reports in `report.json` (docs/design/self-test.md);
//! `vsesvit_core::testkit::report` writes it.

/// The checks, in the order the run makes them (docs/design/self-test.md says what each proves).
pub(crate) const CHECKS: [&str; 28] = [
    "profile_open",
    "install_crx",
    "engine_loaded_extension",
    "navigate",
    "history_recorded",
    "content_script",
    "extension_port",
    "dnr_blocked",
    "bookmark",
    "bookmarks_bar_icons",
    "tabs",
    "tab_layout",
    "zoom_is_remembered_per_site",
    "popup",
    "omnibox",
    "address_completion",
    "selection_search",
    "search_engines",
    "session",
    "new_tab_page",
    "tracking_protection",
    "passwords_purged",
    "shortcuts",
    "shortcuts_sync",
    "save_page",
    "page_commands",
    "download",
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of the run's `check(report, "name", ...)` calls, in source order.
    fn checks_in(source: &str) -> Vec<&str> {
        source
            .split("check(")
            .zip(source.split("check(").skip(1))
            .filter(|(before, _)| !before.ends_with(|c: char| c.is_alphanumeric() || c == '_'))
            .filter_map(|(_, call)| {
                let call = call.trim_start().strip_prefix("report,")?.trim_start();
                call.strip_prefix('"')?.split('"').next()
            })
            .collect()
    }

    #[test]
    fn checks_are_the_ones_the_run_makes_in_its_order() {
        let mut run = vec!["profile_open"];
        run.extend(checks_in(include_str!("selftest.rs")));
        assert_eq!(run, expected(true));
    }
}
