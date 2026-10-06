//! One extension's declarativeNetRequest rules as its API changes them: which of the
//! manifest's static rulesets are enabled, and the dynamic and session rules it added, with
//! Chrome's checks and limits. Pure; the runtime keeps [`Saved`] on disk (dynamic rules and
//! the enabled rulesets outlive a restart, session rules do not) and compiles the static
//! rulesets it names together with [`Rules::added`], so that an allow rule here can lift a
//! static block, as in Chrome.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vsesvit_core::extensions::manifest::DnrRuleset;

use crate::dnr::{ActionType, Rule, Skipped};

pub const MAX_NUMBER_OF_DYNAMIC_RULES: usize = 30_000;
pub const MAX_NUMBER_OF_UNSAFE_DYNAMIC_RULES: usize = 5_000;
pub const MAX_NUMBER_OF_SESSION_RULES: usize = 5_000;
pub const MAX_NUMBER_OF_UNSAFE_SESSION_RULES: usize = 5_000;
/// Across the dynamic and session rules.
pub const MAX_NUMBER_OF_REGEX_RULES: usize = 1_000;
pub const MAX_NUMBER_OF_ENABLED_STATIC_RULESETS: usize = 50;
/// What `getAvailableStaticRuleCount` counts down from: Chrome's guaranteed 30,000 static
/// rules plus its global pool of 300,000. Vsesvit sets no lower limit of its own.
pub const STATIC_RULE_BUDGET: usize = 330_000;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Dynamic,
    Session,
}

impl Scope {
    fn limits(self) -> (usize, usize, &'static str) {
        match self {
            Scope::Dynamic => (MAX_NUMBER_OF_DYNAMIC_RULES, MAX_NUMBER_OF_UNSAFE_DYNAMIC_RULES, "Dynamic"),
            Scope::Session => (MAX_NUMBER_OF_SESSION_RULES, MAX_NUMBER_OF_UNSAFE_SESSION_RULES, "Session"),
        }
    }
}

/// What outlives a restart. Dynamic rules also outlive an update; the enabled rulesets go
/// back to the manifest's choice with each new version.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    /// The enabled static rulesets, once the extension changed them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<Vec<String>>,
    #[serde(default)]
    pub dynamic: Vec<Value>,
}

/// A rule as the extension gave it, which `get*Rules` returns, and as the translator reads it.
#[derive(Clone, Debug)]
struct Added {
    json: Value,
    rule: Rule,
}

impl Added {
    /// Redirect and modifyHeaders rules, which Chrome limits apart.
    fn unsafe_action(&self) -> bool {
        matches!(self.rule.action.kind, ActionType::Redirect | ActionType::ModifyHeaders)
    }
}

#[derive(Clone, Debug)]
pub struct Rules {
    /// The manifest's ruleset ids, in its order.
    rulesets: Vec<String>,
    enabled: BTreeSet<String>,
    enabled_changed: bool,
    dynamic: Vec<Added>,
    session: Vec<Added>,
}

impl Rules {
    /// The rules of an extension with `rulesets` in its manifest, as `saved` left them. A saved
    /// rule that no longer parses is left out and reported.
    pub fn new(rulesets: &[DnrRuleset], saved: Saved) -> (Rules, Vec<Skipped>) {
        let ids: Vec<String> = rulesets.iter().map(|r| r.id.clone()).collect();
        let enabled = match &saved.enabled {
            Some(list) => list.iter().filter(|id| ids.contains(id)).cloned().collect(),
            None => rulesets.iter().filter(|r| r.enabled).map(|r| r.id.clone()).collect(),
        };
        let mut skipped = Vec::new();
        let dynamic = saved
            .dynamic
            .into_iter()
            .filter_map(|json| match parse(&json) {
                Ok(rule) => Some(Added { json, rule }),
                Err(reason) => {
                    skipped.push(Skipped { rule_id: json.get("id").and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok()), reason });
                    None
                }
            })
            .collect();
        let rules = Rules { rulesets: ids, enabled, enabled_changed: saved.enabled.is_some(), dynamic, session: Vec::new() };
        (rules, skipped)
    }

    pub fn saved(&self) -> Saved {
        Saved { enabled: self.enabled_changed.then(|| self.enabled()), dynamic: self.dynamic.iter().map(|a| a.json.clone()).collect() }
    }

    /// `getEnabledRulesets()`, in the manifest's order.
    pub fn enabled(&self) -> Vec<String> {
        self.rulesets.iter().filter(|id| self.enabled.contains(*id)).cloned().collect()
    }

    /// `updateEnabledRulesets({ disableRulesetIds, enableRulesetIds })`: disables, then
    /// enables, or changes nothing on an error.
    pub fn update_enabled(&mut self, options: &Value) -> Result<(), String> {
        let disable = ruleset_ids(&options["disableRulesetIds"])?;
        let enable = ruleset_ids(&options["enableRulesetIds"])?;
        if let Some(unknown) = disable.iter().chain(&enable).find(|id| !self.rulesets.contains(id)) {
            return Err(format!("Invalid ruleset id: {unknown}."));
        }
        let mut enabled = self.enabled.clone();
        for id in &disable {
            enabled.remove(id);
        }
        enabled.extend(enable);
        if enabled.len() > MAX_NUMBER_OF_ENABLED_STATIC_RULESETS {
            return Err("The number of enabled static rulesets exceeds the enabled ruleset count limit.".into());
        }
        self.enabled = enabled;
        self.enabled_changed = true;
        Ok(())
    }

    /// `updateDynamicRules` or `updateSessionRules({ removeRuleIds, addRules })`: removes,
    /// then adds, or changes nothing on an error. Ids that match no rule are ignored.
    pub fn update(&mut self, scope: Scope, options: &Value) -> Result<(), String> {
        let remove: Vec<u32> = match &options["removeRuleIds"] {
            Value::Null => Vec::new(),
            ids => serde_json::from_value(ids.clone()).map_err(|_| "removeRuleIds must be a list of rule ids".to_owned())?,
        };
        let add = match &options["addRules"] {
            Value::Null => Vec::new(),
            Value::Array(rules) => rules.clone(),
            _ => return Err("addRules must be a list of rules".into()),
        };
        let mut rules: Vec<Added> = self.list(scope).iter().filter(|a| !remove.contains(&a.rule.id)).cloned().collect();
        for json in add {
            let rule = parse(&json)?;
            if scope == Scope::Dynamic && (rule.condition.tab_ids.is_some() || rule.condition.excluded_tab_ids.is_some()) {
                return Err(format!("Rule with id {} specifies the \"tabIds\" or \"excludedTabIds\" condition, which only session rules may use.", rule.id));
            }
            if rules.iter().any(|a| a.rule.id == rule.id) {
                return Err(format!("Rule with id {} does not have a unique ID.", rule.id));
            }
            let mut json = json;
            json["priority"] = json!(rule.priority);
            rules.push(Added { json, rule });
        }
        let (max, max_unsafe, name) = scope.limits();
        if rules.len() > max {
            return Err(format!("{name} rule count exceeded."));
        }
        if rules.iter().filter(|a| a.unsafe_action()).count() > max_unsafe {
            return Err(format!("{name} rule count for unsafe rules exceeded."));
        }
        let other = match scope {
            Scope::Dynamic => &self.session,
            Scope::Session => &self.dynamic,
        };
        if rules.iter().chain(other).filter(|a| a.rule.condition.regex_filter.is_some()).count() > MAX_NUMBER_OF_REGEX_RULES {
            return Err(format!("{name} rule count for regex rules exceeded."));
        }
        *self.list_mut(scope) = rules;
        Ok(())
    }

    /// `getDynamicRules` or `getSessionRules(filter)`: every rule, or those `filter.ruleIds`
    /// names.
    pub fn get(&self, scope: Scope, filter: &Value) -> Result<Vec<Value>, String> {
        let ids: Option<Vec<u32>> = match &filter["ruleIds"] {
            Value::Null => None,
            ids => Some(serde_json::from_value(ids.clone()).map_err(|_| "ruleIds must be a list of rule ids".to_owned())?),
        };
        Ok(self.list(scope).iter().filter(|a| ids.as_ref().is_none_or(|ids| ids.contains(&a.rule.id))).map(|a| a.json.clone()).collect())
    }

    /// The dynamic rules, then the session rules, for the content blocker.
    pub fn added(&self) -> Vec<Rule> {
        self.dynamic.iter().chain(&self.session).map(|a| a.rule.clone()).collect()
    }

    fn list(&self, scope: Scope) -> &Vec<Added> {
        match scope {
            Scope::Dynamic => &self.dynamic,
            Scope::Session => &self.session,
        }
    }

    fn list_mut(&mut self, scope: Scope) -> &mut Vec<Added> {
        match scope {
            Scope::Dynamic => &mut self.dynamic,
            Scope::Session => &mut self.session,
        }
    }
}

/// One rule the extension adds, checked as Chrome checks it. Whether WebKit can express it
/// is the translator's business: like a static rule, one it cannot is kept and logged.
fn parse(json: &Value) -> Result<Rule, String> {
    let id = json.get("id").and_then(Value::as_i64);
    let named = |what: &str| match id {
        Some(id) => format!("Rule with id {id} {what}"),
        None => format!("A rule without an id {what}"),
    };
    let rule: Rule = serde_json::from_value(json.clone()).map_err(|e| named(&format!("is invalid: {e}.")))?;
    if rule.id == 0 {
        return Err(named("does not have a valid ID."));
    }
    if rule.priority == 0 {
        return Err(named("does not have a valid priority."));
    }
    Ok(rule)
}

fn ruleset_ids(v: &Value) -> Result<Vec<String>, String> {
    match v {
        Value::Null => Ok(Vec::new()),
        ids => serde_json::from_value(ids.clone()).map_err(|_| "ruleset ids must be strings".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vsesvit_core::extensions::manifest::RelPath;

    fn rulesets() -> Vec<DnrRuleset> {
        let ruleset = |id: &str, enabled| DnrRuleset { id: id.into(), enabled, path: RelPath::parse(&format!("{id}.json")).unwrap() };
        vec![ruleset("ads", true), ruleset("privacy", true), ruleset("annoyances", false)]
    }

    fn fresh() -> Rules {
        Rules::new(&rulesets(), Saved::default()).0
    }

    fn block(id: u32, filter: &str) -> Value {
        json!({ "id": id, "action": { "type": "block" }, "condition": { "urlFilter": filter } })
    }

    fn ids(rules: &Rules, scope: Scope) -> Vec<u64> {
        rules.get(scope, &Value::Null).unwrap().iter().map(|r| r["id"].as_u64().unwrap()).collect()
    }

    #[test]
    fn enabled_rulesets_start_from_the_manifest_and_change_atomically() {
        let mut r = fresh();
        assert_eq!(r.enabled(), ["ads", "privacy"]);
        assert_eq!(r.saved().enabled, None, "nothing to keep until the extension changes them");
        r.update_enabled(&json!({ "disableRulesetIds": ["ads"], "enableRulesetIds": ["annoyances"] })).unwrap();
        assert_eq!(r.enabled(), ["privacy", "annoyances"]);
        assert_eq!(r.update_enabled(&json!({ "enableRulesetIds": ["ads", "nope"] })), Err("Invalid ruleset id: nope.".into()));
        assert_eq!(r.enabled(), ["privacy", "annoyances"]);
        // Disabling comes first, so a ruleset in both lists ends up enabled.
        r.update_enabled(&json!({ "disableRulesetIds": ["privacy"], "enableRulesetIds": ["privacy"] })).unwrap();
        assert_eq!(r.enabled(), ["privacy", "annoyances"]);
        let saved = r.saved();
        assert_eq!(saved.enabled.as_deref(), Some(["privacy".to_owned(), "annoyances".to_owned()].as_slice()));
        assert_eq!(Rules::new(&rulesets(), saved).0.enabled(), ["privacy", "annoyances"]);
    }

    #[test]
    fn rules_are_removed_then_added_or_not_at_all() {
        let mut r = fresh();
        r.update(Scope::Dynamic, &json!({ "addRules": [block(1, "a"), block(2, "b")] })).unwrap();
        r.update(Scope::Dynamic, &json!({ "removeRuleIds": [1, 99], "addRules": [block(1, "c")] })).unwrap();
        assert_eq!(ids(&r, Scope::Dynamic), [2, 1]);
        assert_eq!(r.get(Scope::Dynamic, &json!({ "ruleIds": [1] })).unwrap(), [json!({ "id": 1, "priority": 1, "action": { "type": "block" }, "condition": { "urlFilter": "c" } })]);
        let duplicate = r.update(Scope::Dynamic, &json!({ "removeRuleIds": [2], "addRules": [block(3, "d"), block(1, "e")] }));
        assert_eq!(duplicate, Err("Rule with id 1 does not have a unique ID.".into()));
        assert_eq!(ids(&r, Scope::Dynamic), [2, 1], "a failed update changes nothing");
        assert!(r.update(Scope::Dynamic, &json!({ "addRules": [block(0, "x")] })).unwrap_err().contains("valid ID"));
        assert!(r.update(Scope::Dynamic, &json!({ "addRules": [{ "id": 5, "action": { "type": "nope" }, "condition": {} }] })).unwrap_err().starts_with("Rule with id 5 is invalid"));
        assert!(ids(&r, Scope::Session).is_empty(), "session rules are apart");
    }

    #[test]
    fn tab_conditions_are_for_session_rules_only() {
        let mut r = fresh();
        let rule = json!({ "id": 1, "action": { "type": "allow" }, "condition": { "tabIds": [-1] } });
        assert!(r.update(Scope::Dynamic, &json!({ "addRules": [rule.clone()] })).is_err());
        r.update(Scope::Session, &json!({ "addRules": [rule] })).unwrap();
        assert_eq!(ids(&r, Scope::Session), [1]);
    }

    #[test]
    fn chrome_limits_apply() {
        let mut r = fresh();
        let many: Vec<Value> = (1..=MAX_NUMBER_OF_SESSION_RULES as u32 + 1).map(|id| block(id, "x")).collect();
        assert_eq!(r.update(Scope::Session, &json!({ "addRules": many })), Err("Session rule count exceeded.".into()));
        let regex = |id: u32| json!({ "id": id, "action": { "type": "block" }, "condition": { "regexFilter": "^x" } });
        let half: Vec<Value> = (1..=MAX_NUMBER_OF_REGEX_RULES as u32 / 2).map(regex).collect();
        r.update(Scope::Dynamic, &json!({ "addRules": half })).unwrap();
        let rest: Vec<Value> = (1..=MAX_NUMBER_OF_REGEX_RULES as u32 / 2 + 1).map(regex).collect();
        assert_eq!(r.update(Scope::Session, &json!({ "addRules": rest })), Err("Session rule count for regex rules exceeded.".into()));
        let redirect = |id: u32| json!({ "id": id, "action": { "type": "redirect", "redirect": { "url": "https://x.test/" } }, "condition": {} });
        let unsafe_rules: Vec<Value> = (1000..=1000 + MAX_NUMBER_OF_UNSAFE_DYNAMIC_RULES as u32).map(redirect).collect();
        assert_eq!(r.update(Scope::Dynamic, &json!({ "addRules": unsafe_rules })), Err("Dynamic rule count for unsafe rules exceeded.".into()));
    }

    #[test]
    fn dynamic_rules_and_enabled_rulesets_are_saved_and_session_rules_are_not() {
        let mut r = fresh();
        r.update(Scope::Dynamic, &json!({ "addRules": [block(7, "kept")] })).unwrap();
        r.update(Scope::Session, &json!({ "addRules": [block(8, "dropped")] })).unwrap();
        assert_eq!(r.added().iter().map(|rule| rule.id).collect::<Vec<_>>(), [7, 8]);
        let saved: Saved = serde_json::from_str(&serde_json::to_string(&r.saved()).unwrap()).unwrap();
        let (restored, skipped) = Rules::new(&rulesets(), saved);
        assert!(skipped.is_empty());
        assert_eq!(ids(&restored, Scope::Dynamic), [7]);
        assert!(ids(&restored, Scope::Session).is_empty());
        let corrupt = Saved { enabled: Some(vec!["gone".into(), "ads".into()]), dynamic: vec![json!({ "id": 3, "action": {} })] };
        let (restored, skipped) = Rules::new(&rulesets(), corrupt);
        assert_eq!(restored.enabled(), ["ads"], "rulesets a new version dropped are forgotten");
        assert_eq!(skipped.iter().map(|s| s.rule_id).collect::<Vec<_>>(), [Some(3)]);
    }
}
