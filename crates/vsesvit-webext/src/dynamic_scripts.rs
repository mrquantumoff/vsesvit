//! One extension's dynamic content scripts, which `chrome.scripting.registerContentScripts`
//! adds and `getRegisteredContentScripts`, `updateContentScripts` and
//! `unregisterContentScripts` list, change and remove, with Chrome's checks and errors.
//! Pure; the runtime keeps [`Scripts::saved`] on disk (the scripts registered to persist
//! across sessions outlive a restart, not an update) and injects
//! [`Scripts::content_scripts`] beside the manifest's.

use serde::Deserialize;
use serde_json::{Value, json};
use vsesvit_core::extensions::manifest::{ContentScript, MatchPattern, RelPath, RunAt, World};

/// A dynamic content script as the extension registered it.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub id: String,
    pub matches: Vec<MatchPattern>,
    pub exclude_matches: Vec<MatchPattern>,
    pub js: Vec<RelPath>,
    pub css: Vec<RelPath>,
    pub all_frames: bool,
    /// Kept and reported, but WebKit cannot match a frame by the origin that created it.
    pub match_origin_as_fallback: bool,
    pub run_at: RunAt,
    pub world: World,
    pub persist: bool,
}

/// `scripting.RegisteredContentScript` as a call gives it. Only `id` is required, since an
/// update names just what changes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Given {
    id: String,
    matches: Option<Vec<String>>,
    exclude_matches: Option<Vec<String>>,
    js: Option<Vec<String>>,
    css: Option<Vec<String>>,
    all_frames: Option<bool>,
    match_origin_as_fallback: Option<bool>,
    run_at: Option<String>,
    world: Option<String>,
    persist_across_sessions: Option<bool>,
}

/// A script being registered or updated, before its patterns and files are checked.
struct Draft {
    id: String,
    matches: Vec<String>,
    exclude_matches: Vec<String>,
    js: Vec<String>,
    css: Vec<String>,
    all_frames: bool,
    match_origin_as_fallback: bool,
    run_at: RunAt,
    world: World,
    persist: bool,
}

/// Where a call's file references lead: the file inside the extension, when it can be read.
pub type Files<'a> = &'a dyn Fn(&str) -> Option<RelPath>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scripts {
    scripts: Vec<Script>,
}

impl Scripts {
    /// The scripts [`Scripts::saved`] left. One that no longer checks out (a file the
    /// extension dropped) is left out and reported.
    pub fn restore(saved: &[Value], files: Files) -> (Scripts, Vec<String>) {
        let mut scripts = Scripts::default();
        let mut skipped = Vec::new();
        for script in saved {
            if let Err(e) = scripts.register(&json!([script]), files) {
                skipped.push(e);
            }
        }
        (scripts, skipped)
    }

    /// The scripts that persist across sessions, as `getRegisteredContentScripts` lists them.
    pub fn saved(&self) -> Vec<Value> {
        self.scripts.iter().filter(|s| s.persist).map(Script::to_json).collect()
    }

    /// `registerContentScripts(scripts)`: all of them, or none on an error.
    pub fn register(&mut self, scripts: &Value, files: Files) -> Result<(), String> {
        let given = given_list(scripts, "registerContentScripts")?;
        check_ids(&given, |id| self.scripts.iter().any(|s| s.id == id))?;
        let mut drafts = Vec::new();
        for given in given {
            if given.matches.is_none() {
                return Err(format!("Script with ID '{}' must specify 'matches'.", given.id));
            }
            let mut draft = Draft::new(given.id.clone());
            draft.apply(given)?;
            drafts.push(draft);
        }
        let added = resolve(drafts, files)?;
        self.scripts.extend(added);
        Ok(())
    }

    /// `updateContentScripts(scripts)`: what each names replaces that part of the registered
    /// script, and what it leaves out stays, `persistAcrossSessions` included, as Chrome
    /// documents it. All of them, or none on an error.
    pub fn update(&mut self, scripts: &Value, files: Files) -> Result<(), String> {
        let given = given_list(scripts, "updateContentScripts")?;
        check_ids(&given, |_| false)?;
        let mut drafts = Vec::new();
        for given in given {
            let Some(script) = self.scripts.iter().find(|s| s.id == given.id) else {
                return Err(format!("Script with ID '{}' does not exist or is not fully registered", given.id));
            };
            let mut draft = Draft::of(script);
            draft.apply(given)?;
            drafts.push(draft);
        }
        for updated in resolve(drafts, files)? {
            if let Some(script) = self.scripts.iter_mut().find(|s| s.id == updated.id) {
                *script = updated;
            }
        }
        Ok(())
    }

    /// `unregisterContentScripts(filter)`: those `filter.ids` names, else every one. As in
    /// Chrome, an empty `ids` list counts as none and removes every script.
    pub fn unregister(&mut self, filter: &Value) -> Result<(), String> {
        let ids = filter_ids(filter, "unregisterContentScripts")?;
        let Some(ids) = ids.filter(|ids| !ids.is_empty()) else {
            self.scripts.clear();
            return Ok(());
        };
        for id in &ids {
            check_id(id)?;
            if !self.scripts.iter().any(|s| s.id == *id) {
                return Err(format!("Nonexistent script ID '{id}'"));
            }
        }
        self.scripts.retain(|s| !ids.contains(&s.id));
        Ok(())
    }

    /// `getRegisteredContentScripts(filter)`: those `filter.ids` names, else every one (an
    /// empty list counts as none, as in Chrome).
    pub fn get(&self, filter: &Value) -> Result<Vec<Value>, String> {
        let ids = filter_ids(filter, "getRegisteredContentScripts")?.filter(|ids| !ids.is_empty());
        Ok(self.scripts.iter().filter(|s| ids.as_ref().is_none_or(|ids| ids.contains(&s.id))).map(Script::to_json).collect())
    }

    /// The scripts to inject, as manifest entries. Chrome injects a dynamic script only
    /// where the extension has host permissions, so each one's `matches` are narrowed to
    /// `host_permissions`, and one left with none is out.
    pub fn content_scripts(&self, host_permissions: &[MatchPattern]) -> Vec<ContentScript> {
        self.scripts
            .iter()
            .filter_map(|s| {
                let mut matches: Vec<MatchPattern> = Vec::new();
                for granted in s.matches.iter().flat_map(|m| host_permissions.iter().filter_map(|h| m.within(h))) {
                    if !matches.contains(&granted) {
                        matches.push(granted);
                    }
                }
                (!matches.is_empty()).then(|| ContentScript {
                    matches,
                    exclude_matches: s.exclude_matches.clone(),
                    js: s.js.clone(),
                    css: s.css.clone(),
                    run_at: s.run_at,
                    all_frames: s.all_frames,
                    match_about_blank: false,
                    world: s.world,
                })
            })
            .collect()
    }
}

impl Script {
    /// `scripting.RegisteredContentScript`, as Chrome reports it.
    fn to_json(&self) -> Value {
        let patterns = |list: &[MatchPattern]| json!(list.iter().map(MatchPattern::as_str).collect::<Vec<_>>());
        let files = |list: &[RelPath]| json!(list.iter().map(RelPath::as_str).collect::<Vec<_>>());
        let mut v = json!({
            "id": self.id,
            "matches": patterns(&self.matches),
            "allFrames": self.all_frames,
            "matchOriginAsFallback": self.match_origin_as_fallback,
            "runAt": run_at_name(self.run_at),
            "world": world_name(self.world),
            "persistAcrossSessions": self.persist,
        });
        if !self.exclude_matches.is_empty() {
            v["excludeMatches"] = patterns(&self.exclude_matches);
        }
        if !self.js.is_empty() {
            v["js"] = files(&self.js);
        }
        if !self.css.is_empty() {
            v["css"] = files(&self.css);
        }
        v
    }
}

impl Draft {
    fn new(id: String) -> Draft {
        Draft {
            id,
            matches: Vec::new(),
            exclude_matches: Vec::new(),
            js: Vec::new(),
            css: Vec::new(),
            all_frames: false,
            match_origin_as_fallback: false,
            run_at: RunAt::DocumentIdle,
            world: World::Isolated,
            persist: true,
        }
    }

    fn of(script: &Script) -> Draft {
        let patterns = |list: &[MatchPattern]| list.iter().map(|p| p.as_str().to_owned()).collect();
        let files = |list: &[RelPath]| list.iter().map(|p| p.as_str().to_owned()).collect();
        Draft {
            id: script.id.clone(),
            matches: patterns(&script.matches),
            exclude_matches: patterns(&script.exclude_matches),
            js: files(&script.js),
            css: files(&script.css),
            all_frames: script.all_frames,
            match_origin_as_fallback: script.match_origin_as_fallback,
            run_at: script.run_at,
            world: script.world,
            persist: script.persist,
        }
    }

    fn apply(&mut self, given: Given) -> Result<(), String> {
        let invalid = |property: &str, values: &str| format!("Error at property '{property}': Value must be one of {values}.");
        if let Some(name) = given.run_at {
            self.run_at = match name.as_str() {
                "document_start" => RunAt::DocumentStart,
                "document_end" => RunAt::DocumentEnd,
                "document_idle" => RunAt::DocumentIdle,
                _ => return Err(invalid("runAt", "document_end, document_idle, document_start")),
            };
        }
        if let Some(name) = given.world {
            self.world = match name.as_str() {
                "ISOLATED" => World::Isolated,
                "MAIN" => World::Main,
                _ => return Err(invalid("world", "ISOLATED, MAIN")),
            };
        }
        self.matches = given.matches.unwrap_or(std::mem::take(&mut self.matches));
        self.exclude_matches = given.exclude_matches.unwrap_or(std::mem::take(&mut self.exclude_matches));
        self.js = given.js.unwrap_or(std::mem::take(&mut self.js));
        self.css = given.css.unwrap_or(std::mem::take(&mut self.css));
        self.all_frames = given.all_frames.unwrap_or(self.all_frames);
        self.match_origin_as_fallback = given.match_origin_as_fallback.unwrap_or(self.match_origin_as_fallback);
        self.persist = given.persist_across_sessions.unwrap_or(self.persist);
        Ok(())
    }

    /// The checks Chrome makes as it parses a script, in its order.
    fn check(&self) -> Result<(Vec<MatchPattern>, Vec<MatchPattern>), String> {
        let id = &self.id;
        if self.js.is_empty() && self.css.is_empty() {
            return Err(format!("Script with ID '{id}' must specify at least one js or css file."));
        }
        if self.matches.is_empty() {
            return Err(format!("Script with ID '{id}' must specify at least one match."));
        }
        let parse = |list: &[String], field: &str| -> Result<Vec<MatchPattern>, String> {
            list.iter()
                .enumerate()
                .map(|(i, p)| MatchPattern::parse(p).map_err(|_| format!("Script with ID '{id}' has invalid value for {field}[{i}]: {}", pattern_error(p))))
                .collect()
        };
        Ok((parse(&self.matches, "matches")?, parse(&self.exclude_matches, "exclude_matches")?))
    }
}

/// Every draft checked, then every file found, as Chrome reads the files only once all the
/// scripts parse.
fn resolve(drafts: Vec<Draft>, files: Files) -> Result<Vec<Script>, String> {
    let patterns = drafts.iter().map(Draft::check).collect::<Result<Vec<_>, _>>()?;
    drafts
        .into_iter()
        .zip(patterns)
        .map(|(draft, (matches, exclude_matches))| {
            let find = |list: &[String], kind: &str| -> Result<Vec<RelPath>, String> {
                list.iter().map(|f| files(f).ok_or_else(|| format!("Could not load {kind} '{}' for script.", f.trim_start_matches('/')))).collect()
            };
            Ok(Script {
                js: find(&draft.js, "javascript")?,
                css: find(&draft.css, "css")?,
                id: draft.id,
                matches,
                exclude_matches,
                all_frames: draft.all_frames,
                match_origin_as_fallback: draft.match_origin_as_fallback,
                run_at: draft.run_at,
                world: draft.world,
                persist: draft.persist,
            })
        })
        .collect()
}

fn given_list(scripts: &Value, method: &str) -> Result<Vec<Given>, String> {
    let list = scripts.as_array().ok_or_else(|| format!("Error in invocation of scripting.{method}: scripts must be an array."))?;
    list.iter()
        .enumerate()
        .map(|(i, script)| serde_json::from_value(script.clone()).map_err(|e| format!("Error in invocation of scripting.{method}: Error at parameter 'scripts': Error at index {i}: {e}.")))
        .collect()
}

fn filter_ids(filter: &Value, method: &str) -> Result<Option<Vec<String>>, String> {
    match &filter["ids"] {
        Value::Null => Ok(None),
        ids => serde_json::from_value(ids.clone()).map(Some).map_err(|_| format!("Error in invocation of scripting.{method}: filter.ids must be a list of strings.")),
    }
}

fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("Script's ID must not be empty".into());
    }
    if id.starts_with('_') {
        return Err(format!("Script's ID '{id}' must not start with '_'"));
    }
    Ok(())
}

/// Each id valid and named once, and none taken already.
fn check_ids(given: &[Given], taken: impl Fn(&str) -> bool) -> Result<(), String> {
    for (i, script) in given.iter().enumerate() {
        check_id(&script.id)?;
        if taken(&script.id) || given[..i].iter().any(|s| s.id == script.id) {
            return Err(format!("Duplicate script ID '{}'", script.id));
        }
    }
    Ok(())
}

/// Why Chrome's pattern parser refuses `pattern`, in its words.
fn pattern_error(pattern: &str) -> &'static str {
    let Some((scheme, rest)) = pattern.split_once("://") else { return "Missing scheme separator." };
    if !matches!(scheme, "*" | "http" | "https" | "ws" | "wss" | "ftp" | "file" | "urn") {
        return "Invalid scheme.";
    }
    let Some(slash) = rest.find('/') else { return "Empty path." };
    let host = &rest[..slash];
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    if name.is_empty() && scheme != "file" {
        return "Host can not be empty.";
    }
    if name.contains('*') {
        return "Invalid host wildcard.";
    }
    if host.rsplit_once(':').is_some_and(|(_, port)| port != "*" && port.parse::<u16>().is_err()) {
        return "Invalid port.";
    }
    "Invalid host."
}

fn run_at_name(run_at: RunAt) -> &'static str {
    match run_at {
        RunAt::DocumentStart => "document_start",
        RunAt::DocumentEnd => "document_end",
        RunAt::DocumentIdle => "document_idle",
    }
}

fn world_name(world: World) -> &'static str {
    match world {
        World::Isolated => "ISOLATED",
        World::Main => "MAIN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(path: &str) -> Option<RelPath> {
        let path = path.trim_start_matches('/');
        ["a.js", "b.js", "a.css"].contains(&path).then(|| RelPath::parse(path).unwrap())
    }

    fn script(id: &str) -> Value {
        json!({ "id": id, "matches": ["https://*.example.com/*"], "js": ["a.js"] })
    }

    fn ids(scripts: &Scripts) -> Vec<String> {
        scripts.get(&Value::Null).unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn registered_scripts_are_listed_as_chrome_lists_them() {
        let mut s = Scripts::default();
        s.register(&json!([script("plain"), { "id": "full", "matches": ["<all_urls>"], "excludeMatches": ["*://*/private/*"], "js": ["/b.js"], "css": ["a.css"], "allFrames": true, "runAt": "document_start", "world": "MAIN", "persistAcrossSessions": false }]), &files).unwrap();
        let listed = s.get(&Value::Null).unwrap();
        assert_eq!(
            listed[0],
            json!({ "id": "plain", "matches": ["https://*.example.com/*"], "js": ["a.js"], "allFrames": false, "matchOriginAsFallback": false, "runAt": "document_idle", "world": "ISOLATED", "persistAcrossSessions": true })
        );
        assert_eq!(
            listed[1],
            json!({ "id": "full", "matches": ["<all_urls>"], "excludeMatches": ["*://*/private/*"], "js": ["b.js"], "css": ["a.css"], "allFrames": true, "matchOriginAsFallback": false, "runAt": "document_start", "world": "MAIN", "persistAcrossSessions": false })
        );
        assert_eq!(s.get(&json!({ "ids": ["full", "nope"] })).unwrap(), [listed[1].clone()]);
        assert_eq!(s.get(&json!({ "ids": [] })).unwrap().len(), 2, "an empty filter is no filter, as in Chrome");
    }

    #[test]
    fn a_failed_registration_registers_nothing_and_says_why_as_chrome_does() {
        let mut s = Scripts::default();
        s.register(&json!([script("taken")]), &files).unwrap();
        let refused = |scripts: Value| Scripts::register(&mut s.clone(), &scripts, &files).unwrap_err();
        assert_eq!(refused(json!([script("")])), "Script's ID must not be empty");
        assert_eq!(refused(json!([script("_x")])), "Script's ID '_x' must not start with '_'");
        assert_eq!(refused(json!([script("taken")])), "Duplicate script ID 'taken'");
        assert_eq!(refused(json!([script("new"), script("new")])), "Duplicate script ID 'new'");
        assert_eq!(refused(json!([{ "id": "a", "js": ["a.js"] }])), "Script with ID 'a' must specify 'matches'.");
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>"] }])), "Script with ID 'a' must specify at least one js or css file.");
        assert_eq!(refused(json!([{ "id": "a", "matches": [], "css": ["a.css"] }])), "Script with ID 'a' must specify at least one match.");
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>", "example.com"], "js": ["a.js"] }])), "Script with ID 'a' has invalid value for matches[1]: Missing scheme separator.");
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>"], "excludeMatches": ["https://*foo.com/*"], "js": ["a.js"] }])), "Script with ID 'a' has invalid value for exclude_matches[0]: Invalid host wildcard.");
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>"], "js": ["a.js", "/gone.js"] }])), "Could not load javascript 'gone.js' for script.");
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>"], "css": ["../a.css"] }])), "Could not load css '../a.css' for script.");
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>"], "js": ["a.js"], "runAt": "later" }])), "Error at property 'runAt': Value must be one of document_end, document_idle, document_start.");
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>"], "js": ["a.js"], "world": "USER_SCRIPT" }])), "Error at property 'world': Value must be one of ISOLATED, MAIN.");
        assert!(refused(json!([{ "id": "a", "matches": ["<all_urls>"], "js": ["a.js"], "colour": "red" }])).contains("Error at index 0"));
        assert!(refused(json!({ "id": "a" })).contains("must be an array"));
        // Every script parses before any file is looked for.
        assert_eq!(refused(json!([{ "id": "a", "matches": ["<all_urls>"], "js": ["gone.js"] }, { "id": "b", "matches": ["nope"], "js": ["a.js"] }])), "Script with ID 'b' has invalid value for matches[0]: Missing scheme separator.");
        assert_eq!(refused(json!([script("fine"), { "id": "bad", "matches": ["<all_urls>"], "js": ["gone.js"] }])), "Could not load javascript 'gone.js' for script.");
        assert_eq!(ids(&s), ["taken"]);
    }

    #[test]
    fn an_update_changes_what_it_names_and_keeps_the_rest() {
        let mut s = Scripts::default();
        s.register(&json!([script("a"), { "id": "b", "matches": ["<all_urls>"], "css": ["a.css"], "persistAcrossSessions": false, "runAt": "document_end" }]), &files).unwrap();
        s.update(&json!([{ "id": "b", "js": ["b.js"], "css": [], "allFrames": true }]), &files).unwrap();
        let b = &s.get(&json!({ "ids": ["b"] })).unwrap()[0];
        assert_eq!(b, &json!({ "id": "b", "matches": ["<all_urls>"], "js": ["b.js"], "allFrames": true, "matchOriginAsFallback": false, "runAt": "document_end", "world": "ISOLATED", "persistAcrossSessions": false }));
        assert_eq!(s.update(&json!([{ "id": "nope", "js": ["a.js"] }]), &files), Err("Script with ID 'nope' does not exist or is not fully registered".into()));
        assert_eq!(s.update(&json!([{ "id": "a", "js": ["b.js"] }, { "id": "b", "js": [] }]), &files), Err("Script with ID 'b' must specify at least one js or css file.".into()));
        assert_eq!(s.update(&json!([{ "id": "a" }, { "id": "a" }]), &files), Err("Duplicate script ID 'a'".into()));
        assert_eq!(s.get(&json!({ "ids": ["a"] })).unwrap()[0]["js"], json!(["a.js"]), "a failed update changes nothing");
        s.update(&json!([{ "id": "b", "persistAcrossSessions": true }]), &files).unwrap();
        assert_eq!(s.saved().len(), 2);
        assert_eq!(ids(&s), ["a", "b"], "an update keeps the order");
    }

    #[test]
    fn unregistering_removes_the_named_scripts_or_every_one() {
        let mut s = Scripts::default();
        s.register(&json!([script("a"), script("b"), script("c")]), &files).unwrap();
        assert_eq!(s.unregister(&json!({ "ids": ["a", "nope"] })), Err("Nonexistent script ID 'nope'".into()));
        assert_eq!(s.unregister(&json!({ "ids": ["_a"] })), Err("Script's ID '_a' must not start with '_'".into()));
        assert_eq!(ids(&s), ["a", "b", "c"]);
        s.unregister(&json!({ "ids": ["b"] })).unwrap();
        assert_eq!(ids(&s), ["a", "c"]);
        s.unregister(&json!({ "ids": [] })).unwrap();
        assert!(ids(&s).is_empty(), "an empty list removes every script, as in Chrome");
        s.register(&json!([script("a")]), &files).unwrap();
        s.unregister(&Value::Null).unwrap();
        assert!(ids(&s).is_empty());
    }

    #[test]
    fn only_scripts_that_persist_are_saved_and_a_restore_drops_what_no_longer_loads() {
        let mut s = Scripts::default();
        s.register(&json!([script("kept"), { "id": "session", "matches": ["<all_urls>"], "js": ["a.js"], "persistAcrossSessions": false }, { "id": "gone", "matches": ["<all_urls>"], "js": ["b.js"] }]), &files).unwrap();
        let saved = s.saved();
        assert_eq!(saved.iter().map(|v| v["id"].clone()).collect::<Vec<_>>(), [json!("kept"), json!("gone")]);
        let without_b = |path: &str| files(path).filter(|p| p.as_str() != "b.js");
        let (restored, skipped) = Scripts::restore(&saved, &without_b);
        assert_eq!(ids(&restored), ["kept"]);
        assert_eq!(skipped, ["Could not load javascript 'b.js' for script."]);
        assert_eq!(restored.get(&Value::Null).unwrap()[0], s.get(&json!({ "ids": ["kept"] })).unwrap()[0]);
    }

    #[test]
    fn scripts_are_injected_only_where_the_extension_has_host_permissions() {
        let mut s = Scripts::default();
        s.register(&json!([{ "id": "all", "matches": ["<all_urls>", "*://*.example.com/*"], "excludeMatches": ["*://*/private/*"], "js": ["a.js"] }, script("elsewhere")]), &files).unwrap();
        let hosts = [MatchPattern::parse("http://127.0.0.1/*").unwrap(), MatchPattern::parse("https://*.test/*").unwrap()];
        let injected = s.content_scripts(&hosts);
        assert_eq!(injected.len(), 1, "a script outside every host permission is not injected");
        let matches: Vec<&str> = injected[0].matches.iter().map(MatchPattern::as_str).collect();
        assert_eq!(matches, ["http://127.0.0.1/*", "https://*.test/*"]);
        assert_eq!(injected[0].exclude_matches[0].as_str(), "*://*/private/*");
        let everywhere = s.content_scripts(&[MatchPattern::parse("<all_urls>").unwrap()]);
        assert_eq!(everywhere.iter().map(|c| c.matches.len()).collect::<Vec<_>>(), [2, 1]);
    }

    #[test]
    fn pattern_errors_use_chromes_words() {
        assert_eq!(pattern_error("example.com/*"), "Missing scheme separator.");
        assert_eq!(pattern_error("gopher://x/*"), "Invalid scheme.");
        assert_eq!(pattern_error("https://x"), "Empty path.");
        assert_eq!(pattern_error("https:///*"), "Host can not be empty.");
        assert_eq!(pattern_error("https://a.*.com/*"), "Invalid host wildcard.");
    }
}
