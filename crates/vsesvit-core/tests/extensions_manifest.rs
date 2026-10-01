//! Manifest normalization: one shape out of the MV2/MV3 and Chrome/Firefox dialects.

use std::path::{Path, PathBuf};

use vsesvit_core::extensions::manifest::{Background, Manifest, ManifestError, ManifestVersion, RunAt, World};

fn parse(text: &str) -> Result<Manifest, ManifestError> {
    Manifest::parse(text, &|_| None)
}

fn strs<T: AsRef<str>>(items: impl IntoIterator<Item = T>) -> Vec<String> {
    items.into_iter().map(|s| s.as_ref().to_owned()).collect()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("vsesvit-manifest-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn mv2_browser_action_becomes_action() {
    let m = parse(
        r#"{"manifest_version": 2, "name": "A", "version": "1.2.3",
            "browser_action": {"default_popup": "/ui/popup.html", "default_title": "Open", "default_icon": "icon.png"}}"#,
    )
    .unwrap();
    assert_eq!(m.manifest_version, ManifestVersion::V2);
    let action = m.action.unwrap();
    assert_eq!(action.default_popup.unwrap().as_str(), "ui/popup.html");
    assert_eq!(action.default_title.as_deref(), Some("Open"));
    assert_eq!(action.default_icon.get(&16).unwrap().as_str(), "icon.png");

    let page = parse(r#"{"manifest_version": 2, "name": "A", "version": "1", "page_action": {"default_icon": {"19": "a.png", "38": "b.png"}, "default_popup": ""}}"#)
        .unwrap()
        .action
        .unwrap();
    assert_eq!(page.default_popup, None, "an empty popup means none");
    assert_eq!(page.default_icon.keys().copied().collect::<Vec<_>>(), [19, 38]);

    let mv3 = parse(r#"{"manifest_version": 3, "name": "A", "version": "1", "action": {"default_title": "new"}, "browser_action": {"default_title": "old"}}"#).unwrap();
    assert_eq!(mv3.action.unwrap().default_title.as_deref(), Some("new"));
}

#[test]
fn host_patterns_inside_permissions_move_to_host_permissions() {
    let m = parse(
        r#"{"manifest_version": 2, "name": "A", "version": "1",
            "permissions": ["storage", "tabs", "<all_urls>", "*://*.example.com/*", "https://bad*.com/", {"socket": []}],
            "host_permissions": ["https://example.org/*", "*://*.example.com/*"],
            "optional_permissions": ["bookmarks"], "optional_host_permissions": ["https://opt.example/*"]}"#,
    )
    .unwrap();
    assert_eq!(m.permissions, strs(["storage", "tabs"]));
    assert_eq!(strs(m.host_permissions.iter().map(|p| p.as_str())), strs(["<all_urls>", "*://*.example.com/*", "https://example.org/*"]));
    assert_eq!(m.optional_permissions, strs(["bookmarks", "https://opt.example/*"]));
}

#[test]
fn each_background_form() {
    let bg = |json: &str| parse(json).unwrap().background;
    match bg(r#"{"manifest_version": 3, "name": "A", "version": "1", "background": {"service_worker": "sw.js", "type": "module"}}"#) {
        Some(Background::ServiceWorker { script, module: true }) => assert_eq!(script.as_str(), "sw.js"),
        other => panic!("{other:?}"),
    }
    match bg(r#"{"manifest_version": 2, "name": "A", "version": "1", "background": {"scripts": ["a.js", "./b.js"]}}"#) {
        Some(Background::Scripts { scripts, persistent: true }) => {
            assert_eq!(strs(scripts.iter().map(|s| s.as_str())), strs(["a.js", "b.js"]))
        }
        other => panic!("MV2 scripts default to persistent: {other:?}"),
    }
    match bg(r#"{"manifest_version": 2, "name": "A", "version": "1", "background": {"page": "bg.html", "persistent": false}}"#) {
        Some(Background::Page { page, persistent: false }) => assert_eq!(page.as_str(), "bg.html"),
        other => panic!("{other:?}"),
    }
    // Firefox MV3 event page: not persistent.
    assert!(matches!(
        bg(r#"{"manifest_version": 3, "name": "A", "version": "1", "background": {"scripts": ["a.js"]}}"#),
        Some(Background::Scripts { persistent: false, .. })
    ));
    // Cross-browser MV3: Chrome runs the service worker and ignores `scripts`.
    assert!(matches!(
        bg(r#"{"manifest_version": 3, "name": "A", "version": "1", "background": {"service_worker": "sw.js", "scripts": ["a.js"]}}"#),
        Some(Background::ServiceWorker { module: false, .. })
    ));
    assert_eq!(bg(r#"{"manifest_version": 3, "name": "A", "version": "1", "background": {"scripts": []}}"#), None);
    assert!(parse(r#"{"manifest_version": 3, "name": "A", "version": "1", "background": {"service_worker": "../sw.js"}}"#).is_err());
}

#[test]
fn messages_resolve_from_ui_locale_then_language_then_default_locale() {
    let dir = TempDir::new();
    dir.write(
        "manifest.json",
        r#"{"manifest_version": 3, "name": "__MSG_extName__", "version": "1", "default_locale": "en",
            "description": "__MSG_extDescription__", "action": {"default_title": "__MSG_Title__ (__MSG_missing__)"}}"#,
    );
    dir.write(
        "_locales/en/messages.json",
        r#"{"extName": {"message": "Probe"}, "extDescription": {"message": "Counts $what$", "placeholders": {"what": {"content": "visits"}}},
            "title": {"message": "Open"}, }"#,
    );
    dir.write("_locales/uk/messages.json", "// Ukrainian\n{\"extname\": {\"message\": \"Зонд\"}}");
    dir.write("_locales/uk_UA/messages.json", r#"{"title": {"message": "Відкрити"}}"#);

    let uk = Manifest::load(dir.path(), "uk-UA").unwrap();
    assert_eq!(uk.name, "Зонд", "language bundle, case-insensitive key");
    assert_eq!(uk.description.as_deref(), Some("Counts visits"), "default_locale fallback");
    assert_eq!(uk.action.unwrap().default_title.as_deref(), Some("Відкрити (__MSG_missing__)"), "region bundle; unknown key kept");

    let de = Manifest::load(dir.path(), "de").unwrap();
    assert_eq!(de.name, "Probe");
    assert_eq!(de.raw["name"], "__MSG_extName__", "raw keeps the unlocalized text");
}

#[test]
fn default_locale_cannot_escape_the_extension_dir() {
    let dir = TempDir::new();
    dir.write("inner/manifest.json", r#"{"manifest_version": 3, "name": "__MSG_n__", "version": "1", "default_locale": "../../x"}"#);
    dir.write("x/messages.json", r#"{"n": {"message": "escaped"}}"#);
    assert_eq!(Manifest::load(&dir.path().join("inner"), "en").unwrap().name, "__MSG_n__");
}

#[test]
fn files_may_start_with_one_byte_order_mark() {
    let dir = TempDir::new();
    let manifest = r#"{"manifest_version": 3, "name": "__MSG_n__", "version": "1", "default_locale": "en"}"#;
    dir.write("manifest.json", &format!("\u{feff}{manifest}"));
    dir.write("_locales/en/messages.json", "\u{feff}{\"n\": {\"message\": \"Hi\"}}");
    assert_eq!(Manifest::load(dir.path(), "en").unwrap().name, "Hi");

    // Chrome skips one BOM; a second is not JSON.
    dir.write("manifest.json", &format!("\u{feff}\u{feff}{manifest}"));
    assert!(matches!(Manifest::load(dir.path(), "en"), Err(ManifestError::Json(_))));
}

#[test]
fn tolerant_json() {
    let m = parse(
        "\u{feff}// comment\n{\n  \"manifest_version\": 3, /* block */\n  \"name\": \"A // not a comment\",\n  \"version\": \"1\",\n  \"permissions\": [\"storage\",],\n}\n",
    )
    .unwrap();
    assert_eq!(m.name, "A // not a comment");
    assert_eq!(m.permissions, strs(["storage"]));
}

#[test]
fn content_scripts_web_accessible_dnr_and_ids() {
    let m = parse(
        r#"{"manifest_version": 2, "name": "A", "version": "1",
            "content_scripts": [{"matches": ["<all_urls>"], "exclude_matches": ["https://x.example/*"], "js": ["/c.js"], "css": ["c.css"],
                                 "run_at": "document_start", "all_frames": true, "world": "MAIN"},
                                {"matches": ["https://*/*"], "js": ["d.js"]}],
            "web_accessible_resources": ["img/*.png", "page.html"],
            "declarative_net_request": {"rule_resources": [{"id": "r", "enabled": true, "path": "rules.json"}]},
            "options_ui": {"page": "options.html"},
            "applications": {"gecko": {"id": "old@example.org"}}}"#,
    )
    .unwrap();
    let cs = &m.content_scripts[0];
    assert_eq!((cs.run_at, cs.all_frames, cs.world, cs.match_about_blank), (RunAt::DocumentStart, true, World::Main, false));
    assert_eq!(cs.js[0].as_str(), "c.js");
    assert_eq!(cs.exclude_matches[0].as_str(), "https://x.example/*");
    assert_eq!((m.content_scripts[1].run_at, m.content_scripts[1].world), (RunAt::DocumentIdle, World::Isolated));
    assert_eq!(m.web_accessible_resources.len(), 1);
    assert_eq!(m.web_accessible_resources[0].resources, strs(["img/*.png", "page.html"]));
    assert_eq!(m.web_accessible_resources[0].matches[0].as_str(), "<all_urls>");
    assert_eq!((m.dnr_rulesets[0].id.as_str(), m.dnr_rulesets[0].enabled, m.dnr_rulesets[0].path.as_str()), ("r", true, "rules.json"));
    assert_eq!(m.options_page.unwrap().as_str(), "options.html");
    assert_eq!(m.gecko_id.as_deref(), Some("old@example.org"));

    let newer = parse(
        r#"{"manifest_version": 3, "name": "A", "version": "1", "applications": {"gecko": {"id": "old@x"}},
            "browser_specific_settings": {"gecko": {"id": "new@x"}},
            "web_accessible_resources": [{"resources": ["a.png"], "matches": ["https://*/*"]}, {"resources": ["b.png"], "extension_ids": ["x"]}]}"#,
    )
    .unwrap();
    assert_eq!(newer.gecko_id.as_deref(), Some("new@x"));
    assert_eq!(newer.web_accessible_resources[0].matches[0].as_str(), "https://*/*");
    assert!(newer.web_accessible_resources[1].matches.is_empty());
}

type ErrorCheck = fn(&ManifestError) -> bool;

#[test]
fn invalid_manifests_are_rejected() {
    let cases: &[(&str, ErrorCheck)] = &[
        (r#"{"name": "A", "version": "1"}"#, |e| matches!(e, ManifestError::Field("manifest_version"))),
        (r#"{"manifest_version": 1, "name": "A", "version": "1"}"#, |e| matches!(e, ManifestError::UnsupportedVersion(1))),
        (r#"{"manifest_version": 3, "version": "1"}"#, |e| matches!(e, ManifestError::Field("name"))),
        (r#"{"manifest_version": 3, "name": " ", "version": "1"}"#, |e| matches!(e, ManifestError::Field("name"))),
        (r#"{"manifest_version": 3, "name": "A", "version": "1.02"}"#, |e| matches!(e, ManifestError::Field("version"))),
        (r#"{"manifest_version": 3, "name": "A", "version": "1.2.3.4.5"}"#, |e| matches!(e, ManifestError::Field("version"))),
        (r#"{"manifest_version": 3, "name": "A", "version": "1", "key": "!!"}"#, |e| matches!(e, ManifestError::Field("key"))),
        (r#"{"manifest_version": 3, "name": "A", "version": "1", "content_scripts": [{"matches": ["nope"], "js": ["a.js"]}]}"#, |e| {
            matches!(e, ManifestError::BadPattern(_))
        }),
        (
            r#"{"manifest_version": 3, "name": "A", "version": "1", "content_scripts": [{"matches": ["<all_urls>"], "js": ["../a.js"]}]}"#,
            |e| matches!(e, ManifestError::BadPath(_)),
        ),
        (r#"{"manifest_version": 3, "name": "A", "version": "1", "content_scripts": [{"js": ["a.js"]}]}"#, |e| {
            matches!(e, ManifestError::Field("content_scripts.matches"))
        }),
        (r#"{"manifest_version": 3, "name": "A", "version": "1", "icons": {"big": "a.png"}}"#, |e| {
            matches!(e, ManifestError::Field("icons"))
        }),
        (r#"[1, 2]"#, |e| matches!(e, ManifestError::Json(_))),
    ];
    for (json, expected) in cases {
        match parse(json) {
            Err(e) if expected(&e) => {}
            other => panic!("{json}: unexpected {other:?}"),
        }
    }
}

#[test]
fn the_probe_manifest_parses() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/extensions/probe");
    let m = Manifest::load(&dir, "en").unwrap();
    assert_eq!((m.name.as_str(), m.version.as_str()), ("Vsesvit Probe", "1.0.0"));
    assert!(matches!(&m.background, Some(Background::ServiceWorker { script, module: false }) if script.as_str() == "background.js"));
    assert_eq!(m.content_scripts[0].run_at, RunAt::DocumentEnd);
    assert_eq!(m.action.unwrap().default_popup.unwrap().as_str(), "popup.html");
    assert_eq!(m.dnr_rulesets[0].path.as_str(), "rules.json");
    assert_eq!(m.permissions, strs(["storage", "declarativeNetRequest"]));
    assert_eq!(m.host_permissions[0].as_str(), "<all_urls>");
    assert_eq!(m.key, None);
}
