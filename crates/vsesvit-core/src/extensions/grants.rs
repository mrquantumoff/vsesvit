//! What an extension may do, and what a new version asks for beyond what the user
//! approved. Pure, and empty when nothing is new, which is all the update check asks of it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::manifest::{Manifest, MatchPattern};

/// API permissions Chrome grants without an install warning, so a version that adds one
/// needs no approval.
const SILENT_PERMISSIONS: &[&str] = &[
    "activeTab",
    "alarms",
    "background",
    "contextMenus",
    "cookies",
    "declarativeContent",
    "declarativeNetRequestWithHostAccess",
    "dns",
    "fontSettings",
    "gcm",
    "identity",
    "idle",
    "menus",
    "offscreen",
    "power",
    "scripting",
    "sidePanel",
    "storage",
    "system.cpu",
    "system.display",
    "system.memory",
    "tts",
    "unlimitedStorage",
    "webRequest",
    "webRequestBlocking",
];

/// API permissions by name, and the hosts the extension may read and change. No host
/// pattern covers another (see [`MatchPattern::covers`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet {
    pub api: BTreeSet<String>,
    pub hosts: Vec<MatchPattern>,
}

impl PermissionSet {
    /// What `manifest` asks for at install: its API and host permissions, and the pages its
    /// content scripts run on, which Chrome counts as host access too. Optional permissions
    /// are left out: the extension asks for those while it runs.
    pub fn required(manifest: &Manifest) -> PermissionSet {
        let mut set = PermissionSet { api: manifest.permissions.iter().cloned().collect(), hosts: Vec::new() };
        let scripts = manifest.content_scripts.iter().flat_map(|script| &script.matches);
        for pattern in manifest.host_permissions.iter().chain(scripts) {
            set.add_host(pattern);
        }
        set
    }

    /// Keeps `hosts` free of patterns another one covers.
    fn add_host(&mut self, pattern: &MatchPattern) {
        if self.hosts.iter().any(|host| host.covers(pattern)) {
            return;
        }
        self.hosts.retain(|host| !pattern.covers(host));
        self.hosts.push(pattern.clone());
    }

    pub fn is_empty(&self) -> bool {
        self.api.is_empty() && self.hosts.is_empty()
    }

    /// For display: the API permissions, then the host patterns as the manifest spells them.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.api.iter().map(String::as_str).chain(self.hosts.iter().map(MatchPattern::as_str))
    }
}

/// What `new` asks for that `old` does not grant and that Chrome would warn about: API
/// permissions `old` lacks, except those Chrome grants silently, and host patterns no
/// pattern in `old` covers.
pub fn permissions_added(old: &PermissionSet, new: &PermissionSet) -> PermissionSet {
    PermissionSet {
        api: new.api.iter().filter(|name| !old.api.contains(*name) && !SILENT_PERMISSIONS.contains(&name.as_str())).cloned().collect(),
        hosts: new.hosts.iter().filter(|host| !old.hosts.iter().any(|granted| granted.covers(host))).cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(fields: &str) -> Manifest {
        Manifest::parse(&format!(r#"{{"manifest_version": 3, "name": "A", "version": "1"{fields}}}"#), &|_| None).unwrap()
    }

    fn set(api: &[&str], hosts: &[&str]) -> PermissionSet {
        PermissionSet {
            api: api.iter().map(|s| (*s).to_owned()).collect(),
            hosts: hosts.iter().map(|s| MatchPattern::parse(s).unwrap()).collect(),
        }
    }

    #[test]
    fn required_counts_content_script_pages_as_host_access() {
        let m = manifest(
            r#", "permissions": ["tabs", "storage", "http://legacy.test/*"],
                "host_permissions": ["https://a.test/*", "https://a.test/other/*"],
                "optional_permissions": ["history"],
                "content_scripts": [{"matches": ["https://b.test/*", "https://x.a.test/*"], "js": ["c.js"]},
                                    {"matches": ["https://a.test/page"], "js": ["d.js"]}]"#,
        );
        let required = PermissionSet::required(&m);
        assert_eq!(required, set(&["storage", "tabs"], &["http://legacy.test/*", "https://a.test/*", "https://b.test/*", "https://x.a.test/*"]));
        assert_eq!(required.names().collect::<Vec<_>>(), ["storage", "tabs", "http://legacy.test/*", "https://a.test/*", "https://b.test/*", "https://x.a.test/*"]);
        assert!(PermissionSet::required(&manifest("")).is_empty());
    }

    #[test]
    fn a_broader_host_replaces_the_ones_it_covers() {
        let m = manifest(r#", "host_permissions": ["https://a.test/*", "*://*.b.test/*"], "content_scripts": [{"matches": ["<all_urls>"], "js": ["c.js"]}]"#);
        assert_eq!(PermissionSet::required(&m).hosts, set(&[], &["<all_urls>"]).hosts);
    }

    #[test]
    fn added_permissions_skip_what_chrome_grants_silently() {
        let old = set(&["storage"], &[]);
        let new = set(&["storage", "alarms", "contextMenus", "scripting", "unlimitedStorage", "tabs", "history"], &[]);
        assert_eq!(permissions_added(&old, &new), set(&["history", "tabs"], &[]));
        assert!(permissions_added(&new, &old).is_empty(), "dropping permissions adds none");
        assert!(permissions_added(&old, &old).is_empty());
    }

    #[test]
    fn added_hosts_are_the_ones_no_granted_pattern_covers() {
        let old = set(&[], &["*://*.a.test/*", "http://local.test:8080/*"]);
        let new = set(&[], &["https://x.a.test/path*", "http://local.test:8080/*", "http://local.test/*", "https://b.test/*"]);
        assert_eq!(permissions_added(&old, &new), set(&[], &["http://local.test/*", "https://b.test/*"]));

        let all = set(&[], &["<all_urls>"]);
        let web = set(&[], &["*://*/*"]);
        assert!(permissions_added(&all, &web).is_empty());
        assert_eq!(permissions_added(&web, &all), all, "<all_urls> adds file and ftp");
        assert_eq!(permissions_added(&set(&[], &[]), &web), web);
    }

    #[test]
    fn granted_sets_round_trip_through_json() {
        let granted = set(&["tabs"], &["https://a.test/*"]);
        let json = serde_json::to_string(&granted).unwrap();
        assert_eq!(json, r#"{"api":["tabs"],"hosts":["https://a.test/*"]}"#);
        assert_eq!(serde_json::from_str::<PermissionSet>(&json).unwrap(), granted);
    }
}
