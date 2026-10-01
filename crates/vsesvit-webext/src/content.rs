//! Content scripts and CSS as WebKit user content.

use std::path::Path;

use vsesvit_core::extensions::manifest::{ContentScript, Manifest, RunAt, World};

use crate::dnr;
use crate::patterns;
use crate::runtime::LoadError;

/// One `UserScript` per `content_scripts` entry (its files concatenated in order, in the
/// extension's world, with the bootstrap in front so `chrome` exists before the first
/// line runs; every entry carries it, since WebKit gives no order between user scripts;
/// a `"world": "MAIN"` entry in the page's world, without either) and one
/// `UserStyleSheet` per entry with CSS.
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
        // WebKit injects a script with no allow list everywhere; this entry matches only
        // what the runtime may not reach (local files).
        if allow.is_empty() {
            continue;
        }
        let block: Vec<String> = entry.exclude_matches.iter().flat_map(|m| patterns::webkit_patterns(m.as_str())).collect();
        let allow: Vec<&str> = allow.iter().map(String::as_str).collect();
        let block: Vec<&str> = block.iter().map(String::as_str).collect();
        let frames = if entry.all_frames {
            webkit::UserContentInjectedFrames::AllFrames
        } else {
            webkit::UserContentInjectedFrames::TopFrame
        };
        if !entry.js.is_empty() {
            // A `"world": "MAIN"` entry shares the page's globals and gets no extension API,
            // as in Chrome (and as `scripting.executeScript` with `world: "MAIN"`).
            let main = entry.world == World::Main;
            let mut source = String::from(if main { "" } else { bootstrap });
            for js in &entry.js {
                let path = js.resolve(dir);
                let text = std::fs::read_to_string(&path).map_err(|e| LoadError::Io { path: path.clone(), source: e })?;
                source.push_str("\n;\n");
                source.push_str(&text);
            }
            let time = injection_time(entry);
            scripts.push(if main {
                webkit::UserScript::new(&source, frames, time, &allow, &block)
            } else {
                webkit::UserScript::for_world(&source, frames, time, world, &allow, &block)
            });
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
/// extension declares none, lacks the permission for them (see [`dnr::Grants`]), or every
/// rule was inexpressible.
pub(crate) fn dnr_json(dir: &Path, manifest: &Manifest, base_url: &str) -> Result<Option<String>, LoadError> {
    let host_permissions: Vec<String> = manifest.host_permissions.iter().map(|p| p.as_str().to_owned()).collect();
    let Some(grants) = dnr::Grants::from_manifest(&manifest.permissions, &host_permissions) else {
        if manifest.dnr_rulesets.iter().any(|r| r.enabled) {
            log::warn!("{}: declarativeNetRequest rulesets ignored without the declarativeNetRequest permission", manifest.name);
        }
        return Ok(None);
    };
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
    let translation = dnr::translate(&rules, base_url.trim_end_matches('/'), &grants);
    if !translation.skipped.is_empty() {
        log::warn!("{}: declarativeNetRequest rules WebKit cannot express: {}", manifest.name, dnr::describe_skipped(&translation.skipped));
    }
    Ok((!translation.is_empty()).then(|| translation.to_json()))
}
