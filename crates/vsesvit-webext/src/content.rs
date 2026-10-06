//! Content scripts and CSS as WebKit user content.

use std::path::Path;

use vsesvit_core::extensions::manifest::{ContentScript, RunAt, World};

use crate::patterns;
use crate::runtime::LoadError;

/// The user scripts and style sheets of some content scripts, which every tab's
/// `UserContentManager` gets.
#[derive(Default)]
pub(crate) struct UserContent {
    scripts: Vec<webkit::UserScript>,
    styles: Vec<webkit::UserStyleSheet>,
}

impl UserContent {
    /// One `UserScript` per entry (its files concatenated in order, in the extension's
    /// world, with the bootstrap in front so `chrome` exists before the first line runs;
    /// every entry carries it, since WebKit gives no order between user scripts; a
    /// `"world": "MAIN"` entry in the page's world, without either) and one
    /// `UserStyleSheet` per entry with CSS.
    pub fn build(dir: &Path, entries: &[ContentScript], world: &str, bootstrap: &str) -> Result<UserContent, LoadError> {
        let mut content = UserContent::default();
        for entry in entries {
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
                content.scripts.push(if main {
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
                // At the user level, since WebKitGTK (2.52) applies an author-level user style
                // sheet to quirks-mode documents only. A page's own rules win over these unless
                // they are `!important`, where Chrome would weigh them as the page's equals.
                content.styles.push(webkit::UserStyleSheet::for_world(&css, frames, webkit::UserStyleLevel::User, world, &allow, &block));
            }
        }
        Ok(content)
    }

    pub fn add_to(&self, ucm: &webkit::UserContentManager) {
        for script in &self.scripts {
            ucm.add_script(script);
        }
        for style in &self.styles {
            ucm.add_style_sheet(style);
        }
    }

    pub fn remove_from(&self, ucm: &webkit::UserContentManager) {
        for script in &self.scripts {
            ucm.remove_script(script);
        }
        for style in &self.styles {
            ucm.remove_style_sheet(style);
        }
    }
}

/// `document_idle` has no WebKit equivalent; document end is the closest (GNOME Web
/// makes the same choice).
fn injection_time(entry: &ContentScript) -> webkit::UserScriptInjectionTime {
    match entry.run_at {
        RunAt::DocumentStart => webkit::UserScriptInjectionTime::Start,
        RunAt::DocumentEnd | RunAt::DocumentIdle => webkit::UserScriptInjectionTime::End,
    }
}
