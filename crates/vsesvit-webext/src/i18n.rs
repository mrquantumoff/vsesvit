//! `chrome.i18n` backing: the message catalog of `_locales/<locale>/messages.json`,
//! resolved once per extension and embedded into the shim. Substitution
//! (`$PLACEHOLDER$`, `$1`) happens in JavaScript.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use vsesvit_core::extensions::manifest::{ManifestError, locale_chain, parse_tolerant_json, read_text};

/// The catalog the shim receives: lowercase message name to
/// `{"message": .., "placeholders": {..}}`.
pub type Catalog = BTreeMap<String, Value>;

/// Load the catalog for `ui` from `dir/_locales`, more specific locales overriding the
/// default one, read as core reads them for the manifest (BOM, comments and trailing
/// commas tolerated). A missing `_locales` dir yields an empty catalog; an unparsable
/// file is skipped with a warning.
pub fn load_catalog(dir: &Path, ui: &str, default_locale: Option<&str>) -> Catalog {
    let mut catalog = Catalog::new();
    for locale in locale_chain(ui, default_locale).iter().rev() {
        let path = dir.join("_locales").join(locale).join("messages.json");
        let Ok(text) = read_text(&path) else { continue };
        match parse_messages(&text) {
            Ok(messages) => catalog.extend(messages),
            Err(e) => log::warn!("{}: {e}", path.display()),
        }
    }
    catalog
}

pub fn parse_messages(text: &str) -> Result<Catalog, ManifestError> {
    let value = parse_tolerant_json(text)?;
    let mut out = Catalog::new();
    if let Value::Object(map) = value {
        for (k, v) in map {
            if v.get("message").is_some() {
                out.insert(k.to_lowercase(), v);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerant_messages() {
        let text = r#"{
          // comment
          "Greeting": { "message": "Hello $NAME$", "placeholders": { "name": { "content": "$1" } } }, /* x */
          "Plain": { "message": "a\"b // not a comment" },
          "junk": 1,
        }"#;
        let c = parse_messages(text).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c["greeting"]["message"], "Hello $NAME$");
        assert_eq!(c["plain"]["message"], "a\"b // not a comment");
        assert!(parse_messages("[1,").is_err());
    }

    #[test]
    fn catalog_from_dir() {
        let dir = std::env::temp_dir().join(format!("vsesvit-i18n-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("_locales/en")).unwrap();
        std::fs::create_dir_all(dir.join("_locales/en_US")).unwrap();
        std::fs::write(dir.join("_locales/en/messages.json"), r#"{"a": {"message": "en"}, "b": {"message": "en-b"}}"#).unwrap();
        std::fs::write(dir.join("_locales/en_US/messages.json"), r#"{"a": {"message": "en_US"}}"#).unwrap();
        let c = load_catalog(&dir, "en_US", Some("en"));
        assert_eq!(c["a"]["message"], "en_US");
        assert_eq!(c["b"]["message"], "en-b");
        assert!(load_catalog(&dir.join("missing"), "en", None).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_ascii_messages_and_bom() {
        let c = parse_messages("{\"hi\": {\"message\": \"Привіт — ok\"}}").unwrap();
        assert_eq!(c["hi"]["message"], "Привіт — ok");
        let dir = std::env::temp_dir().join(format!("vsesvit-i18n-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("_locales/uk")).unwrap();
        std::fs::write(dir.join("_locales/uk/messages.json"), "\u{feff}{\"hi\": {\"message\": \"Привіт\"}}").unwrap();
        assert_eq!(load_catalog(&dir, "uk_UA", Some("en"))["hi"]["message"], "Привіт");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trailing_comma_before_comment() {
        for text in ["{\"last\": {\"message\": \"x\"}, // keep sorted\n}", "{\"last\": {\"message\": \"x\"}, /* c */ }"] {
            let c = parse_messages(text).unwrap();
            assert_eq!(c.len(), 1);
            assert_eq!(c["last"]["message"], "x");
        }
    }

    #[test]
    fn default_locale_cannot_escape_the_extension_dir() {
        let root = std::env::temp_dir().join(format!("vsesvit-i18n-escape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ext/_locales")).unwrap();
        std::fs::create_dir_all(root.join("x")).unwrap();
        std::fs::write(root.join("x/messages.json"), r#"{"n": {"message": "escaped"}}"#).unwrap();
        assert!(load_catalog(&root.join("ext"), "en", Some("../../x")).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
