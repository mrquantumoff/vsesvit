//! What an extension may do, in Chrome's words, and the optional permissions the user granted it.
//!
//! The install prompt lists [`install_warnings`]: Chrome's permission messages
//! (`chrome_permission_message_rules.cc`), coalesced as Chrome coalesces them. Each rule
//! consumes the permissions that make its message and the ones it absorbs, in order, so
//! "Read and change all your data on all websites" stands for `tabs` and
//! `declarativeNetRequest` too. [`update_warnings`] is what a new version adds, which is when
//! Chrome asks again.
//!
//! `permissions.request` grants an extension some of its manifest's `optional_permissions`
//! for good (until it is uninstalled or gives them back with `permissions.remove`). The grants
//! are one local preference, [`GRANTED_PERMISSIONS`], as Chrome keeps them in the profile and
//! does not sync them. What an extension holds is [`Extensions::active_permissions`]: its
//! manifest's required permissions and whatever granted ones its manifest still lists as optional.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::manifest::{Manifest, MatchPattern};
use super::{ExtensionId, Extensions};
use crate::Error;
use crate::prefs::{Pref, Scope};

pub const GRANTED_PERMISSIONS: Pref<BTreeMap<ExtensionId, PermissionSet>> =
    Pref { key: "extensions.granted_permissions", scope: Scope::Local, default: BTreeMap::new };

/// Chrome's install prompt: [`install_heading`], then this, then the warnings; Cancel and
/// "Add extension".
pub const INSTALL_LEAD: &str = "It can:";
/// Chrome's `permissions.request` prompt: [`request_heading`], then this, then the warnings;
/// Deny and Allow.
pub const REQUEST_LEAD: &str = "It could:";

pub fn install_heading(name: &str) -> String {
    format!("Add “{name}”?")
}

pub fn request_heading(name: &str) -> String {
    format!("“{name}” has requested additional permissions.")
}

/// API permission names and host patterns, as `chrome.permissions.Permissions` has them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet {
    pub apis: BTreeSet<String>,
    pub origins: BTreeSet<MatchPattern>,
}

impl PermissionSet {
    /// A manifest list, where host patterns sit among API names (MV2 `permissions`, and
    /// `optional_permissions`). A malformed pattern is skipped, as Chrome skips it.
    pub fn from_manifest_list<'a>(entries: impl IntoIterator<Item = &'a str>) -> PermissionSet {
        let mut set = PermissionSet::default();
        for entry in entries {
            if is_pattern(entry) {
                set.origins.extend(MatchPattern::parse(entry).ok());
            } else {
                set.apis.insert(entry.to_owned());
            }
        }
        set
    }

    /// `chrome.permissions.Permissions` from an extension: an origin that is not a match
    /// pattern is refused with Chrome's message.
    pub fn from_request(apis: &[String], origins: &[String]) -> Result<PermissionSet, PermissionsError> {
        let origins = origins
            .iter()
            .map(|o| MatchPattern::parse(o).map_err(|e| PermissionsError::InvalidOrigin(o.clone(), e.to_string())))
            .collect::<Result<_, _>>()?;
        Ok(PermissionSet { apis: apis.iter().cloned().collect(), origins })
    }

    pub fn is_empty(&self) -> bool {
        self.apis.is_empty() && self.origins.is_empty()
    }

    pub fn union(&self, other: &PermissionSet) -> PermissionSet {
        PermissionSet { apis: &self.apis | &other.apis, origins: &self.origins | &other.origins }
    }

    /// Whether this set grants everything `other` asks for, as `permissions.contains` answers.
    pub fn contains(&self, other: &PermissionSet) -> bool {
        other.apis.is_subset(&self.apis) && other.origins.iter().all(|o| self.covers(o))
    }

    fn covers(&self, origin: &MatchPattern) -> bool {
        self.origins.iter().any(|held| held.covers(origin))
    }

    /// What `self` has that `held` does not.
    fn beyond(&self, held: &PermissionSet) -> PermissionSet {
        PermissionSet {
            apis: &self.apis - &held.apis,
            origins: self.origins.iter().filter(|o| !held.covers(o)).cloned().collect(),
        }
    }

    /// The part of `self` that `allowed` lists.
    fn within(&self, allowed: &PermissionSet) -> PermissionSet {
        PermissionSet {
            apis: &self.apis & &allowed.apis,
            origins: self.origins.iter().filter(|o| allowed.covers(o)).cloned().collect(),
        }
    }
}

fn is_pattern(entry: &str) -> bool {
    entry == "<all_urls>" || entry.contains("://")
}

/// The permissions an extension holds from install on.
pub fn required(manifest: &Manifest) -> PermissionSet {
    PermissionSet { apis: manifest.permissions.iter().cloned().collect(), origins: manifest.host_permissions.iter().cloned().collect() }
}

/// The permissions it may ask for with `permissions.request`.
pub fn optional(manifest: &Manifest) -> PermissionSet {
    PermissionSet::from_manifest_list(manifest.optional_permissions.iter().map(String::as_str))
}

/// One line of a prompt. `details` lists the sites behind "a number of websites".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionMessage {
    pub text: String,
    pub details: Vec<String>,
}

/// What the install prompt says the extension can do. Its content scripts' sites count, as
/// they do in Chrome.
pub fn install_warnings(manifest: &Manifest) -> Vec<PermissionMessage> {
    warnings(&prompted(manifest))
}

/// What a new version can do that the installed one could not, which Chrome asks the user
/// to approve before it runs the new version. Empty when it asks for nothing new.
pub fn update_warnings(installed: &Manifest, new: &Manifest) -> Vec<PermissionMessage> {
    added_warnings(&prompted(installed), &prompted(new))
}

fn prompted(manifest: &Manifest) -> PermissionSet {
    let mut set = required(manifest);
    set.origins.extend(manifest.content_scripts.iter().flat_map(|s| s.matches.iter().cloned()));
    set
}

/// The warnings `after` has that `before` does not.
fn added_warnings(before: &PermissionSet, after: &PermissionSet) -> Vec<PermissionMessage> {
    let had = warnings(before);
    warnings(after).into_iter().filter(|m| !had.contains(m)).collect()
}

/// What one rule says.
enum Says {
    Text(&'static str),
    /// Chrome's host list: up to three sites in the line, more as its details.
    Hosts,
}

/// One of Chrome's message rules: it applies when every permission in `requires` is left,
/// and consumes those and the ones in `absorbs`.
struct Rule {
    says: Says,
    requires: &'static [&'static str],
    absorbs: &'static [&'static str],
}

/// Stand-ins for what Chrome derives from host patterns.
const ALL_HOSTS: &str = "<all hosts>";
const SOME_HOSTS: &str = "<some hosts>";

/// What the history rules absorb.
const BROWSING: &[&str] = &["declarativeNetRequestFeedback", "favicon", "processes", "tabs", "topSites", "webNavigation"];

const fn rule(text: &'static str, requires: &'static [&'static str], absorbs: &'static [&'static str]) -> Rule {
    Rule { says: Says::Text(text), requires, absorbs }
}

/// Chrome's rules for the permissions an extension can name, in Chrome's order (which is the
/// order of the prompt). Apps' and ChromeOS's are left out.
const RULES: &[Rule] = &[
    rule("Access the page debugger backend", &["debugger"], &[]),
    rule(
        "Read and change all your data on all websites",
        &[ALL_HOSTS],
        &[SOME_HOSTS, "declarativeWebRequest", "declarativeNetRequestFeedback", "favicon", "processes", "tabs", "topSites", "webNavigation", "declarativeNetRequest", "webAuthenticationProxy"],
    ),
    rule(
        "Read and change all your data on all websites",
        &["webAuthenticationProxy"],
        &[SOME_HOSTS, "declarativeWebRequest", "declarativeNetRequestFeedback", "favicon", "processes", "tabs", "topSites", "webNavigation", "declarativeNetRequest"],
    ),
    Rule { says: Says::Hosts, requires: &[SOME_HOSTS], absorbs: &[] },
    rule("Use your microphone and camera", &["audioCapture", "videoCapture"], &[]),
    rule("Use your microphone", &["audioCapture"], &[]),
    rule("Use your camera", &["videoCapture"], &[]),
    rule("Detect your physical location", &["geolocation"], &[]),
    rule("Read and change your browsing history on all your signed-in devices", &["history"], BROWSING),
    rule("Read your browsing history on all your signed-in devices", &["tabs", "sessions"], BROWSING),
    rule("Read your browsing history", &["tabs"], BROWSING),
    rule("Read your browsing history", &["processes"], BROWSING),
    rule("Read your browsing history", &["webNavigation"], &["declarativeNetRequestFeedback", "favicon", "topSites"]),
    rule("Read your browsing history", &["declarativeNetRequestFeedback"], &["favicon", "topSites"]),
    rule("Read the icons of the websites you visit", &["favicon"], &[]),
    rule("Read a list of your most frequently visited websites", &["topSites"], &[]),
    rule("Access your printers", &["printing"], &[]),
    rule("See your printing history", &["printingMetrics"], &[]),
    rule("Block parts of web pages", &["declarativeWebRequest"], &[]),
    rule("Block content on any page", &["declarativeNetRequest"], &[]),
    rule("Access your serial devices", &["serial"], &[]),
    rule("Display notifications", &["notifications"], &[]),
    rule("Read and change your accessibility settings", &["accessibilityFeatures.modify", "accessibilityFeatures.read"], &[]),
    rule("Change your accessibility settings", &["accessibilityFeatures.modify"], &[]),
    rule("Read your accessibility settings", &["accessibilityFeatures.read"], &[]),
    rule("Access your network traffic", &["vpnProvider"], &[]),
    rule("Read and change your bookmarks", &["bookmarks"], &[]),
    rule("Read and change entries in the reading list", &["readingList"], &[]),
    rule("Read and modify data you copy and paste", &["clipboardRead", "clipboardWrite"], &[]),
    rule("Read data you copy and paste", &["clipboardRead"], &[]),
    rule("Modify data you copy and paste", &["clipboardWrite"], &[]),
    rule("Capture content of your screen", &["desktopCapture"], &[]),
    rule("Manage your downloads", &["downloads"], &[]),
    rule("Open downloaded files", &["downloads.open"], &[]),
    rule("Know your email address", &["identity.email"], &[]),
    rule("Identify and eject storage devices", &["system.storage"], &[]),
    rule(
        "Change and grant access to features such as geolocation, microphone, camera, cookies, etc., for all your websites and extensions, including this extension.",
        &["contentSettings"],
        &[],
    ),
    rule("Access document scanners attached via USB or on the local network", &["documentScan"], &[]),
    rule("Read and change anything you type", &["input"], &[]),
    rule("Manage your apps, extensions, and themes", &["management"], &[]),
    rule("Discover devices on your local network, like printers", &["mdns"], &[]),
    rule("Communicate with cooperating native applications", &["nativeMessaging"], &[]),
    rule("Change your privacy-related settings", &["privacy"], &[]),
    rule("View and manage your tab groups", &["tabGroups"], &[]),
    rule("Read all text spoken using synthesized speech", &["ttsEngine"], &[]),
    rule("Change your wallpaper", &["wallpaper"], &[]),
    rule("Use your client certificates", &["platformKeys"], &[]),
    rule("Provide certificates for authentication", &["certificateProvider"], &[]),
];

/// Chrome's messages for `set`, coalesced as Chrome coalesces them.
pub fn warnings(set: &PermissionSet) -> Vec<PermissionMessage> {
    let mut left: BTreeSet<&str> = set.apis.iter().map(String::as_str).collect();
    let hosts: BTreeSet<String> = set.origins.iter().filter_map(MatchPattern::warning_host).collect();
    if set.origins.iter().any(MatchPattern::matches_all_hosts) {
        left.insert(ALL_HOSTS);
    } else if !hosts.is_empty() {
        left.insert(SOME_HOSTS);
    }
    let mut out = Vec::new();
    for rule in RULES {
        if !rule.requires.iter().all(|p| left.contains(p)) {
            continue;
        }
        for p in rule.requires.iter().chain(rule.absorbs) {
            left.remove(p);
        }
        out.push(match rule.says {
            Says::Text(text) => PermissionMessage { text: text.to_owned(), details: Vec::new() },
            Says::Hosts => host_message(&hosts),
        });
    }
    out
}

/// "Read and change your data on a.com, all b.com sites, and c.com", or with more than three
/// sites, a line that lists them below it.
fn host_message(hosts: &BTreeSet<String>) -> PermissionMessage {
    let few = hosts.len() <= 3;
    let named: Vec<String> = hosts
        .iter()
        .map(|h| match h.strip_prefix("*.") {
            Some(domain) if few => format!("all {domain} sites"),
            Some(domain) => format!("All {domain} sites"),
            None => h.clone(),
        })
        .collect();
    let text = match named.as_slice() {
        [one] => format!("Read and change your data on {one}"),
        [a, b] => format!("Read and change your data on {a} and {b}"),
        [a, b, c] => format!("Read and change your data on {a}, {b}, and {c}"),
        _ => return PermissionMessage { text: "Read and change your data on a number of websites".into(), details: named },
    };
    PermissionMessage { text, details: Vec::new() }
}

/// Why `permissions.request` or `permissions.remove` refused, in Chrome's words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionsError {
    #[error("Only permissions specified in the manifest may be requested.")]
    NotInManifest,
    #[error("You cannot remove required permissions.")]
    Required,
    #[error("Extension must have file access enabled to request '{0}'.")]
    FileAccess(String),
    #[error("Invalid value for origin pattern {0}: {1}")]
    InvalidOrigin(String, String),
}

/// What a `permissions.request` comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// It holds all of them already.
    Held,
    /// These are new but have no warning, so Chrome grants them without asking.
    Grant(PermissionSet),
    /// The user decides on these permissions, which Chrome describes by their own warnings.
    Ask(PermissionSet, Vec<PermissionMessage>),
}

/// `permissions.request` for `requested` by an extension with `manifest` that holds `active`.
/// Local files need a file-access grant this browser does not offer, as `host_access` says.
pub fn request(manifest: &Manifest, active: &PermissionSet, requested: &PermissionSet) -> Result<Request, PermissionsError> {
    if let Some(file) = requested.origins.iter().find(|o| o.as_str().starts_with("file:")) {
        return Err(PermissionsError::FileAccess(file.as_str().to_owned()));
    }
    let listed = required(manifest).union(&optional(manifest));
    if !listed.contains(requested) {
        return Err(PermissionsError::NotInManifest);
    }
    let new = requested.beyond(active);
    if new.is_empty() {
        return Ok(Request::Held);
    }
    let warned = warnings(&new);
    Ok(if warned.is_empty() { Request::Grant(new) } else { Request::Ask(new, warned) })
}

impl Extensions<'_> {
    /// The one place a shell or the Linux runtime reads what an extension may do: its
    /// manifest's required permissions and the optional ones the user granted it that the
    /// manifest still lists.
    pub fn active_permissions(&mut self, id: &ExtensionId, manifest: &Manifest) -> PermissionSet {
        let granted = self.p.prefs().get(&GRANTED_PERMISSIONS).remove(id).unwrap_or_default();
        required(manifest).union(&granted.within(&optional(manifest)))
    }

    /// Grants `granted` (as [`request`] found it) for good.
    pub fn grant_permissions(&mut self, id: &ExtensionId, granted: &PermissionSet) -> Result<(), Error> {
        let mut all = self.p.prefs().get(&GRANTED_PERMISSIONS);
        let entry = all.entry(id.clone()).or_default();
        *entry = entry.union(granted);
        self.p.prefs().set(&GRANTED_PERMISSIONS, &all)
    }

    /// `permissions.remove`: takes back the granted permissions `removed` names and answers
    /// what the extension held of them. A required one, or one the manifest does not list,
    /// is refused as Chrome refuses it.
    pub fn remove_permissions(&mut self, id: &ExtensionId, manifest: &Manifest, removed: &PermissionSet) -> Result<PermissionSet, RemoveError> {
        let optional = optional(manifest);
        let required = required(manifest);
        if !required.union(&optional).contains(removed) {
            return Err(PermissionsError::NotInManifest.into());
        }
        if removed.apis.iter().any(|a| required.apis.contains(a)) || removed.origins.iter().any(|o| required.covers(o)) {
            return Err(PermissionsError::Required.into());
        }
        let mut all = self.p.prefs().get(&GRANTED_PERMISSIONS);
        let Some(granted) = all.get_mut(id) else { return Ok(PermissionSet::default()) };
        let taken = granted.within(&optional).within(removed);
        granted.apis.retain(|a| !removed.apis.contains(a));
        granted.origins.retain(|o| !removed.covers(o));
        if granted.is_empty() {
            all.remove(id);
        }
        self.p.prefs().set(&GRANTED_PERMISSIONS, &all)?;
        Ok(taken)
    }

    /// Forgets what the user granted `id`, when it is uninstalled.
    pub(crate) fn forget_permissions(&mut self, id: &ExtensionId) -> Result<(), Error> {
        let mut all = self.p.prefs().get(&GRANTED_PERMISSIONS);
        if all.remove(id).is_some() { self.p.prefs().set(&GRANTED_PERMISSIONS, &all) } else { Ok(()) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveError {
    #[error(transparent)]
    Refused(#[from] PermissionsError),
    #[error(transparent)]
    Profile(#[from] Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Manifest {
        Manifest::parse(json, &|_| None).expect("test manifest parses")
    }

    fn set(apis: &[&str], origins: &[&str]) -> PermissionSet {
        PermissionSet {
            apis: apis.iter().map(|s| (*s).to_owned()).collect(),
            origins: origins.iter().map(|o| MatchPattern::parse(o).unwrap()).collect(),
        }
    }

    fn texts(messages: &[PermissionMessage]) -> Vec<&str> {
        messages.iter().map(|m| m.text.as_str()).collect()
    }

    #[test]
    fn every_host_absorbs_what_it_already_reveals() {
        let probe = manifest(include_str!("../../../../tests/fixtures/extensions/probe/manifest.json"));
        assert_eq!(texts(&install_warnings(&probe)), ["Read and change all your data on all websites", "Display notifications"]);
        assert_eq!(texts(&warnings(&set(&["tabs", "bookmarks"], &["*://*.com/*"]))), ["Read and change all your data on all websites", "Read and change your bookmarks"]);
        assert!(warnings(&set(&["storage", "alarms", "activeTab", "scripting", "contextMenus", "cookies"], &[])).is_empty());
    }

    #[test]
    fn history_rules_coalesce_in_chromes_order() {
        assert_eq!(texts(&warnings(&set(&["tabs", "webNavigation", "topSites"], &[]))), ["Read your browsing history"]);
        assert_eq!(texts(&warnings(&set(&["tabs", "sessions"], &[]))), ["Read your browsing history on all your signed-in devices"]);
        assert_eq!(texts(&warnings(&set(&["history", "tabs", "favicon"], &[]))), ["Read and change your browsing history on all your signed-in devices"]);
        assert_eq!(texts(&warnings(&set(&["clipboardWrite", "clipboardRead"], &[]))), ["Read and modify data you copy and paste"]);
        assert_eq!(
            texts(&warnings(&set(&["notifications", "geolocation", "tabs"], &["https://a.example/*"]))),
            ["Read and change your data on a.example", "Detect your physical location", "Read your browsing history", "Display notifications"]
        );
    }

    #[test]
    fn hosts_are_named_up_to_three_then_listed() {
        let line = |origins: &[&str]| warnings(&set(&[], origins)).pop().unwrap();
        assert_eq!(line(&["https://a.com/x", "http://a.com:8080/*"]).text, "Read and change your data on a.com");
        assert_eq!(line(&["https://b.com/*", "*://*.a.com/*"]).text, "Read and change your data on all a.com sites and b.com");
        assert_eq!(line(&["https://a.com/*", "https://b.com/*", "https://c.com/*"]).text, "Read and change your data on a.com, b.com, and c.com");
        let many = line(&["https://a.com/*", "https://b.com/*", "https://c.com/*", "*://*.d.com/*"]);
        assert_eq!(many.text, "Read and change your data on a number of websites");
        assert_eq!(many.details, ["All d.com sites", "a.com", "b.com", "c.com"]);
        assert!(warnings(&set(&[], &["file:///*"])).is_empty(), "local files are not a site");
    }

    #[test]
    fn content_scripts_count_as_sites_at_install() {
        let m = manifest(r#"{"manifest_version":3,"name":"t","version":"1","content_scripts":[{"matches":["https://news.example/*"],"js":["c.js"]}]}"#);
        assert_eq!(texts(&install_warnings(&m)), ["Read and change your data on news.example"]);
    }

    #[test]
    fn an_update_warns_only_about_what_it_adds() {
        let old = manifest(r#"{"manifest_version":3,"name":"t","version":"1","permissions":["tabs"],"host_permissions":["https://a.com/*"]}"#);
        let same = manifest(r#"{"manifest_version":3,"name":"t","version":"2","permissions":["tabs","storage"],"host_permissions":["https://a.com/*"]}"#);
        assert!(update_warnings(&old, &same).is_empty());
        let more = manifest(r#"{"manifest_version":3,"name":"t","version":"3","permissions":["tabs","bookmarks"],"host_permissions":["https://a.com/*","https://b.com/*"]}"#);
        assert_eq!(texts(&update_warnings(&old, &more)), ["Read and change your data on a.com and b.com", "Read and change your bookmarks"]);
    }

    #[test]
    fn requests_follow_chromes_rules() {
        let m = manifest(
            r#"{"manifest_version":3,"name":"t","version":"1","permissions":["storage"],"host_permissions":["https://a.com/*"],
                "optional_permissions":["bookmarks","alarms"],"optional_host_permissions":["*://*.example/*"]}"#,
        );
        let active = required(&m);
        assert_eq!(request(&m, &active, &set(&["storage"], &["https://a.com/path"])), Ok(Request::Held));
        assert_eq!(request(&m, &active, &set(&["alarms"], &[])), Ok(Request::Grant(set(&["alarms"], &[]))), "no new warning, no prompt");
        let asked = request(&m, &active, &set(&["bookmarks"], &["https://x.example/*"])).unwrap();
        let Request::Ask(new, warned) = asked else { panic!("{asked:?}") };
        assert_eq!(new, set(&["bookmarks"], &["https://x.example/*"]));
        assert_eq!(texts(&warned), ["Read and change your data on x.example", "Read and change your bookmarks"]);
        assert_eq!(request(&m, &active, &set(&["tabs"], &[])), Err(PermissionsError::NotInManifest));
        assert_eq!(request(&m, &active, &set(&[], &["https://b.com/*"])), Err(PermissionsError::NotInManifest));
        assert_eq!(request(&m, &active, &set(&[], &["file:///*"])), Err(PermissionsError::FileAccess("file:///*".into())));
        assert!(matches!(PermissionSet::from_request(&[], &["nope".into()]), Err(PermissionsError::InvalidOrigin(..))));
    }

    #[test]
    fn a_request_is_described_by_its_own_warnings_as_in_chrome() {
        let m = manifest(r#"{"manifest_version":3,"name":"t","version":"1","host_permissions":["<all_urls>"],"optional_permissions":["tabs"]}"#);
        let Ok(Request::Ask(_, warned)) = request(&m, &required(&m), &set(&["tabs"], &[])) else { panic!("no prompt") };
        assert_eq!(texts(&warned), ["Read your browsing history"]);
    }
}

/// Grants against a real profile.
#[cfg(test)]
mod profile_tests {
    use super::*;
    use crate::crdt::Seq;
    use crate::sync::Kind;
    use crate::{OpenOptions, Profile};

    fn manifest() -> Manifest {
        Manifest::parse(
            r#"{"manifest_version":3,"name":"t","version":"1","permissions":["storage"],"host_permissions":["https://a.com/*"],
                "optional_permissions":["bookmarks","tabs"],"optional_host_permissions":["https://*.example/*"]}"#,
            &|_| None,
        )
        .unwrap()
    }

    fn set(apis: &[&str], origins: &[&str]) -> PermissionSet {
        PermissionSet {
            apis: apis.iter().map(|s| (*s).to_owned()).collect(),
            origins: origins.iter().map(|o| MatchPattern::parse(o).unwrap()).collect(),
        }
    }

    #[test]
    fn grants_last_until_removed_and_follow_the_manifest() {
        let dir = std::env::temp_dir().join(format!("vsesvit-ext-permissions-{}", uuid::Uuid::new_v4().simple()));
        let mut p = Profile::open(&dir, OpenOptions::default()).unwrap();
        let id = ExtensionId::parse("abcdefghijklmnopabcdefghijklmnop").unwrap();
        let m = manifest();
        assert_eq!(p.extensions().active_permissions(&id, &m), set(&["storage"], &["https://a.com/*"]));

        p.extensions().grant_permissions(&id, &set(&["bookmarks"], &["https://x.example/*"])).unwrap();
        p.extensions().grant_permissions(&id, &set(&["tabs"], &[])).unwrap();
        assert_eq!(p.extensions().active_permissions(&id, &m), set(&["bookmarks", "storage", "tabs"], &["https://a.com/*", "https://x.example/*"]));

        let narrower = Manifest::parse(r#"{"manifest_version":3,"name":"t","version":"2","optional_permissions":["tabs"]}"#, &|_| None).unwrap();
        assert_eq!(p.extensions().active_permissions(&id, &narrower), set(&["tabs"], &[]), "a version that no longer lists them does not hold them");

        assert!(matches!(p.extensions().remove_permissions(&id, &m, &set(&["storage"], &[])), Err(RemoveError::Refused(PermissionsError::Required))));
        assert!(matches!(p.extensions().remove_permissions(&id, &m, &set(&["history"], &[])), Err(RemoveError::Refused(PermissionsError::NotInManifest))));
        let taken = p.extensions().remove_permissions(&id, &m, &set(&["bookmarks"], &["https://*.example/*"])).unwrap();
        assert_eq!(taken, set(&["bookmarks"], &["https://x.example/*"]));
        assert_eq!(p.extensions().active_permissions(&id, &m), set(&["storage", "tabs"], &["https://a.com/*"]));

        p.extensions().forget_permissions(&id).unwrap();
        assert_eq!(p.extensions().active_permissions(&id, &m), required(&m));
        drop(p);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grants_stay_in_their_profile_on_this_device() {
        let root = std::env::temp_dir().join(format!("vsesvit-ext-permissions-{}", uuid::Uuid::new_v4().simple()));
        let mut first = Profile::open(&root.join("Default"), OpenOptions::default()).unwrap();
        let mut second = Profile::open(&root.join("Profile 1"), OpenOptions::default()).unwrap();
        let id = ExtensionId::parse("abcdefghijklmnopabcdefghijklmnop").unwrap();
        let m = manifest();
        first.extensions().grant_permissions(&id, &set(&["tabs"], &[])).unwrap();
        assert!(first.extensions().active_permissions(&id, &m).apis.contains("tabs"));
        assert_eq!(second.extensions().active_permissions(&id, &m), required(&m), "another profile's extension holds only what it requires");

        let upload = first.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap();
        assert!(upload.records.iter().all(|r| r.id.as_str() != GRANTED_PERMISSIONS.key), "grants are never synced");
        drop((first, second));
        let _ = std::fs::remove_dir_all(&root);
    }
}
