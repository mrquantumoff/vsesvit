//! `chrome.i18n` backing: the message catalog of `_locales/<locale>/messages.json`,
//! resolved once per extension and embedded into the shim. Substitution
//! (`$PLACEHOLDER$`, `$1`) happens in JavaScript.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// The catalog the shim receives: lowercase message name to
/// `{"message": .., "placeholders": {..}}`.
pub type Catalog = BTreeMap<String, Value>;

/// The UI locale from the environment (`LC_ALL`, `LC_MESSAGES`, `LANG`), as Chrome
/// spells it: `en_US`. Falls back to `en`.
pub fn ui_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|v| normalize_locale(&v))
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_else(|| "en".to_owned())
}

/// `en_US.UTF-8` -> `en_US`, `pt-BR` -> `pt_BR`.
pub fn normalize_locale(raw: &str) -> String {
    let base = raw.split(['.', '@']).next().unwrap_or(raw);
    base.replace('-', "_")
}

/// Candidate locale directories in lookup order: `en_US`, `en`, then the default locale.
pub fn locale_chain(ui: &str, default_locale: Option<&str>) -> Vec<String> {
    let mut chain = vec![ui.to_owned()];
    if let Some((lang, _)) = ui.split_once('_') {
        chain.push(lang.to_owned());
    }
    if let Some(d) = default_locale
        && !chain.iter().any(|c| c == d)
    {
        chain.push(d.to_owned());
    }
    chain
}

/// Load the catalog for `ui` from `dir/_locales`, more specific locales overriding the
/// default one. A missing `_locales` dir yields an empty catalog; an unreadable file is
/// skipped with a warning, since Chrome tolerates comments and trailing commas there.
pub fn load_catalog(dir: &Path, ui: &str, default_locale: Option<&str>) -> Catalog {
    let mut catalog = Catalog::new();
    for locale in locale_chain(ui, default_locale).iter().rev() {
        let path = dir.join("_locales").join(locale).join("messages.json");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        match parse_messages(&text) {
            Ok(messages) => catalog.extend(messages),
            Err(e) => log::warn!("{}: {e}", path.display()),
        }
    }
    catalog
}

pub fn parse_messages(text: &str) -> Result<Catalog, serde_json::Error> {
    let value: Value = serde_json::from_str(&strip_json_comments(text))?;
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

/// Remove `//` and `/* */` comments and trailing commas, outside string literals.
pub fn strip_json_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            out.push(c as char);
            if c == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if !matches!(bytes.get(j), Some(b'}') | Some(b']')) {
                    out.push(',');
                }
                i += 1;
            }
            _ => {
                // Multi-byte UTF-8 only occurs inside strings in valid JSON, but be safe.
                let ch_len = utf8_len(c);
                out.push_str(&text[i..i + ch_len]);
                i += ch_len;
            }
        }
    }
    out
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales() {
        assert_eq!(normalize_locale("en_US.UTF-8"), "en_US");
        assert_eq!(normalize_locale("pt-BR"), "pt_BR");
        assert_eq!(normalize_locale("de_DE@euro"), "de_DE");
        assert_eq!(normalize_locale("C.UTF-8"), "C");
        assert_eq!(locale_chain("en_US", Some("de")), ["en_US", "en", "de"]);
        assert_eq!(locale_chain("en", Some("en")), ["en"]);
        assert_eq!(locale_chain("fr", None), ["fr"]);
    }

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
}
