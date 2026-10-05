//! What a manifest asks for that this runtime cannot provide. The extensions page shows
//! the result next to each installed extension, so a user knows before wondering why a
//! feature is silent. Pure: manifest in, sorted list out, no I/O.

use std::fmt;

use vsesvit_core::extensions::manifest::Manifest;

/// API permissions the runtime implements (see the crate docs). `activeTab` and
/// `unlimitedStorage` change no behaviour here, so they count as honoured; `menus` is
/// Firefox's name for `contextMenus`.
pub const SUPPORTED_PERMISSIONS: &[&str] = &[
    "activeTab",
    "alarms",
    "contextMenus",
    "declarativeNetRequest",
    "declarativeNetRequestWithHostAccess",
    "menus",
    "scripting",
    "storage",
    "tabs",
    "unlimitedStorage",
];

/// Top-level manifest keys that declare a surface the runtime does not build.
pub const UNSUPPORTED_MANIFEST_KEYS: &[&str] = &[
    "chrome_settings_overrides",
    "chrome_url_overrides",
    "commands",
    "devtools_page",
    "nacl_modules",
    "omnibox",
    "sandbox",
    "side_panel",
    "sidebar_action",
    "user_scripts",
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unsupported {
    /// A required `permissions` entry.
    Permission(String),
    /// An `optional_permissions` entry the extension may ask for at runtime.
    OptionalPermission(String),
    /// A manifest key whose feature has no implementation.
    ManifestKey(String),
}

impl Unsupported {
    pub fn name(&self) -> &str {
        match self {
            Unsupported::Permission(n) | Unsupported::OptionalPermission(n) | Unsupported::ManifestKey(n) => n,
        }
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unsupported::Permission(n) => write!(f, "{n}"),
            Unsupported::OptionalPermission(n) => write!(f, "{n} (optional)"),
            Unsupported::ManifestKey(n) => write!(f, "{n} (manifest key)"),
        }
    }
}

/// What `externally_connectable` asks for that the runtime does not do: messages from
/// other extensions (`ids`) arrive, but no web page (`matches`) gets an API to send any.
pub const EXTERNAL_WEB_PAGES: &str = "externally_connectable.matches";

/// Every permission and manifest surface the extension requests that the Linux runtime
/// lacks, sorted and deduplicated. Host patterns are never reported: the manifest model
/// moves MV2 host entries out of `permissions`, and any pattern left over is a host
/// grant, which content scripts and CORS honour.
pub fn unsupported_features(manifest: &Manifest) -> Vec<Unsupported> {
    let mut out: Vec<Unsupported> = manifest
        .permissions
        .iter()
        .filter(|p| !is_supported_permission(p))
        .map(|p| Unsupported::Permission(p.clone()))
        .chain(
            manifest
                .optional_permissions
                .iter()
                .filter(|p| !is_supported_permission(p))
                .map(|p| Unsupported::OptionalPermission(p.clone())),
        )
        .chain(
            UNSUPPORTED_MANIFEST_KEYS
                .iter()
                .filter(|key| manifest.raw.get(**key).is_some_and(|v| !v.is_null()))
                .map(|key| Unsupported::ManifestKey((*key).to_owned())),
        )
        .chain(
            manifest.raw["externally_connectable"]["matches"]
                .as_array()
                .is_some_and(|m| !m.is_empty())
                .then(|| Unsupported::ManifestKey(EXTERNAL_WEB_PAGES.to_owned())),
        )
        .collect();
    out.sort();
    out.dedup();
    out
}

fn is_supported_permission(permission: &str) -> bool {
    SUPPORTED_PERMISSIONS.contains(&permission) || is_host_pattern(permission)
}

fn is_host_pattern(permission: &str) -> bool {
    permission == "<all_urls>" || permission.contains("://")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Manifest {
        Manifest::parse(json, &|_| None).expect("test manifest parses")
    }

    #[test]
    fn the_probe_manifest_is_fully_supported() {
        let text = include_str!("../../../tests/fixtures/extensions/probe/manifest.json");
        assert_eq!(unsupported_features(&manifest(text)), Vec::new());
    }

    #[test]
    fn unknown_and_unimplemented_permissions_are_reported_sorted() {
        let m = manifest(
            r#"{
                "manifest_version": 3, "name": "x", "version": "1",
                "permissions": ["webRequest", "storage", "nativeMessaging", "webRequest", "cookies", "contextMenus", "<all_urls>", "*://example.com/*"],
                "optional_permissions": ["bookmarks", "tabs", "menus"],
                "host_permissions": ["https://*/*"]
            }"#,
        );
        assert_eq!(
            unsupported_features(&m),
            vec![
                Unsupported::Permission("cookies".into()),
                Unsupported::Permission("nativeMessaging".into()),
                Unsupported::Permission("webRequest".into()),
                Unsupported::OptionalPermission("bookmarks".into()),
            ]
        );
    }

    #[test]
    fn manifest_keys_without_an_implementation_are_reported() {
        let m = manifest(
            r#"{
                "manifest_version": 2, "name": "x", "version": "1",
                "devtools_page": "devtools.html",
                "omnibox": { "keyword": "x" },
                "commands": { "_execute_browser_action": { "suggested_key": { "default": "Ctrl+Shift+Y" } } },
                "background": { "scripts": ["bg.js"] }
            }"#,
        );
        let found = unsupported_features(&m);
        let names: Vec<&str> = found.iter().map(Unsupported::name).collect();
        assert_eq!(names, ["commands", "devtools_page", "omnibox"]);
        assert_eq!(Unsupported::ManifestKey("omnibox".into()).to_string(), "omnibox (manifest key)");
        assert_eq!(Unsupported::OptionalPermission("bookmarks".into()).to_string(), "bookmarks (optional)");
    }

    #[test]
    fn mv2_host_patterns_in_permissions_are_not_reported() {
        let m = manifest(
            r#"{ "manifest_version": 2, "name": "x", "version": "1",
                 "permissions": ["http://*/*", "storage", "activeTab", "unlimitedStorage"] }"#,
        );
        assert!(unsupported_features(&m).is_empty());
    }

    #[test]
    fn externally_connectable_is_reported_only_for_web_pages() {
        let ids = manifest(r#"{ "manifest_version": 3, "name": "x", "version": "1", "externally_connectable": { "ids": ["*"] } }"#);
        assert!(unsupported_features(&ids).is_empty());
        let pages = manifest(r#"{ "manifest_version": 3, "name": "x", "version": "1", "externally_connectable": { "ids": ["*"], "matches": ["https://x.test/*"] } }"#);
        assert_eq!(unsupported_features(&pages), vec![Unsupported::ManifestKey(EXTERNAL_WEB_PAGES.into())]);
        let ports = include_str!("../../../tests/fixtures/extensions/ports/manifest.json");
        assert!(unsupported_features(&manifest(ports)).is_empty());
    }
}
