//! Content scripts and CSS as WebKit user content, and the shim bootstrap source.

use std::path::Path;

use vsesvit_core::extensions::manifest::{ContentScript, Manifest, RunAt};

use crate::dnr;
use crate::patterns;
use crate::runtime::LoadError;

/// `api.js` with the per-context configuration prepended.
pub(crate) fn bootstrap(config: &serde_json::Value) -> String {
    format!("const __VSESVIT_CONFIG__ = {};\n{}", config, crate::API_JS)
}

/// One `UserScript` per `content_scripts` entry (its files concatenated in order, in the
/// extension's world, with the bootstrap in front so `chrome` exists before the first
/// line runs) and one `UserStyleSheet` per entry with CSS.
pub(crate) fn user_content(
    dir: &Path,
    manifest: &Manifest,
    world: &str,
    bootstrap: &str,
) -> Result<(Vec<webkit::UserScript>, Vec<webkit::UserStyleSheet>), LoadError> {
    let mut scripts = Vec::new();
    let mut styles = Vec::new();
    for entry in &manifest.content_scripts {
        let allow: Vec<String> = entry.matches.iter().flat_map(|m| patterns::webkit_patterns(m.as_str())).collect();
        let block: Vec<String> = entry.exclude_matches.iter().flat_map(|m| patterns::webkit_patterns(m.as_str())).collect();
        let allow: Vec<&str> = allow.iter().map(String::as_str).collect();
        let block: Vec<&str> = block.iter().map(String::as_str).collect();
        let frames = if entry.all_frames {
            webkit::UserContentInjectedFrames::AllFrames
        } else {
            webkit::UserContentInjectedFrames::TopFrame
        };
        if !entry.js.is_empty() {
            let mut source = String::from(bootstrap);
            for js in &entry.js {
                let path = js.resolve(dir);
                let text = std::fs::read_to_string(&path).map_err(|e| LoadError::Io { path: path.clone(), source: e })?;
                source.push_str("\n;\n");
                source.push_str(&text);
            }
            scripts.push(webkit::UserScript::for_world(&source, frames, injection_time(entry), world, &allow, &block));
        }
        if !entry.css.is_empty() {
            let mut css = String::new();
            for file in &entry.css {
                let path = file.resolve(dir);
                let text = std::fs::read_to_string(&path).map_err(|e| LoadError::Io { path: path.clone(), source: e })?;
                css.push_str(&text);
                css.push('\n');
            }
            styles.push(webkit::UserStyleSheet::for_world(&css, frames, webkit::UserStyleLevel::Author, world, &allow, &block));
        }
    }
    Ok((scripts, styles))
}

/// `document_idle` has no WebKit equivalent; document end is the closest (GNOME Web
/// makes the same choice).
fn injection_time(entry: &ContentScript) -> webkit::UserScriptInjectionTime {
    match entry.run_at {
        RunAt::DocumentStart => webkit::UserScriptInjectionTime::Start,
        RunAt::DocumentEnd | RunAt::DocumentIdle => webkit::UserScriptInjectionTime::End,
    }
}

/// The merged content-blocker JSON of every enabled static ruleset, or `None` when the
/// extension declares none (or every rule was inexpressible).
pub(crate) fn dnr_json(dir: &Path, manifest: &Manifest, base_url: &str) -> Result<Option<String>, LoadError> {
    let mut rules = Vec::new();
    for ruleset in manifest.dnr_rulesets.iter().filter(|r| r.enabled) {
        let path = ruleset.path.resolve(dir);
        let text = std::fs::read_to_string(&path).map_err(|e| LoadError::Io { path: path.clone(), source: e })?;
        let (parsed, malformed) = dnr::parse_rules(&text).map_err(|e| LoadError::Ruleset { path: path.clone(), reason: e.to_string() })?;
        if !malformed.is_empty() {
            log::warn!("{}: skipped malformed rules: {}", path.display(), dnr::describe_skipped(&malformed));
        }
        rules.extend(parsed);
    }
    if rules.is_empty() {
        return Ok(None);
    }
    let translation = dnr::translate(&rules, base_url.trim_end_matches('/'));
    if !translation.skipped.is_empty() {
        log::warn!("{}: declarativeNetRequest rules WebKit cannot express: {}", manifest.name, dnr::describe_skipped(&translation.skipped));
    }
    Ok((!translation.is_empty()).then(|| translation.to_json()))
}
