//! declarativeNetRequest static rulesets translated to WebKit content-blocker JSON.
//!
//! Platform-neutral and pure: `rules.json` text in, content-blocker JSON out, plus the
//! list of rules WebKit cannot express and why. The mapping follows WebKit's own
//! translator (`_WKWebExtensionDeclarativeNetRequestRule.mm`), as summarized in
//! `docs/design/research-linux-extensions.md` section 6.
//!
//! Ordering is the one subtle point. WebKit evaluates the rules of one filter in list
//! order and `ignore-following-rules` cancels every later rule, while DNR picks the
//! matching rule with the highest priority and, at equal priority, prefers allow over
//! block over upgrade over redirect. Emitting the rules sorted by priority (descending)
//! and then by that action rank reproduces DNR's choice: an allow rule precedes exactly
//! the block rules it may override. That only holds inside one filter, so every
//! extension's enabled rulesets are merged into one list.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// One DNR rule as written in a ruleset file. Unknown fields are ignored, as Chrome does.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: u32,
    #[serde(default = "default_priority")]
    pub priority: u32,
    pub action: Action,
    pub condition: Condition,
}

fn default_priority() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    #[serde(rename = "type")]
    pub kind: ActionType,
    pub redirect: Option<Redirect>,
    #[serde(default)]
    pub request_headers: Vec<HeaderOp>,
    #[serde(default)]
    pub response_headers: Vec<HeaderOp>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionType {
    Block,
    Allow,
    AllowAllRequests,
    UpgradeScheme,
    Redirect,
    ModifyHeaders,
}

impl ActionType {
    /// DNR's tie-break at equal priority, lowest first.
    fn rank(self) -> u8 {
        match self {
            ActionType::Allow => 0,
            ActionType::AllowAllRequests => 1,
            ActionType::Block => 2,
            ActionType::UpgradeScheme => 3,
            ActionType::Redirect => 4,
            ActionType::ModifyHeaders => 5,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Redirect {
    pub url: Option<String>,
    pub extension_path: Option<String>,
    pub regex_substitution: Option<String>,
    pub transform: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderOp {
    pub header: String,
    pub operation: HeaderOperation,
    pub value: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HeaderOperation {
    Set,
    Append,
    Remove,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub url_filter: Option<String>,
    pub regex_filter: Option<String>,
    #[serde(default)]
    pub is_url_filter_case_sensitive: bool,
    pub resource_types: Option<Vec<ResourceType>>,
    pub excluded_resource_types: Option<Vec<ResourceType>>,
    pub domain_type: Option<DomainType>,
    pub domains: Option<Vec<String>>,
    pub excluded_domains: Option<Vec<String>>,
    pub initiator_domains: Option<Vec<String>>,
    pub excluded_initiator_domains: Option<Vec<String>>,
    pub request_domains: Option<Vec<String>>,
    pub excluded_request_domains: Option<Vec<String>>,
    pub request_methods: Option<Vec<String>>,
    pub excluded_request_methods: Option<Vec<String>>,
    pub tab_ids: Option<Vec<i64>>,
    pub excluded_tab_ids: Option<Vec<i64>>,
    pub response_headers: Option<Value>,
    pub excluded_response_headers: Option<Value>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    MainFrame,
    SubFrame,
    Stylesheet,
    Script,
    Image,
    Font,
    Object,
    Xmlhttprequest,
    Ping,
    CspReport,
    Media,
    Websocket,
    Webtransport,
    Webbundle,
    Other,
}

impl ResourceType {
    const ALL: [ResourceType; 15] = [
        ResourceType::MainFrame,
        ResourceType::SubFrame,
        ResourceType::Stylesheet,
        ResourceType::Script,
        ResourceType::Image,
        ResourceType::Font,
        ResourceType::Object,
        ResourceType::Xmlhttprequest,
        ResourceType::Ping,
        ResourceType::CspReport,
        ResourceType::Media,
        ResourceType::Websocket,
        ResourceType::Webtransport,
        ResourceType::Webbundle,
        ResourceType::Other,
    ];

    /// The WebKit `resource-type` value. Types WebKit has no name for map to `other`.
    fn webkit(self) -> &'static str {
        match self {
            ResourceType::MainFrame => "top-document",
            ResourceType::SubFrame => "child-document",
            ResourceType::Stylesheet => "style-sheet",
            ResourceType::Script => "script",
            ResourceType::Image => "image",
            ResourceType::Font => "font",
            ResourceType::Xmlhttprequest => "fetch",
            ResourceType::Ping => "ping",
            ResourceType::Media => "media",
            ResourceType::Websocket => "websocket",
            ResourceType::Object
            | ResourceType::CspReport
            | ResourceType::Webtransport
            | ResourceType::Webbundle
            | ResourceType::Other => "other",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DomainType {
    FirstParty,
    ThirdParty,
}

/// A rule that was left out of the filter, with the reason for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub rule_id: Option<u32>,
    pub reason: String,
}

/// The content-blocker rules for one extension plus what was dropped on the way.
#[derive(Clone, Debug, Default)]
pub struct Translation {
    pub rules: Vec<Value>,
    pub skipped: Vec<Skipped>,
}

impl Translation {
    /// The JSON WebKit's `UserContentFilterStore` compiles.
    pub fn to_json(&self) -> String {
        Value::Array(self.rules.clone()).to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Parse a ruleset file. A malformed rule is reported and skipped; only a file that is not
/// a JSON array at all is an error.
pub fn parse_rules(text: &str) -> Result<(Vec<Rule>, Vec<Skipped>), serde_json::Error> {
    let raw: Vec<Value> = serde_json::from_str(text)?;
    let mut rules = Vec::with_capacity(raw.len());
    let mut skipped = Vec::new();
    for value in raw {
        let id = value.get("id").and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok());
        match serde_json::from_value::<Rule>(value) {
            Ok(rule) => rules.push(rule),
            Err(e) => skipped.push(Skipped { rule_id: id, reason: format!("malformed rule: {e}") }),
        }
    }
    Ok((rules, skipped))
}

/// What an extension's permissions let its rules do, as in Chrome: rulesets need the
/// `declarativeNetRequest` (or `declarativeNetRequestWithHostAccess`) permission, and
/// redirect and modifyHeaders rules (every rule, with only the latter) act only on
/// requests to hosts the extension has host permissions for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grants {
    pub hosts: HostScope,
    /// Only `declarativeNetRequestWithHostAccess`: every rule needs host access.
    pub host_access_only: bool,
}

/// The hosts an extension's host permissions cover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostScope {
    /// `<all_urls>` or a pattern for any host of any web scheme.
    All,
    /// Just these; none when the extension has no host permissions.
    Hosts(Vec<HostGrant>),
}

/// One host permission pattern, as a request URL condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostGrant {
    /// `None` for `*` (http, https, ws, wss).
    scheme: Option<String>,
    /// `*` for any host.
    host: String,
    subdomains: bool,
}

impl Grants {
    /// `None` when `permissions` has neither DNR permission, so no ruleset applies.
    pub fn from_manifest(permissions: &[String], host_permissions: &[String]) -> Option<Grants> {
        let has = |name: &str| permissions.iter().any(|p| p == name);
        if !has("declarativeNetRequest") && !has("declarativeNetRequestWithHostAccess") {
            return None;
        }
        Some(Grants { hosts: HostScope::from_patterns(host_permissions), host_access_only: !has("declarativeNetRequest") })
    }
}

impl HostScope {
    /// Match patterns (`<all_urls>`, `*://*.example.com/*`, ...) as hosts; paths do not
    /// limit host access, and patterns for schemes without hosts (`file`) grant none.
    pub fn from_patterns(patterns: &[String]) -> HostScope {
        let mut grants = Vec::new();
        for pattern in patterns {
            if pattern == "<all_urls>" {
                return HostScope::All;
            }
            let Some((scheme, rest)) = pattern.split_once("://") else { continue };
            let scheme = match scheme {
                "*" => None,
                "http" | "https" | "ws" | "wss" | "ftp" => Some(scheme.to_owned()),
                _ => continue,
            };
            let host = rest.split('/').next().unwrap_or_default();
            let host = host.split(':').next().unwrap_or_default().to_ascii_lowercase();
            if host.is_empty() {
                continue;
            }
            if host == "*" && scheme.is_none() {
                return HostScope::All;
            }
            grants.push(match host.strip_prefix("*.") {
                Some(domain) => HostGrant { scheme, host: domain.to_owned(), subdomains: true },
                None => HostGrant { scheme, subdomains: host == "*", host },
            });
        }
        HostScope::Hosts(grants)
    }
}

impl HostGrant {
    /// The part of this grant inside `domain` and its subdomains (a `requestDomains`
    /// entry), if any.
    fn within(&self, domain: &str) -> Option<HostGrant> {
        let domain = domain.trim_start_matches("*.").to_ascii_lowercase();
        let under = |host: &str, parent: &str| host == parent || host.ends_with(&format!(".{parent}"));
        if self.host == "*" || (self.subdomains && under(&domain, &self.host)) {
            Some(HostGrant { scheme: self.scheme.clone(), host: domain, subdomains: true })
        } else if under(&self.host, &domain) {
            Some(self.clone())
        } else {
            None
        }
    }

    /// `^https://+([^:/]+\.)?example\.com[:/]`: the start of a URL this grant covers.
    fn anchor(&self) -> Result<String, String> {
        let mut re = String::from("^");
        match &self.scheme {
            Some(s) => s.chars().for_each(|ch| push_literal(&mut re, ch)),
            None => re.push_str("[^:]+"),
        }
        re.push_str("://+");
        if self.host == "*" {
            return Ok(re);
        }
        if !self.host.is_ascii() {
            return Err(format!("host permission {:?} is not ASCII", self.host));
        }
        if self.subdomains {
            re.push_str("([^:/]+\\.)?");
        }
        for ch in self.host.chars() {
            push_literal(&mut re, ch);
        }
        re.push_str("[:/]");
        Ok(re)
    }
}

/// Translate the merged rules of one extension. `extension_base` is
/// `chrome-extension://<id>`; `redirect.extensionPath` resolves against it.
pub fn translate(rules: &[Rule], extension_base: &str, grants: &Grants) -> Translation {
    let mut out = Translation::default();
    let mut ordered: Vec<&Rule> = rules.iter().collect();
    ordered.sort_by_key(|r| (std::cmp::Reverse(r.priority), r.action.kind.rank()));
    for rule in ordered {
        match translate_rule(rule, extension_base, grants) {
            Ok(webkit_rules) => out.rules.extend(webkit_rules),
            Err(reason) => out.skipped.push(Skipped { rule_id: Some(rule.id), reason }),
        }
    }
    out
}

fn translate_rule(rule: &Rule, extension_base: &str, grants: &Grants) -> Result<Vec<Value>, String> {
    let c = &rule.condition;
    if c.tab_ids.is_some() || c.excluded_tab_ids.is_some() {
        return Err("tabIds conditions cannot be expressed as a content blocker".into());
    }
    if c.response_headers.is_some() || c.excluded_response_headers.is_some() {
        return Err("responseHeaders conditions cannot be expressed as a content blocker".into());
    }
    if c.excluded_request_domains.is_some() {
        return Err("excludedRequestDomains cannot be expressed as a content blocker".into());
    }
    // `domains` and `excludedDomains` are Chrome's deprecated names for the initiator lists.
    let initiators = merged(&c.domains, &c.initiator_domains);
    let excluded_initiators = merged(&c.excluded_domains, &c.excluded_initiator_domains);
    if initiators.is_some() && excluded_initiators.is_some() {
        return Err("initiatorDomains (domains) and excludedInitiatorDomains (excludedDomains) together are not supported".into());
    }

    let filters = url_filter_regex(c)?;
    let mut trigger = Map::new();
    if c.is_url_filter_case_sensitive {
        trigger.insert("url-filter-is-case-sensitive".into(), json!(true));
    }
    if let Some(load_type) = c.domain_type {
        let v = match load_type {
            DomainType::FirstParty => "first-party",
            DomainType::ThirdParty => "third-party",
        };
        trigger.insert("load-type".into(), json!([v]));
    }
    // The only frame or domain condition on the trigger: WebKit rejects a trigger with two,
    // and with it the extension's whole filter.
    if let Some(initiators) = &initiators {
        trigger.insert("if-frame-url".into(), frame_url_list(initiators)?);
    } else if let Some(excluded) = &excluded_initiators {
        trigger.insert("unless-frame-url".into(), frame_url_list(excluded)?);
    }
    // WebKit takes one `request-method` string per rule, so a method list fans out.
    let methods: Vec<Option<String>> = match request_methods(c)? {
        Some(list) => list.into_iter().map(Some).collect(),
        None => vec![None],
    };

    let action = translate_action(rule, extension_base)?;

    // The host permissions a rule of this kind is limited to; `None` when it may act on
    // any request.
    let needs_hosts = grants.host_access_only || matches!(rule.action.kind, ActionType::Redirect | ActionType::ModifyHeaders);
    let scope = match &grants.hosts {
        HostScope::Hosts(list) if needs_hosts => Some(list),
        _ => None,
    };

    if rule.action.kind == ActionType::AllowAllRequests {
        if scope.is_some() {
            return Err("allowAllRequests needs host permissions for every host".into());
        }
        return allow_all_requests(c, &filters, trigger, action, &methods);
    }

    trigger.insert("resource-type".into(), json!(resource_types(c)));

    let (hosts, what) = match (scope, &c.request_domains) {
        (Some(grants), domains) => {
            let covered: Vec<HostGrant> = match domains {
                Some(domains) => domains.iter().flat_map(|d| grants.iter().filter_map(move |g| g.within(d))).collect(),
                None => grants.clone(),
            };
            let anchors: BTreeSet<String> = covered.iter().map(HostGrant::anchor).collect::<Result<_, _>>()?;
            if anchors.is_empty() {
                return Err("this action needs host permissions for the requests it acts on".into());
            }
            (Some(anchors.into_iter().collect::<Vec<_>>()), "a rule limited to host permissions")
        }
        (None, Some(domains)) => (Some(domains.iter().map(|d| domain_regex(d)).collect::<Result<Vec<_>, _>>()?), "requestDomains"),
        (None, None) => (None, ""),
    };
    // WebKit takes one `url-filter` per rule, so alternative filters fan out too.
    let url_filters = match hosts {
        Some(hosts) => filters.iter().map(|f| fold_hosts(&hosts, f, what)).collect::<Result<Vec<_>, _>>()?.concat(),
        None => filters,
    };
    let mut out = Vec::new();
    for url_filter in url_filters {
        for method in &methods {
            let mut t = trigger.clone();
            t.insert("url-filter".into(), json!(url_filter));
            if let Some(m) = method {
                t.insert("request-method".into(), json!(m));
            }
            out.push(json!({ "trigger": Value::Object(t), "action": action }));
        }
    }
    Ok(out)
}

/// `allowAllRequests` exempts every load inside a frame whose URL matches, so the URL
/// condition moves from the request to the frame: `if-top-url` for `main_frame`,
/// `if-frame-url` for `sub_frame`.
fn allow_all_requests(
    c: &Condition,
    filters: &[String],
    trigger: Map<String, Value>,
    action: Value,
    methods: &[Option<String>],
) -> Result<Vec<Value>, String> {
    let types = c.resource_types.clone().unwrap_or_else(|| vec![ResourceType::MainFrame]);
    if types.iter().any(|t| !matches!(t, ResourceType::MainFrame | ResourceType::SubFrame)) {
        return Err("allowAllRequests resourceTypes must be main_frame or sub_frame".into());
    }
    if trigger.contains_key("if-frame-url") || trigger.contains_key("unless-frame-url") {
        return Err("allowAllRequests cannot combine initiator domains with the frame URL condition".into());
    }
    if c.request_domains.is_some() {
        return Err("allowAllRequests with requestDomains is not supported".into());
    }
    let mut out = Vec::new();
    for t in types {
        let key = match t {
            ResourceType::MainFrame => "if-top-url",
            _ => "if-frame-url",
        };
        for method in methods {
            let mut trig = trigger.clone();
            trig.insert("url-filter".into(), json!(".*"));
            trig.insert(key.into(), json!(filters));
            if let Some(m) = method {
                trig.insert("request-method".into(), json!(m));
            }
            out.push(json!({ "trigger": Value::Object(trig), "action": action }));
        }
    }
    Ok(out)
}

fn translate_action(rule: &Rule, extension_base: &str) -> Result<Value, String> {
    let a = &rule.action;
    Ok(match a.kind {
        ActionType::Block => json!({ "type": "block" }),
        ActionType::Allow | ActionType::AllowAllRequests => json!({ "type": "ignore-following-rules" }),
        ActionType::UpgradeScheme => json!({ "type": "make-https" }),
        ActionType::Redirect => {
            let r = a.redirect.as_ref().ok_or("redirect action without a redirect object")?;
            let redirect = if let Some(url) = &r.url {
                json!({ "url": url })
            } else if let Some(path) = &r.extension_path {
                json!({ "url": format!("{}{}", extension_base.trim_end_matches('/'), path) })
            } else if let Some(sub) = &r.regex_substitution {
                json!({ "regex-substitution": sub.replace('\\', "$") })
            } else if let Some(t) = &r.transform {
                json!({ "transform": transform(t)? })
            } else {
                return Err("redirect without url, extensionPath, regexSubstitution or transform".into());
            };
            json!({ "type": "redirect", "redirect": redirect })
        }
        ActionType::ModifyHeaders => {
            if a.request_headers.is_empty() && a.response_headers.is_empty() {
                return Err("modifyHeaders without any header operation".into());
            }
            let mut v = json!({ "type": "modify-headers", "priority": rule.priority });
            if !a.request_headers.is_empty() {
                v["request-headers"] = header_ops(&a.request_headers)?;
            }
            if !a.response_headers.is_empty() {
                v["response-headers"] = header_ops(&a.response_headers)?;
            }
            v
        }
    })
}

fn header_ops(ops: &[HeaderOp]) -> Result<Value, String> {
    ops.iter()
        .map(|op| {
            let operation = match op.operation {
                HeaderOperation::Set => "set",
                HeaderOperation::Append => "append",
                HeaderOperation::Remove => "remove",
            };
            let mut v = json!({ "operation": operation, "header": op.header });
            match (&op.value, op.operation) {
                (Some(value), HeaderOperation::Set | HeaderOperation::Append) => v["value"] = json!(value),
                (None, HeaderOperation::Set | HeaderOperation::Append) => {
                    return Err(format!("header operation on {:?} needs a value", op.header));
                }
                (_, HeaderOperation::Remove) => {}
            }
            Ok(v)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// DNR `URLTransform` (camelCase) to WebKit's kebab-case transform object.
fn transform(t: &Value) -> Result<Value, String> {
    let obj = t.as_object().ok_or("redirect.transform must be an object")?;
    let mut out = Map::new();
    for (k, v) in obj {
        match k.as_str() {
            "scheme" | "host" | "port" | "path" | "query" | "fragment" | "username" | "password" => {
                out.insert(k.clone(), v.clone());
            }
            "queryTransform" => {
                let qt = v.as_object().ok_or("queryTransform must be an object")?;
                let mut q = Map::new();
                if let Some(add) = qt.get("addOrReplaceParams") {
                    let items = add.as_array().ok_or("addOrReplaceParams must be an array")?;
                    let mapped: Vec<Value> = items
                        .iter()
                        .map(|p| {
                            let mut m = json!({ "key": p.get("key").cloned().unwrap_or(Value::Null), "value": p.get("value").cloned().unwrap_or(Value::Null) });
                            if let Some(r) = p.get("replaceOnly") {
                                m["replace-only"] = r.clone();
                            }
                            m
                        })
                        .collect();
                    q.insert("add-or-replace-parameters".into(), Value::Array(mapped));
                }
                if let Some(remove) = qt.get("removeParams") {
                    q.insert("remove-parameters".into(), remove.clone());
                }
                out.insert("query-transform".into(), Value::Object(q));
            }
            other => return Err(format!("unsupported transform key {other:?}")),
        }
    }
    Ok(Value::Object(out))
}

fn resource_types(c: &Condition) -> Vec<&'static str> {
    let selected: Vec<ResourceType> = match (&c.resource_types, &c.excluded_resource_types) {
        (Some(types), _) => types.clone(),
        (None, Some(excluded)) => ResourceType::ALL.iter().copied().filter(|t| !excluded.contains(t)).collect(),
        // DNR default: everything but the main frame.
        (None, None) => ResourceType::ALL.iter().copied().filter(|t| *t != ResourceType::MainFrame).collect(),
    };
    let set: BTreeSet<&'static str> = selected.into_iter().map(ResourceType::webkit).collect();
    set.into_iter().collect()
}

const ALL_METHODS: [&str; 9] = ["connect", "delete", "get", "head", "options", "patch", "post", "put", "trace"];

fn request_methods(c: &Condition) -> Result<Option<Vec<String>>, String> {
    let normalize = |methods: &[String]| -> Result<Vec<String>, String> {
        methods
            .iter()
            .map(|m| {
                let m = m.to_ascii_lowercase();
                if ALL_METHODS.contains(&m.as_str()) { Ok(m) } else { Err(format!("unsupported request method {m:?}")) }
            })
            .collect()
    };
    Ok(match (&c.request_methods, &c.excluded_request_methods) {
        (Some(methods), _) => Some(normalize(methods)?),
        (None, Some(excluded)) => {
            let excluded = normalize(excluded)?;
            Some(ALL_METHODS.iter().filter(|m| !excluded.iter().any(|e| e == *m)).map(|m| (*m).to_owned()).collect())
        }
        (None, None) => None,
    })
}

/// Both lists in one, or `None` when neither is given.
fn merged(a: &Option<Vec<String>>, b: &Option<Vec<String>>) -> Option<Vec<String>> {
    match (a, b) {
        (None, None) => None,
        _ => Some(a.iter().chain(b).flatten().cloned().collect()),
    }
}

fn frame_url_list(domains: &[String]) -> Result<Value, String> {
    domains.iter().map(|d| domain_regex(d).map(Value::String)).collect::<Result<Vec<_>, _>>().map(Value::Array)
}

/// `^[^:]+://+([^:/]+\.)?example\.com[:/]`: the host is `domain` or a subdomain of it.
fn domain_regex(domain: &str) -> Result<String, String> {
    if !domain.is_ascii() || domain.is_empty() {
        return Err(format!("domain {domain:?} is not ASCII"));
    }
    let mut re = String::from("^[^:]+://+([^:/]+\\.)?");
    for ch in domain.trim_start_matches("*.").chars() {
        push_literal(&mut re, ch.to_ascii_lowercase());
    }
    re.push_str("[:/]");
    Ok(re)
}

/// Host anchors (`requestDomains`, host permissions) go in front of the filter, one rule
/// each. That only composes with a filter that is not itself anchored at the start.
fn fold_hosts(hosts: &[String], filter: &str, what: &str) -> Result<Vec<String>, String> {
    if filter.starts_with('^') {
        return Err(format!("{what} cannot combine with a start-anchored filter"));
    }
    Ok(hosts.iter().map(|host| if filter == ".*" { host.clone() } else { format!("{host}.*{filter}") }).collect())
}

/// The condition's URL filter as WebKit regexes; a request matches if any of them does.
fn url_filter_regex(c: &Condition) -> Result<Vec<String>, String> {
    match (&c.url_filter, &c.regex_filter) {
        (Some(_), Some(_)) => Err("urlFilter and regexFilter are mutually exclusive".into()),
        (Some(f), None) => url_filter_to_regex(f),
        (None, Some(r)) => {
            check_webkit_regex(r)?;
            Ok(vec![r.clone()])
        }
        (None, None) => Ok(vec![".*".into()]),
    }
}

/// Chrome's URL filter grammar: `||` anchors at a host boundary, `|` at the start or the
/// end, `*` is a wildcard, `^` a separator, everything else literal. A separator at the
/// very end also matches the end of the URL; WebKit's regex subset has no alternation, so
/// that filter becomes two regexes, one for each.
pub fn url_filter_to_regex(filter: &str) -> Result<Vec<String>, String> {
    if !filter.is_ascii() {
        return Err(format!("urlFilter {filter:?} contains non-ASCII characters"));
    }
    if filter.is_empty() {
        return Ok(vec![".*".into()]);
    }
    let mut out = String::new();
    let mut rest = filter;
    if let Some(r) = rest.strip_prefix("||") {
        out.push_str("^[^:]+://+([^:/]+\\.)?");
        rest = r;
    } else if let Some(r) = rest.strip_prefix('|') {
        out.push('^');
        rest = r;
    }
    let end_anchor = rest.ends_with('|');
    if end_anchor {
        rest = &rest[..rest.len() - 1];
    }
    let trailing_separator = rest.ends_with('^');
    if trailing_separator {
        rest = &rest[..rest.len() - 1];
    }
    for ch in rest.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '|' => return Err(format!("urlFilter {filter:?} has `|` in the middle")),
            '^' => out.push_str(SEPARATOR),
            other => push_literal(&mut out, other),
        }
    }
    let end = if end_anchor { "$" } else { "" };
    if trailing_separator {
        return Ok(vec![format!("{out}{SEPARATOR}{end}"), format!("{out}$")]);
    }
    out.push_str(end);
    if out.is_empty() {
        out.push_str(".*");
    }
    Ok(vec![out])
}

/// A character that is not a letter, a digit or one of `_-.%`.
const SEPARATOR: &str = "[^-.%a-zA-Z0-9_]";

fn push_literal(out: &mut String, ch: char) {
    if matches!(ch, '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' | '$' | '^' | '|' | '/') {
        out.push('\\');
    }
    out.push(ch);
}

/// WebKit compiles `url-filter` with its own engine: ASCII only, no alternation, no word
/// boundaries or character-class escapes, no back-references or counted repetition, and
/// anchors only at the ends.
pub fn check_webkit_regex(re: &str) -> Result<(), String> {
    if !re.is_ascii() {
        return Err(format!("regexFilter {re:?} contains non-ASCII characters"));
    }
    let bytes = re.as_bytes();
    let mut i = 0;
    let mut in_class = false;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\\' => {
                let Some(&next) = bytes.get(i + 1) else {
                    return Err(format!("regexFilter {re:?} ends with a backslash"));
                };
                if matches!(next, b'b' | b'B' | b'd' | b'D' | b'w' | b'W' | b's' | b'S' | b'1'..=b'9') {
                    return Err(format!("regexFilter {re:?} uses \\{} which WebKit does not support", next as char));
                }
                i += 2;
                continue;
            }
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'|' if !in_class => return Err(format!("regexFilter {re:?} uses alternation")),
            b'{' if !in_class => return Err(format!("regexFilter {re:?} uses counted repetition")),
            b'(' if !in_class && bytes.get(i + 1) == Some(&b'?') => {
                return Err(format!("regexFilter {re:?} uses a lookaround or non-capturing group"));
            }
            b'^' if !in_class && i != 0 => return Err(format!("regexFilter {re:?} has `^` away from the start")),
            b'$' if !in_class && i + 1 != bytes.len() => return Err(format!("regexFilter {re:?} has `$` away from the end")),
            _ => {}
        }
        i += 1;
    }
    if in_class {
        return Err(format!("regexFilter {re:?} has an unterminated character class"));
    }
    Ok(())
}

/// Render the skipped list for one log line.
pub fn describe_skipped(skipped: &[Skipped]) -> String {
    let mut s = String::new();
    for sk in skipped {
        match sk.rule_id {
            Some(id) => {
                let _ = write!(s, "rule {id}: {}; ", sk.reason);
            }
            None => {
                let _ = write!(s, "{}; ", sk.reason);
            }
        }
    }
    s.trim_end_matches("; ").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";
    /// `declarativeNetRequest` and `<all_urls>`: every rule may act everywhere.
    const ALL: Grants = Grants { hosts: HostScope::All, host_access_only: false };

    fn grants(permissions: &[&str], hosts: &[&str]) -> Option<Grants> {
        let strings = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        Grants::from_manifest(&strings(permissions), &strings(hosts))
    }

    /// Chrome applies no ruleset without a DNR permission.
    #[test]
    fn rulesets_need_the_dnr_permission() {
        assert_eq!(grants(&[], &["<all_urls>"]), None);
        assert_eq!(grants(&["storage", "tabs"], &["<all_urls>"]), None);
        assert_eq!(grants(&["declarativeNetRequest"], &["<all_urls>"]), Some(ALL));
        assert_eq!(grants(&["declarativeNetRequest"], &["*://*/*"]), Some(ALL));
        let with_host_access = grants(&["declarativeNetRequestWithHostAccess"], &[]).unwrap();
        assert!(with_host_access.host_access_only && with_host_access.hosts == HostScope::Hosts(Vec::new()));
        assert!(!grants(&["declarativeNetRequest", "declarativeNetRequestWithHostAccess"], &[]).unwrap().host_access_only);
    }

    /// Without host permissions an extension may block but not redirect requests or
    /// change their headers (strip CSP, set cookies) on sites it has no access to.
    #[test]
    fn redirect_and_modify_headers_need_host_access() {
        let text = r#"[
          {"id": 1, "action": {"type": "redirect", "redirect": {"url": "https://evil.test/a.js"}}, "condition": {"urlFilter": "||bank.example/app.js", "resourceTypes": ["script"]}},
          {"id": 2, "action": {"type": "modifyHeaders", "responseHeaders": [{"header": "content-security-policy", "operation": "remove"}]}, "condition": {"urlFilter": "*"}},
          {"id": 3, "action": {"type": "block"}, "condition": {"urlFilter": "ads"}}
        ]"#;
        let t = translate(&rules(text), BASE, &grants(&["declarativeNetRequest"], &[]).unwrap());
        let types: Vec<&str> = t.rules.iter().map(|r| r["action"]["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["block"]);
        let mut skipped: Vec<Option<u32>> = t.skipped.iter().map(|s| s.rule_id).collect();
        skipped.sort();
        assert_eq!(skipped, [Some(1), Some(2)]);
    }

    /// With some host permissions, redirect and modifyHeaders rules act on those hosts only.
    #[test]
    fn redirect_scoped_to_host_permissions() {
        let g = grants(&["declarativeNetRequest"], &["https://example.com/*", "*://*.cdn.test/*"]).unwrap();
        let text = r#"[
          {"id": 1, "action": {"type": "redirect", "redirect": {"url": "https://x.test/"}}, "condition": {"urlFilter": "/app.js"}},
          {"id": 2, "action": {"type": "redirect", "redirect": {"url": "https://x.test/"}}, "condition": {"urlFilter": "/app.js", "requestDomains": ["bank.example"]}},
          {"id": 3, "action": {"type": "redirect", "redirect": {"url": "https://x.test/"}}, "condition": {"requestDomains": ["img.cdn.test", "example.com"]}},
          {"id": 4, "action": {"type": "redirect", "redirect": {"url": "https://x.test/"}}, "condition": {"urlFilter": "||example.com/app.js"}},
          {"id": 5, "action": {"type": "block"}, "condition": {"urlFilter": "||bank.example^"}}
        ]"#;
        let t = translate(&rules(text), BASE, &g);
        let filters = |kind: &str| -> Vec<&str> { t.rules.iter().filter(|r| r["action"]["type"] == kind).map(|r| r["trigger"]["url-filter"].as_str().unwrap()).collect() };
        assert_eq!(
            filters("redirect"),
            [
                "^[^:]+://+([^:/]+\\.)?cdn\\.test[:/].*\\/app\\.js",
                "^https://+example\\.com[:/].*\\/app\\.js",
                "^[^:]+://+([^:/]+\\.)?img\\.cdn\\.test[:/]",
                "^https://+example\\.com[:/]",
            ]
        );
        assert_eq!(filters("block").len(), 2, "blocking needs no host permission");
        let mut skipped: Vec<Option<u32>> = t.skipped.iter().map(|s| s.rule_id).collect();
        skipped.sort();
        assert_eq!(skipped, [Some(2), Some(4)], "{:?}", t.skipped);
    }

    /// With only `declarativeNetRequestWithHostAccess`, blocking needs host access too.
    #[test]
    fn host_access_only_limits_every_rule() {
        let g = grants(&["declarativeNetRequestWithHostAccess"], &["*://ads.test/*"]).unwrap();
        let text = r#"[
          {"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "pixel"}},
          {"id": 2, "action": {"type": "allowAllRequests"}, "condition": {"urlFilter": "||ads.test"}}
        ]"#;
        let t = translate(&rules(text), BASE, &g);
        assert_eq!(t.rules.len(), 1);
        assert_eq!(t.rules[0]["trigger"]["url-filter"], "^[^:]+://+ads\\.test[:/].*pixel");
        assert_eq!(t.skipped.iter().map(|s| s.rule_id).collect::<Vec<_>>(), [Some(2)]);
    }

    fn rules(text: &str) -> Vec<Rule> {
        let (rules, skipped) = parse_rules(text).unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        rules
    }

    #[test]
    fn probe_rule_becomes_a_block_rule() {
        let text = include_str!("../../../tests/fixtures/extensions/probe/rules.json");
        let t = translate(&rules(text), BASE, &ALL);
        assert!(t.skipped.is_empty(), "{:?}", t.skipped);
        assert_eq!(t.rules.len(), 1);
        let r = &t.rules[0];
        assert_eq!(r["action"]["type"], "block");
        assert_eq!(r["trigger"]["url-filter"], "\\/vsesvit-blocked\\/");
        assert_eq!(r["trigger"]["resource-type"], json!(["child-document", "fetch", "image", "script"]));
        assert!(r["trigger"].get("url-filter-is-case-sensitive").is_none());
    }

    #[test]
    fn url_filter_grammar() {
        assert_eq!(url_filter_to_regex("||example.com/ads").unwrap(), ["^[^:]+://+([^:/]+\\.)?example\\.com\\/ads"]);
        assert_eq!(url_filter_to_regex("|https://x.test/").unwrap(), ["^https:\\/\\/x\\.test\\/"]);
        assert_eq!(url_filter_to_regex("*/track?id=*|").unwrap(), [".*\\/track\\?id=.*$"]);
        assert_eq!(url_filter_to_regex("abc^def").unwrap(), ["abc[^-.%a-zA-Z0-9_]def"]);
        // A trailing separator is a separator or the end of the URL: two regexes, since
        // WebKit has no alternation.
        assert_eq!(url_filter_to_regex("||ads.test^").unwrap(), ["^[^:]+://+([^:/]+\\.)?ads\\.test[^-.%a-zA-Z0-9_]", "^[^:]+://+([^:/]+\\.)?ads\\.test$"]);
        assert_eq!(url_filter_to_regex("/ad^|").unwrap(), ["\\/ad[^-.%a-zA-Z0-9_]$", "\\/ad$"]);
        assert_eq!(url_filter_to_regex("").unwrap(), [".*"]);
        assert!(url_filter_to_regex("a|b").is_err());
        assert!(url_filter_to_regex("héllo").is_err());
    }

    /// `||tracker.io^` must not block `tracker.iot.com` or `tracker.io.evil.net`.
    #[test]
    fn trailing_separator_ends_the_host() {
        let t = translate(&rules(r#"[{"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "||tracker.io^"}}]"#), BASE, &ALL);
        let filters: Vec<&str> = t.rules.iter().map(|r| r["trigger"]["url-filter"].as_str().unwrap()).collect();
        assert_eq!(filters, ["^[^:]+://+([^:/]+\\.)?tracker\\.io[^-.%a-zA-Z0-9_]", "^[^:]+://+([^:/]+\\.)?tracker\\.io$"]);
        let all = translate(&rules(r#"[{"id": 2, "action": {"type": "allowAllRequests"}, "condition": {"urlFilter": "||trusted.test^"}}]"#), BASE, &ALL);
        assert_eq!(all.rules.len(), 1, "the frame URL condition is a list, so no fan-out: {:?}", all.rules);
        assert_eq!(all.rules[0]["trigger"]["if-top-url"].as_array().map(Vec::len), Some(2));
        let folded = translate(&rules(r#"[{"id": 3, "action": {"type": "block"}, "condition": {"urlFilter": "/ad^", "requestDomains": ["a.test"], "requestMethods": ["get", "post"]}}]"#), BASE, &ALL);
        assert_eq!(folded.rules.len(), 4, "two filters times two methods");
    }

    #[test]
    fn regex_filter_subset() {
        assert!(check_webkit_regex("^https?://[a-z]+\\.example\\.com/.*$").is_ok());
        assert!(check_webkit_regex("[^:]+://x").is_ok());
        for bad in ["a|b", "\\bword", "\\d+", "a{2,3}", "(?:x)", "x^y", "a$b", "(a)\\1", "[abc"] {
            assert!(check_webkit_regex(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn priority_and_action_order() {
        let text = r#"[
          {"id": 1, "priority": 1, "action": {"type": "block"}, "condition": {"urlFilter": "ads"}},
          {"id": 2, "priority": 2, "action": {"type": "allow"}, "condition": {"urlFilter": "ads/ok"}},
          {"id": 3, "priority": 1, "action": {"type": "allow"}, "condition": {"urlFilter": "ads/eq"}},
          {"id": 4, "priority": 3, "action": {"type": "block"}, "condition": {"urlFilter": "ads/strong"}},
          {"id": 5, "priority": 1, "action": {"type": "upgradeScheme"}, "condition": {"urlFilter": "http"}}
        ]"#;
        let t = translate(&rules(text), BASE, &ALL);
        let types: Vec<&str> = t.rules.iter().map(|r| r["action"]["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["block", "ignore-following-rules", "ignore-following-rules", "block", "make-https"]);
        let filters: Vec<&str> = t.rules.iter().map(|r| r["trigger"]["url-filter"].as_str().unwrap()).collect();
        assert_eq!(filters, ["ads\\/strong", "ads\\/ok", "ads\\/eq", "ads", "http"]);
    }

    #[test]
    fn conditions_map_to_trigger_keys() {
        let text = r#"[{
          "id": 7, "action": {"type": "block"},
          "condition": {
            "urlFilter": "pixel", "isUrlFilterCaseSensitive": true, "domainType": "thirdParty",
            "domains": ["Example.com"], "initiatorDomains": ["a.test"],
            "requestMethods": ["POST", "get"], "excludedResourceTypes": ["main_frame", "image"]
          }
        }]"#;
        let t = translate(&rules(text), BASE, &ALL);
        assert!(t.skipped.is_empty(), "{:?}", t.skipped);
        assert_eq!(t.rules.len(), 2, "one WebKit rule per request method");
        let trig = &t.rules[0]["trigger"];
        assert_eq!(trig["url-filter-is-case-sensitive"], true);
        assert_eq!(trig["load-type"], json!(["third-party"]));
        assert!(trig.get("if-domain").is_none(), "{trig}");
        assert_eq!(trig["if-frame-url"], json!(["^[^:]+://+([^:/]+\\.)?example\\.com[:/]", "^[^:]+://+([^:/]+\\.)?a\\.test[:/]"]));
        assert_eq!(trig["request-method"], "post");
        assert_eq!(t.rules[1]["trigger"]["request-method"], "get");
        let types = trig["resource-type"].as_array().unwrap();
        assert!(!types.contains(&json!("top-document")) && !types.contains(&json!("image")));
        assert!(types.contains(&json!("child-document")) && types.contains(&json!("other")));
    }

    /// Chrome lets `excludedDomains` win over `domains`; a WebKit trigger carries either
    /// `if-frame-url` or `unless-frame-url`, so the pair is refused (and logged) like the
    /// initiator pair, never emitted as an `if-frame-url` that also fires on the exclusions.
    #[test]
    fn domains_with_excluded_domains_are_skipped_not_over_blocked() {
        let text = r#"[{
          "id": 3, "action": {"type": "block"},
          "condition": {"urlFilter": "ads", "domains": ["example.com"], "excludedDomains": ["safe.example.com"]}
        }]"#;
        let t = translate(&rules(text), BASE, &ALL);
        assert!(t.rules.is_empty(), "mistranslated: {:?}", t.rules);
        assert_eq!(t.skipped.len(), 1);
        assert_eq!(t.skipped[0].rule_id, Some(3));
        assert!(t.skipped[0].reason.contains("excludedDomains"), "{}", t.skipped[0].reason);
        let alone = translate(&rules(r#"[{"id": 4, "action": {"type": "block"}, "condition": {"urlFilter": "ads", "excludedDomains": ["safe.example.com"]}}]"#), BASE, &ALL);
        assert_eq!(alone.rules[0]["trigger"]["unless-frame-url"], json!(["^[^:]+://+([^:/]+\\.)?safe\\.example\\.com[:/]"]));
    }

    /// WebKit allows one of `if-domain`, `unless-domain`, `if-top-url`, `unless-top-url`,
    /// `if-frame-url` and `unless-frame-url` per trigger and rejects the whole filter
    /// otherwise, so a rule that would need two is translated with one or skipped.
    #[test]
    fn no_trigger_carries_two_conditions() {
        let text = r#"[
          {"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "x", "domains": ["a.test"], "initiatorDomains": ["b.test"]}},
          {"id": 2, "action": {"type": "block"}, "condition": {"urlFilter": "x", "domains": ["a.test"], "excludedInitiatorDomains": ["b.test"]}},
          {"id": 3, "action": {"type": "block"}, "condition": {"urlFilter": "x", "excludedDomains": ["a.test"], "excludedInitiatorDomains": ["b.test"]}},
          {"id": 4, "action": {"type": "allowAllRequests"}, "condition": {"urlFilter": "||trusted.test", "domains": ["a.test"], "resourceTypes": ["main_frame"]}},
          {"id": 5, "action": {"type": "allowAllRequests"}, "condition": {"urlFilter": "||trusted.test", "excludedDomains": ["a.test"], "resourceTypes": ["sub_frame"]}}
        ]"#;
        let t = translate(&rules(text), BASE, &ALL);
        const CONDITIONS: [&str; 6] = ["if-domain", "unless-domain", "if-top-url", "unless-top-url", "if-frame-url", "unless-frame-url"];
        for r in &t.rules {
            let n = CONDITIONS.iter().filter(|k| r["trigger"].get(**k).is_some()).count();
            assert!(n <= 1, "two conditions on one trigger: {r}");
        }
        let mut skipped: Vec<Option<u32>> = t.skipped.iter().map(|s| s.rule_id).collect();
        skipped.sort();
        assert_eq!(skipped, [Some(2), Some(4), Some(5)], "{:?}", t.skipped);
        let both = &t.rules[0]["trigger"];
        assert_eq!(both["if-frame-url"], json!(["^[^:]+://+([^:/]+\\.)?a\\.test[:/]", "^[^:]+://+([^:/]+\\.)?b\\.test[:/]"]));
        assert_eq!(t.rules[1]["trigger"]["unless-frame-url"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn excluded_request_methods_fan_out_to_the_rest() {
        let text = r#"[{"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "x", "excludedRequestMethods": ["get", "HEAD"]}}]"#;
        let t = translate(&rules(text), BASE, &ALL);
        let methods: Vec<&str> = t.rules.iter().map(|r| r["trigger"]["request-method"].as_str().unwrap()).collect();
        assert_eq!(methods, ["connect", "delete", "options", "patch", "post", "put", "trace"]);
        let bad = translate(&rules(r#"[{"id": 1, "action": {"type": "block"}, "condition": {"requestMethods": ["brew"]}}]"#), BASE, &ALL);
        assert!(bad.rules.is_empty() && bad.skipped.len() == 1);
    }

    #[test]
    fn default_resource_types_exclude_main_frame() {
        let text = r#"[{"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "x"}}]"#;
        let t = translate(&rules(text), BASE, &ALL);
        let types = t.rules[0]["trigger"]["resource-type"].as_array().unwrap();
        assert!(!types.contains(&json!("top-document")));
        assert!(types.contains(&json!("script")));
    }

    #[test]
    fn request_domains_fold_into_url_filter() {
        let text = r#"[
          {"id": 1, "action": {"type": "block"}, "condition": {"requestDomains": ["a.test", "b.test"]}},
          {"id": 2, "action": {"type": "block"}, "condition": {"requestDomains": ["a.test"], "urlFilter": "/img/"}},
          {"id": 3, "action": {"type": "block"}, "condition": {"requestDomains": ["a.test"], "urlFilter": "||c.test/"}}
        ]"#;
        let t = translate(&rules(text), BASE, &ALL);
        assert_eq!(t.rules.len(), 3);
        assert_eq!(t.rules[0]["trigger"]["url-filter"], "^[^:]+://+([^:/]+\\.)?a\\.test[:/]");
        assert_eq!(t.rules[1]["trigger"]["url-filter"], "^[^:]+://+([^:/]+\\.)?b\\.test[:/]");
        assert_eq!(t.rules[2]["trigger"]["url-filter"], "^[^:]+://+([^:/]+\\.)?a\\.test[:/].*\\/img\\/");
        assert_eq!(t.skipped.len(), 1);
        assert_eq!(t.skipped[0].rule_id, Some(3));
    }

    #[test]
    fn allow_all_requests_moves_filter_to_frame_url() {
        let text = r#"[{"id": 9, "priority": 5, "action": {"type": "allowAllRequests"},
          "condition": {"urlFilter": "||trusted.test", "resourceTypes": ["main_frame", "sub_frame"]}}]"#;
        let t = translate(&rules(text), BASE, &ALL);
        assert!(t.skipped.is_empty(), "{:?}", t.skipped);
        assert_eq!(t.rules.len(), 2);
        assert_eq!(t.rules[0]["trigger"]["url-filter"], ".*");
        assert_eq!(t.rules[0]["trigger"]["if-top-url"], json!(["^[^:]+://+([^:/]+\\.)?trusted\\.test"]));
        assert_eq!(t.rules[1]["trigger"]["if-frame-url"], json!(["^[^:]+://+([^:/]+\\.)?trusted\\.test"]));
        assert_eq!(t.rules[0]["action"]["type"], "ignore-following-rules");
    }

    #[test]
    fn redirect_and_modify_headers() {
        let text = r#"[
          {"id": 1, "action": {"type": "redirect", "redirect": {"extensionPath": "/empty.js"}}, "condition": {"urlFilter": "tracker.js"}},
          {"id": 2, "action": {"type": "redirect", "redirect": {"transform": {"scheme": "https", "queryTransform": {"removeParams": ["utm_source"]}}}}, "condition": {"urlFilter": "utm_"}},
          {"id": 3, "action": {"type": "modifyHeaders", "requestHeaders": [{"header": "Cookie", "operation": "remove"}], "responseHeaders": [{"header": "X-A", "operation": "set", "value": "1"}]}, "condition": {"urlFilter": "x"}},
          {"id": 4, "action": {"type": "modifyHeaders", "requestHeaders": [{"header": "X", "operation": "set"}]}, "condition": {"urlFilter": "x"}}
        ]"#;
        let t = translate(&rules(text), BASE, &ALL);
        assert_eq!(t.rules[0]["action"]["redirect"]["url"], format!("{BASE}/empty.js"));
        assert_eq!(t.rules[1]["action"]["redirect"]["transform"]["scheme"], "https");
        assert_eq!(t.rules[1]["action"]["redirect"]["transform"]["query-transform"]["remove-parameters"], json!(["utm_source"]));
        assert_eq!(t.rules[2]["action"]["type"], "modify-headers");
        assert_eq!(t.rules[2]["action"]["request-headers"][0]["operation"], "remove");
        assert_eq!(t.rules[2]["action"]["response-headers"][0]["value"], "1");
        assert_eq!(t.skipped.len(), 1);
        assert_eq!(t.skipped[0].rule_id, Some(4));
    }

    #[test]
    fn inexpressible_rules_are_skipped_with_reasons() {
        let text = r#"[
          {"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "x", "tabIds": [1]}},
          {"id": 2, "action": {"type": "block"}, "condition": {"regexFilter": "a|b"}},
          {"id": 3, "action": {"type": "block"}, "condition": {"urlFilter": "x", "excludedRequestDomains": ["a.test"]}},
          {"id": 4, "action": {"type": "block"}, "condition": {"urlFilter": "ok"}}
        ]"#;
        let t = translate(&rules(text), BASE, &ALL);
        assert_eq!(t.rules.len(), 1);
        let ids: Vec<Option<u32>> = t.skipped.iter().map(|s| s.rule_id).collect();
        assert_eq!(ids, [Some(1), Some(2), Some(3)]);
        assert!(describe_skipped(&t.skipped).contains("rule 2: regexFilter"));
    }

    #[test]
    fn malformed_rules_are_reported_not_fatal() {
        let (rules, skipped) = parse_rules(r#"[{"id": 1, "action": {"type": "nope"}, "condition": {}}, {"id": 2, "action": {"type": "block"}, "condition": {}}]"#).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].rule_id, Some(1));
        assert!(parse_rules("{}").is_err());
    }

    #[test]
    fn json_output_is_an_array() {
        let t = translate(&rules(r#"[{"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "x"}}]"#), BASE, &ALL);
        let parsed: Value = serde_json::from_str(&t.to_json()).unwrap();
        assert!(parsed.is_array());
        assert!(translate(&[], BASE, &ALL).is_empty());
    }
}
