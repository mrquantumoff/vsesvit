//! Manifest model: the manifest.json fields core and the shells act on, normalized
//! across MV2/MV3 and Chrome/Firefox dialects at parse time. After parsing, the Linux
//! runtime never has to check `manifest_version`. The original JSON is kept in `raw`
//! for anything unmodeled; WebView2 reads the file itself anyway.
//!
//! Normalizations (done once, here):
//! - `browser_action` / `page_action` (MV2) -> `action`
//! - MV2 host patterns inside `permissions` -> `host_permissions`
//! - `background.scripts` / `.page` / `.service_worker` -> [`Background`]
//! - `applications.gecko.id` -> `browser_specific_settings.gecko.id` -> `gecko_id`
//! - `__MSG_name__` in name/description/action title/command descriptions resolved from
//!   `_locales/<ui>/messages.json`, falling back to `_locales/<default_locale>/`
//! - `commands[*].suggested_key` -> this platform's [`Chord`], if Chrome would accept it
//! - every file reference becomes a [`RelPath`] (validated: relative, no `..`, no
//!   backslash), so joining it to the extension dir cannot escape the dir

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::ExtensionId;
use crate::Url;
use crate::shortcuts::{Chord, Key, Mods};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_version: ManifestVersion,
    pub name: String,
    /// 1-4 dot-separated integers without leading zeros, each 0..=65535 (Chrome's
    /// grammar), or 0..=999999999 in an XPI (Firefox's).
    pub version: String,
    pub description: Option<String>,
    pub default_locale: Option<String>,
    pub icons: BTreeMap<u32, RelPath>,
    pub action: Option<Action>,
    pub background: Option<Background>,
    pub content_scripts: Vec<ContentScript>,
    pub permissions: Vec<String>,
    pub host_permissions: Vec<MatchPattern>,
    pub optional_permissions: Vec<String>,
    pub web_accessible_resources: Vec<WebAccessible>,
    /// `declarative_net_request.rule_resources`. The Linux runtime translates these to
    /// WebKit content-blocker JSON.
    pub dnr_rulesets: Vec<DnrRuleset>,
    pub options_page: Option<RelPath>,
    /// base64 SPKI DER. Present for every CRX install (injected).
    pub key: Option<String>,
    pub gecko_id: Option<String>,
    /// In name order. Defaulted because manifests stored before this field have none; reading
    /// such a row derives them from `raw` (see `LoadedRow::into_installed`).
    #[serde(default)]
    pub commands: Vec<ManifestCommand>,
    pub raw: serde_json::Value,
}

/// The commands `chrome.commands.onCommand` reports, and the action commands that open the
/// popup or click the toolbar button instead.
pub const ACTION_COMMANDS: [&str; 3] = ["_execute_action", "_execute_browser_action", "_execute_page_action"];

/// One entry of the manifest's `commands`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestCommand {
    /// The key under `commands`: what `commands.onCommand` passes, or one of [`ACTION_COMMANDS`].
    pub name: String,
    /// Localized; empty when the manifest gives none (allowed for the action commands).
    pub description: String,
    /// The `suggested_key` for this platform, when it is one Chrome accepts and Vsesvit can bind.
    #[serde(default, deserialize_with = "stored_chord")]
    pub suggested_key: Option<Chord>,
}

/// A stored key this build cannot name (a newer build stored it) reads as none, so the rest of
/// the manifest still loads.
fn stored_chord<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Chord>, D::Error> {
    Ok(Value::deserialize(d)?.as_str().and_then(|s| s.parse().ok()))
}

impl ManifestCommand {
    pub fn activates_action(&self) -> bool {
        ACTION_COMMANDS.contains(&self.name.as_str())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestVersion {
    V2,
    V3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub default_popup: Option<RelPath>,
    pub default_title: Option<String>,
    pub default_icon: BTreeMap<u32, RelPath>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Background {
    /// MV3. `module` = `"type": "module"`.
    ServiceWorker {
        script: RelPath,
        module: bool,
    },
    /// MV2 or Firefox MV3 event page. A generated page loads the scripts.
    Scripts {
        scripts: Vec<RelPath>,
        persistent: bool,
    },
    Page {
        page: RelPath,
        persistent: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContentScript {
    pub matches: Vec<MatchPattern>,
    pub exclude_matches: Vec<MatchPattern>,
    pub js: Vec<RelPath>,
    pub css: Vec<RelPath>,
    pub run_at: RunAt,
    pub all_frames: bool,
    pub match_about_blank: bool,
    pub world: World,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
    DocumentIdle,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum World {
    Isolated,
    Main,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebAccessible {
    pub resources: Vec<String>,
    pub matches: Vec<MatchPattern>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DnrRuleset {
    pub id: String,
    pub enabled: bool,
    pub path: RelPath,
}

/// A path inside the extension dir. Validated on construction, so `dir.join(p)`
/// is always inside `dir`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl RelPath {
    pub fn parse(s: &str) -> Result<Self, ManifestError> {
        let trimmed = s.strip_prefix("./").unwrap_or(s);
        let bad = trimmed.is_empty()
            || trimmed.starts_with('/')
            || trimmed.contains('\\')
            || trimmed.contains(':')
            || trimmed.contains('\0')
            || trimmed.split('/').any(|seg| seg == ".." || seg == ".");
        if bad {
            return Err(ManifestError::BadPath(s.to_owned()));
        }
        Ok(RelPath(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn resolve(&self, dir: &Path) -> PathBuf {
        dir.join(&self.0)
    }
}

impl TryFrom<String> for RelPath {
    type Error = ManifestError;
    fn try_from(s: String) -> Result<Self, ManifestError> {
        RelPath::parse(&s)
    }
}

impl From<RelPath> for String {
    fn from(p: RelPath) -> String {
        p.0
    }
}

/// WebExtensions match pattern: `<all_urls>` or `<scheme>://<host><path>` with `*`
/// wildcards. The Linux runtime uses `matches` to decide content-script injection and
/// host permissions.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MatchPattern {
    source: String,
    scheme: SchemeMatch,
    host: HostMatch,
    /// `None` matches any port.
    port: Option<u16>,
    path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SchemeMatch {
    AllUrls,
    /// `*`: http, https, ws, wss.
    Web,
    Exact(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum HostMatch {
    Any,
    /// `*.example.com` matches `example.com` and every subdomain.
    DomainAndSubdomains(String),
    Exact(String),
}

const PATTERN_SCHEMES: &[&str] = &["http", "https", "ws", "wss", "ftp", "file", "urn"];
const ALL_URLS_SCHEMES: &[&str] = &["http", "https", "ws", "wss", "ftp", "file"];

impl MatchPattern {
    pub fn parse(s: &str) -> Result<Self, ManifestError> {
        let bad = || ManifestError::BadPattern(s.to_owned());
        if s == "<all_urls>" {
            return Ok(MatchPattern { source: s.to_owned(), scheme: SchemeMatch::AllUrls, host: HostMatch::Any, port: None, path: "*".into() });
        }
        let (scheme, rest) = s.split_once("://").ok_or_else(bad)?;
        let scheme = match scheme {
            "*" => SchemeMatch::Web,
            known if PATTERN_SCHEMES.contains(&known) => SchemeMatch::Exact(known.to_owned()),
            _ => return Err(bad()),
        };
        let slash = rest.find('/').ok_or_else(bad)?;
        let (host, path) = rest.split_at(slash);
        let is_file = scheme == SchemeMatch::Exact("file".into());
        // As in Chrome, a pattern without a port (or with `*`) matches any port, and an explicit
        // port must be the URL's.
        let (host, port) = match host.rsplit_once(':') {
            Some((h, "*")) => (h, None),
            Some((h, p)) => match p.parse::<u16>() {
                Ok(p) => (h, Some(p)),
                Err(_) => (host, None),
            },
            None => (host, None),
        };
        let host = match host.to_ascii_lowercase() {
            h if h.is_empty() && is_file => HostMatch::Any,
            h if h.is_empty() => return Err(bad()),
            h if h == "*" => HostMatch::Any,
            h => match h.strip_prefix("*.") {
                Some(domain) if !domain.is_empty() && !domain.contains('*') => HostMatch::DomainAndSubdomains(domain.to_owned()),
                Some(_) => return Err(bad()),
                None if h.contains('*') => return Err(bad()),
                None => HostMatch::Exact(h),
            },
        };
        Ok(MatchPattern { source: s.to_owned(), scheme, host, port, path: path.to_owned() })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn matches(&self, url: &Url) -> bool {
        let scheme_ok = match &self.scheme {
            SchemeMatch::AllUrls => ALL_URLS_SCHEMES.contains(&url.scheme()),
            SchemeMatch::Web => matches!(url.scheme(), "http" | "https" | "ws" | "wss"),
            SchemeMatch::Exact(s) => url.scheme() == s,
        };
        if !scheme_ok {
            return false;
        }
        if self.scheme == SchemeMatch::AllUrls {
            return true;
        }
        let host = url.host_str().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
        let host_ok = match &self.host {
            HostMatch::Any => true,
            HostMatch::DomainAndSubdomains(d) => host == *d || host.strip_suffix(d.as_str()).is_some_and(|p| p.ends_with('.')),
            HostMatch::Exact(h) => host == *h,
        };
        if !host_ok || self.port.is_some_and(|p| url.port_or_known_default() != Some(p)) {
            return false;
        }
        let path = match url.query() {
            Some(q) => format!("{}?{}", url.path(), q),
            None => url.path().to_owned(),
        };
        glob_match(self.path.as_bytes(), path.as_bytes())
    }

    /// The URLs this pattern matches that the host permission `grant` covers, as one
    /// pattern; `None` when there are none. A host permission grants whole origins, as in
    /// Chrome, so its path is ignored and the result keeps this pattern's.
    pub fn within(&self, grant: &MatchPattern) -> Option<MatchPattern> {
        use SchemeMatch::{AllUrls, Exact, Web};
        let web = |s: &str| matches!(s, "http" | "https" | "ws" | "wss");
        let scheme = match (&self.scheme, &grant.scheme) {
            (AllUrls, AllUrls) => return Some(self.clone()),
            (AllUrls, other) | (other, AllUrls) => match other {
                Exact(s) if !ALL_URLS_SCHEMES.contains(&s.as_str()) => return None,
                other => other.clone(),
            },
            (Web, Web) => Web,
            (Web, Exact(s)) | (Exact(s), Web) if web(s) => Exact(s.clone()),
            (Exact(a), Exact(b)) if a == b => Exact(a.clone()),
            _ => return None,
        };
        let covers = |domain: &str, host: &str| host == domain || host.strip_suffix(domain).is_some_and(|p| p.ends_with('.'));
        let host = match (&self.host, &grant.host) {
            (HostMatch::Any, other) | (other, HostMatch::Any) => other.clone(),
            (HostMatch::Exact(a), HostMatch::Exact(b)) if a == b => HostMatch::Exact(a.clone()),
            (HostMatch::Exact(h), HostMatch::DomainAndSubdomains(d)) | (HostMatch::DomainAndSubdomains(d), HostMatch::Exact(h)) if covers(d, h) => HostMatch::Exact(h.clone()),
            (HostMatch::DomainAndSubdomains(a), HostMatch::DomainAndSubdomains(b)) if covers(b, a) => HostMatch::DomainAndSubdomains(a.clone()),
            (HostMatch::DomainAndSubdomains(a), HostMatch::DomainAndSubdomains(b)) if covers(a, b) => HostMatch::DomainAndSubdomains(b.clone()),
            _ => return None,
        };
        let port = match (self.port, grant.port) {
            (None, p) | (p, None) => p,
            (Some(a), Some(b)) if a == b => Some(a),
            _ => return None,
        };
        let scheme = match scheme {
            Exact(s) => s,
            _ => "*".to_owned(),
        };
        let host = match host {
            HostMatch::Any if scheme == "file" => String::new(),
            HostMatch::Any => "*".to_owned(),
            HostMatch::DomainAndSubdomains(d) => format!("*.{d}"),
            HostMatch::Exact(h) => h,
        };
        let port = port.map(|p| format!(":{p}")).unwrap_or_default();
        let path = if self.scheme == AllUrls { "/*" } else { &self.path };
        MatchPattern::parse(&format!("{scheme}://{host}{port}{path}")).ok()
    }
}

/// `*` matches any run of characters, everything else matches literally.
fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

impl TryFrom<String> for MatchPattern {
    type Error = ManifestError;
    fn try_from(s: String) -> Result<Self, ManifestError> {
        MatchPattern::parse(&s)
    }
}

impl From<MatchPattern> for String {
    fn from(p: MatchPattern) -> String {
        p.source
    }
}

impl Manifest {
    /// Read `dir/manifest.json` and localize for `ui_locale` (`en-US`, `uk_UA`, `de`):
    /// each `__MSG_key__` comes from the most specific of `_locales/<ui_locale>`,
    /// `_locales/<language>` and `_locales/<default_locale>` that defines `key`.
    pub fn load(dir: &Path, ui_locale: &str) -> Result<Manifest, ManifestError> {
        Manifest::load_with(dir, ui_locale, CHROME_MAX_VERSION_PART)
    }

    /// [`Manifest::load`] for an unpacked XPI, whose `version` follows Firefox's grammar.
    pub(crate) fn load_xpi(dir: &Path, ui_locale: &str) -> Result<Manifest, ManifestError> {
        Manifest::load_with(dir, ui_locale, FIREFOX_MAX_VERSION_PART)
    }

    fn load_with(dir: &Path, ui_locale: &str, max_version_part: u32) -> Result<Manifest, ManifestError> {
        let raw = parse_tolerant_json(&read_text(&dir.join("manifest.json"))?)?;
        let default_locale = raw.get("default_locale").and_then(Value::as_str);
        let catalog = MessageCatalog::load(dir, ui_locale, default_locale);
        Manifest::from_value(raw, max_version_part, &|key| catalog.get(key))
    }

    /// Parse manifest text. `messages(key)` resolves `__MSG_key__`.
    pub fn parse(text: &str, messages: &dyn Fn(&str) -> Option<String>) -> Result<Manifest, ManifestError> {
        Manifest::from_value(parse_tolerant_json(text)?, CHROME_MAX_VERSION_PART, messages)
    }

    /// The SPKI DER in `key`, if the manifest has one.
    pub fn key_der(&self) -> Option<Vec<u8>> {
        self.key.as_deref().and_then(decode_key)
    }

    /// The Chrome id `key` derives to, if the manifest has one.
    pub fn key_id(&self) -> Option<ExtensionId> {
        self.key_der().map(|der| ExtensionId::from_public_key(&der))
    }

    /// The commands of a manifest stored before [`Manifest::commands`] existed, read from
    /// `raw` and localized from `dir` as [`Manifest::load`] localizes.
    pub(crate) fn commands_from_raw(&self, dir: &Path, ui_locale: &str) -> Vec<ManifestCommand> {
        let Some(obj) = self.raw.as_object().filter(|obj| obj.contains_key("commands")) else { return Vec::new() };
        let catalog = MessageCatalog::load(dir, ui_locale, self.default_locale.as_deref());
        parse_commands(obj, self.action.is_some(), &|s| localize(s, &|key| catalog.get(key)))
    }

    fn from_value(raw: Value, max_version_part: u32, messages: &dyn Fn(&str) -> Option<String>) -> Result<Manifest, ManifestError> {
        let obj = raw.as_object().ok_or_else(|| ManifestError::Json("the top level is not an object".into()))?;
        let l10n = |s: &str| localize(s, messages);

        let manifest_version = match obj.get("manifest_version").map(Value::as_u64) {
            Some(Some(2)) => ManifestVersion::V2,
            Some(Some(3)) => ManifestVersion::V3,
            Some(Some(other)) => return Err(ManifestError::UnsupportedVersion(other)),
            _ => return Err(ManifestError::Field("manifest_version")),
        };
        let name = str_field(obj, "name", "name")?.map(l10n).filter(|n| !n.trim().is_empty()).ok_or(ManifestError::Field("name"))?;
        let version = str_field(obj, "version", "version")?
            .filter(|v| is_valid_version(v, max_version_part))
            .ok_or(ManifestError::Field("version"))?;
        let key = str_field(obj, "key", "key")?;
        if key.is_some_and(|k| decode_key(k).is_none()) {
            return Err(ManifestError::Field("key"));
        }

        let action = parse_action(obj, &l10n)?;
        let commands = parse_commands(obj, action.is_some(), &l10n);
        let (permissions, mut host_permissions) = split_permissions(obj.get("permissions"));
        host_permissions.extend(strings(obj.get("host_permissions")).filter_map(|s| MatchPattern::parse(s).ok()));
        let mut seen = std::collections::HashSet::new();
        host_permissions.retain(|p| seen.insert(p.clone()));

        Ok(Manifest {
            manifest_version,
            version: version.to_owned(),
            description: str_field(obj, "description", "description")?.map(l10n),
            default_locale: str_field(obj, "default_locale", "default_locale")?.map(str::to_owned),
            icons: icon_map(obj.get("icons"), "icons")?,
            action,
            background: parse_background(obj.get("background"), manifest_version)?,
            content_scripts: parse_content_scripts(obj.get("content_scripts"))?,
            permissions,
            host_permissions,
            optional_permissions: strings(obj.get("optional_permissions"))
                .chain(strings(obj.get("optional_host_permissions")))
                .map(str::to_owned)
                .collect(),
            web_accessible_resources: parse_web_accessible(obj.get("web_accessible_resources"))?,
            dnr_rulesets: parse_dnr(obj.get("declarative_net_request"))?,
            options_page: parse_options_page(obj)?,
            key: key.map(str::to_owned),
            gecko_id: gecko_id(obj),
            commands,
            name,
            raw,
        })
    }
}

type Object = serde_json::Map<String, Value>;

/// A UTF-8 text file. [`parse_tolerant_json`] drops a leading BOM, as Chrome does.
pub fn read_text(path: &Path) -> Result<String, ManifestError> {
    String::from_utf8(std::fs::read(path)?).map_err(|_| ManifestError::Json(format!("{} is not UTF-8", path.display())))
}

/// `Ok(None)` when absent, `Err(Field(name))` when present with the wrong type.
fn str_field<'a>(obj: &'a Object, key: &str, name: &'static str) -> Result<Option<&'a str>, ManifestError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ManifestError::Field(name)),
    }
}

fn bool_field(obj: &Object, key: &str, name: &'static str) -> Result<Option<bool>, ManifestError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ManifestError::Field(name)),
    }
}

/// String entries of an optional array; anything else is skipped.
fn strings(v: Option<&Value>) -> impl Iterator<Item = &str> {
    v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str)
}

/// A file reference in the manifest. Chrome and Firefox both read a leading `/` as the
/// extension root.
fn manifest_path(s: &str) -> Result<RelPath, ManifestError> {
    RelPath::parse(s.strip_prefix('/').unwrap_or(s)).map_err(|_| ManifestError::BadPath(s.to_owned()))
}

/// Items of an optional array: absent or null is empty, any other type is an error.
fn array<'a>(v: Option<&'a Value>, name: &'static str) -> Result<&'a [Value], ManifestError> {
    match v {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(ManifestError::Field(name)),
    }
}

/// An optional array of strings, each read by `parse`.
fn list<T>(v: Option<&Value>, name: &'static str, parse: impl Fn(&str) -> Result<T, ManifestError>) -> Result<Vec<T>, ManifestError> {
    array(v, name)?.iter().map(|item| item.as_str().ok_or(ManifestError::Field(name)).and_then(&parse)).collect()
}

/// `{"16": "a.png", "48": "b.png"}`.
fn icon_map(v: Option<&Value>, name: &'static str) -> Result<BTreeMap<u32, RelPath>, ManifestError> {
    match v {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(size, path)| {
                let size = size.parse::<u32>().map_err(|_| ManifestError::Field(name))?;
                let path = path.as_str().ok_or(ManifestError::Field(name))?;
                Ok((size, manifest_path(path)?))
            })
            .collect(),
        Some(_) => Err(ManifestError::Field(name)),
    }
}

/// MV2 lists host patterns among API permissions; they move to `host_permissions`.
/// Unknown or malformed entries are skipped, as Chrome skips them with a warning.
fn split_permissions(v: Option<&Value>) -> (Vec<String>, Vec<MatchPattern>) {
    let mut api = Vec::new();
    let mut hosts = Vec::new();
    for s in strings(v) {
        if s == "<all_urls>" || s.contains("://") {
            hosts.extend(MatchPattern::parse(s).ok());
        } else {
            api.push(s.to_owned());
        }
    }
    (api, hosts)
}

/// `action` (MV3), else `browser_action`, else `page_action` (MV2).
fn parse_action(obj: &Object, l10n: &dyn Fn(&str) -> String) -> Result<Option<Action>, ManifestError> {
    let Some(v) = ["action", "browser_action", "page_action"].iter().find_map(|k| obj.get(*k).filter(|v| !v.is_null())) else {
        return Ok(None);
    };
    let a = v.as_object().ok_or(ManifestError::Field("action"))?;
    let default_popup = match str_field(a, "default_popup", "action.default_popup")? {
        None | Some("") => None,
        Some(p) => Some(manifest_path(p)?),
    };
    // A bare icon path counts as size 16, the toolbar size.
    let default_icon = match a.get("default_icon") {
        Some(Value::String(s)) => BTreeMap::from([(16, manifest_path(s)?)]),
        other => icon_map(other, "action.default_icon")?,
    };
    Ok(Some(Action { default_popup, default_title: str_field(a, "default_title", "action.default_title")?.map(l10n), default_icon }))
}

/// Chrome allows an extension this many suggested keys.
const MAX_SUGGESTED_KEYS: usize = 4;

/// `commands`, leniently: an entry that is not an object is skipped, and a `suggested_key`
/// Chrome would refuse is dropped, so `commands` never fails an install. Action commands need
/// an action to activate. Past [`MAX_SUGGESTED_KEYS`], later commands (in name order) keep no
/// suggested key.
fn parse_commands(obj: &Object, has_action: bool, l10n: &dyn Fn(&str) -> String) -> Vec<ManifestCommand> {
    const PLATFORM: &str = if cfg!(windows) { "windows" } else { "linux" };
    let mut commands: Vec<ManifestCommand> = obj
        .get("commands")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, command)| {
            let command = command.as_object()?;
            // A platform key that is present but invalid gives no key, as in Chrome.
            let suggested = match command.get("suggested_key") {
                Some(Value::String(s)) => Some(s.as_str()),
                Some(Value::Object(keys)) => keys.get(PLATFORM).or_else(|| keys.get("default")).and_then(Value::as_str),
                _ => None,
            };
            Some(ManifestCommand {
                name: name.clone(),
                description: command.get("description").and_then(Value::as_str).map(l10n).unwrap_or_default(),
                suggested_key: suggested.and_then(chrome_shortcut),
            })
        })
        .filter(|command| has_action || !command.activates_action())
        .collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    for command in commands.iter_mut().filter(|c| c.suggested_key.is_some()).skip(MAX_SUGGESTED_KEYS) {
        command.suggested_key = None;
    }
    commands
}

/// A `suggested_key` in Chrome's grammar: `+`-separated modifiers and exactly one key, with
/// Ctrl or Alt but not both (Chrome refuses Ctrl+Alt, which is AltGr on many layouts). The keys
/// are letters, digits, Comma, Period, Home, End, PageUp, PageDown, Space, Insert, Delete and the
/// arrows. `Command`, `MacCtrl`, `Search` and the media keys have no meaning here.
pub fn chrome_shortcut(s: &str) -> Option<Chord> {
    use Key::*;
    let mut mods = Mods::NONE;
    let mut key = None;
    for token in s.split('+').map(str::trim) {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" => mods.ctrl = true,
            "alt" => mods.alt = true,
            "shift" => mods.shift = true,
            _ => {
                // Letters and digits are the keys with one-character names.
                let named = Key::from_name(token).filter(|k| {
                    k.name().len() == 1 || matches!(k, Comma | Period | Home | End | PageUp | PageDown | Space | Insert | Delete | Up | Down | Left | Right)
                })?;
                if key.replace(named).is_some() {
                    return None;
                }
            }
        }
    }
    if mods.ctrl == mods.alt {
        return None;
    }
    Chord::new(mods, key?)
}

/// MV3 prefers `service_worker`; cross-browser manifests also list `scripts` for Firefox,
/// which Chrome ignores when a service worker is present. MV2 prefers `scripts`/`page`.
/// `persistent` defaults to true only in MV2.
fn parse_background(v: Option<&Value>, mv: ManifestVersion) -> Result<Option<Background>, ManifestError> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(None) };
    let b = v.as_object().ok_or(ManifestError::Field("background"))?;
    let persistent = bool_field(b, "persistent", "background.persistent")?.unwrap_or(mv == ManifestVersion::V2);
    let module = match str_field(b, "type", "background.type")? {
        None | Some("classic") => false,
        Some("module") => true,
        Some(_) => return Err(ManifestError::Field("background.type")),
    };
    let worker = str_field(b, "service_worker", "background.service_worker")?
        .map(manifest_path)
        .transpose()?
        .map(|script| Background::ServiceWorker { script, module });
    let scripts = Some(list(b.get("scripts"), "background.scripts", manifest_path)?)
        .filter(|s| !s.is_empty())
        .map(|scripts| Background::Scripts { scripts, persistent });
    let page = str_field(b, "page", "background.page")?.map(manifest_path).transpose()?.map(|page| Background::Page { page, persistent });
    Ok(match mv {
        ManifestVersion::V3 => worker.or(scripts).or(page),
        ManifestVersion::V2 => scripts.or(page).or(worker),
    })
}

fn parse_content_scripts(v: Option<&Value>) -> Result<Vec<ContentScript>, ManifestError> {
    array(v, "content_scripts")?
        .iter()
        .map(|item| {
            let cs = item.as_object().ok_or(ManifestError::Field("content_scripts"))?;
            let matches = list(cs.get("matches"), "content_scripts.matches", MatchPattern::parse)?;
            if matches.is_empty() {
                return Err(ManifestError::Field("content_scripts.matches"));
            }
            let run_at = match str_field(cs, "run_at", "content_scripts.run_at")? {
                None | Some("document_idle") => RunAt::DocumentIdle,
                Some("document_start") => RunAt::DocumentStart,
                Some("document_end") => RunAt::DocumentEnd,
                Some(_) => return Err(ManifestError::Field("content_scripts.run_at")),
            };
            let world = match str_field(cs, "world", "content_scripts.world")? {
                None | Some("ISOLATED") => World::Isolated,
                Some("MAIN") => World::Main,
                Some(_) => return Err(ManifestError::Field("content_scripts.world")),
            };
            Ok(ContentScript {
                matches,
                exclude_matches: list(cs.get("exclude_matches"), "content_scripts.exclude_matches", MatchPattern::parse)?,
                js: list(cs.get("js"), "content_scripts.js", manifest_path)?,
                css: list(cs.get("css"), "content_scripts.css", manifest_path)?,
                run_at,
                all_frames: bool_field(cs, "all_frames", "content_scripts.all_frames")?.unwrap_or(false),
                match_about_blank: bool_field(cs, "match_about_blank", "content_scripts.match_about_blank")?.unwrap_or(false),
                world,
            })
        })
        .collect()
}

/// MV2: a list of resource strings, reachable from every page. MV3: a list of
/// `{resources, matches}` objects. Both shapes normalize to [`WebAccessible`].
fn parse_web_accessible(v: Option<&Value>) -> Result<Vec<WebAccessible>, ManifestError> {
    const NAME: &str = "web_accessible_resources";
    let mut out = Vec::new();
    let mut legacy = Vec::new();
    for item in array(v, NAME)? {
        match item {
            Value::String(s) => legacy.push(s.clone()),
            Value::Object(o) => out.push(WebAccessible {
                resources: strings(o.get("resources")).map(str::to_owned).collect(),
                matches: list(o.get("matches"), NAME, MatchPattern::parse)?,
            }),
            _ => return Err(ManifestError::Field(NAME)),
        }
    }
    if !legacy.is_empty() {
        out.insert(0, WebAccessible { resources: legacy, matches: vec![MatchPattern::parse("<all_urls>")?] });
    }
    Ok(out)
}

fn parse_dnr(v: Option<&Value>) -> Result<Vec<DnrRuleset>, ManifestError> {
    const NAME: &str = "declarative_net_request.rule_resources";
    let Some(dnr) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let dnr = dnr.as_object().ok_or(ManifestError::Field("declarative_net_request"))?;
    array(dnr.get("rule_resources"), NAME)?
        .iter()
        .map(|r| {
            let r = r.as_object().ok_or(ManifestError::Field(NAME))?;
            Ok(DnrRuleset {
                id: str_field(r, "id", NAME)?.ok_or(ManifestError::Field(NAME))?.to_owned(),
                enabled: bool_field(r, "enabled", NAME)?.ok_or(ManifestError::Field(NAME))?,
                path: manifest_path(str_field(r, "path", NAME)?.ok_or(ManifestError::Field(NAME))?)?,
            })
        })
        .collect()
}

/// `options_page`, else `options_ui.page`.
fn parse_options_page(obj: &Object) -> Result<Option<RelPath>, ManifestError> {
    if let Some(p) = str_field(obj, "options_page", "options_page")? {
        return manifest_path(p).map(Some);
    }
    match obj.get("options_ui") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(ui)) => str_field(ui, "page", "options_ui.page")?.map(manifest_path).transpose(),
        Some(_) => Err(ManifestError::Field("options_ui")),
    }
}

/// `browser_specific_settings.gecko.id`, else the older spelling `applications.gecko.id`.
fn gecko_id(obj: &Object) -> Option<String> {
    ["browser_specific_settings", "applications"].iter().find_map(|k| obj.get(*k)?.get("gecko")?.get("id")?.as_str()).map(str::to_owned)
}

/// The largest part of a `version` Chrome accepts.
const CHROME_MAX_VERSION_PART: u32 = 65535;
/// Firefox's: up to 9 digits, so AMO serves date versions like `20240101.1`. Its older
/// grammar's letters (`2.0b3`) stay refused, as AMO refuses them now: a version is part
/// of a dir name, and [`cmp_versions`] compares numbers.
const FIREFOX_MAX_VERSION_PART: u32 = 999_999_999;

/// 1 to 4 dot-separated integers in `0..=max_part`, no leading zeros.
fn is_valid_version(v: &str, max_part: u32) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    (1..=4).contains(&parts.len())
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 9
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
                && p.parse::<u32>().is_ok_and(|n| n <= max_part)
        })
}

/// Orders two versions that passed [`is_valid_version`]; missing parts count as 0.
pub(crate) fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> [u32; 4] {
        let mut out = [0; 4];
        for (slot, p) in out.iter_mut().zip(v.split('.')) {
            *slot = p.parse().unwrap_or(0);
        }
        out
    };
    parts(a).cmp(&parts(b))
}

/// base64 SPKI DER. Whitespace and PEM armor are tolerated, as Chromium tolerates them.
fn decode_key(key: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let body: String =
        key.lines().filter(|l| !l.trim_start().starts_with("-----")).flat_map(str::chars).filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(body).ok().filter(|der| !der.is_empty())
}

/// Replaces every `__MSG_key__` that `messages` resolves. Unknown keys stay as written:
/// an unlocalized name is better than refusing the install.
fn localize(s: &str, messages: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("__MSG_") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "__MSG_".len()..];
        match after.find("__").map(|end| (&after[..end], end)) {
            Some((key, end)) if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'@') => {
                match messages(key) {
                    Some(text) => out.push_str(&text),
                    None => out.push_str(&rest[start..start + "__MSG_".len() + end + 2]),
                }
                rest = &after[end + 2..];
            }
            _ => {
                out.push_str("__MSG_");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `_locales` directories to look in, most specific first: `uk_UA`, `uk`, then
/// `default_locale`. `ui_locale` may be spelled `uk-UA`. Each entry is a path component
/// and `default_locale` comes from the manifest, so anything but a plain locale name is
/// dropped.
pub fn locale_chain(ui_locale: &str, default_locale: Option<&str>) -> Vec<String> {
    let ui = ui_locale.replace('-', "_");
    let language = ui.split('_').next().unwrap_or_default().to_owned();
    let mut chain: Vec<String> = Vec::new();
    for locale in [Some(ui), Some(language), default_locale.map(str::to_owned)].into_iter().flatten() {
        let safe = !locale.is_empty() && locale.len() <= 16 && locale.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if safe && !chain.contains(&locale) {
            chain.push(locale);
        }
    }
    chain
}

/// `_locales/<locale>/messages.json` bundles in lookup order, keys lowercased
/// (message names are case-insensitive).
struct MessageCatalog(Vec<BTreeMap<String, String>>);

impl MessageCatalog {
    fn load(dir: &Path, ui_locale: &str, default_locale: Option<&str>) -> Self {
        let bundles = locale_chain(ui_locale, default_locale)
            .iter()
            .filter_map(|locale| read_text(&dir.join("_locales").join(locale).join("messages.json")).ok())
            .filter_map(|text| parse_tolerant_json(&text).ok())
            .map(|v| message_bundle(&v))
            .collect();
        MessageCatalog(bundles)
    }

    fn get(&self, key: &str) -> Option<String> {
        let key = key.to_ascii_lowercase();
        self.0.iter().find_map(|bundle| bundle.get(&key).cloned())
    }
}

/// `{"name": {"message": "Hi $who$", "placeholders": {"who": {"content": "you"}}}}`
/// -> `{"name": "Hi you"}`. `$$` is a literal `$`.
fn message_bundle(v: &Value) -> BTreeMap<String, String> {
    let Some(obj) = v.as_object() else { return BTreeMap::new() };
    obj.iter()
        .filter_map(|(key, entry)| {
            let message = entry.get("message")?.as_str()?;
            let placeholders: BTreeMap<String, &str> = entry
                .get("placeholders")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(name, p)| Some((name.to_ascii_lowercase(), p.get("content")?.as_str()?)))
                .collect();
            Some((key.to_ascii_lowercase(), expand_placeholders(message, &placeholders)))
        })
        .collect()
}

fn expand_placeholders(message: &str, placeholders: &BTreeMap<String, &str>) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(tail) = after.strip_prefix('$') {
            out.push('$');
            rest = tail;
            continue;
        }
        let name_end = after.find('$').filter(|&end| end > 0 && after[..end].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
        match name_end.and_then(|end| Some((placeholders.get(&after[..end].to_ascii_lowercase())?, end))) {
            Some((content, end)) => {
                out.push_str(content);
                rest = &after[end + 1..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Chrome accepts `//` and `/* */` comments and trailing commas in manifest.json and
/// messages.json. Strip them (string-literal aware) before `serde_json`.
pub fn parse_tolerant_json(text: &str) -> Result<serde_json::Value, ManifestError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let cleaned = strip_trailing_commas(&strip_comments(text)?);
    serde_json::from_str(&cleaned).map_err(|e| ManifestError::Json(e.to_string()))
}

fn strip_comments(text: &str) -> Result<String, ManifestError> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                // Keep the newline so serde_json's line numbers still point at the source.
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = '\0';
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && c == '/' {
                        closed = true;
                        break;
                    }
                    prev = c;
                }
                if !closed {
                    return Err(ManifestError::Json("unterminated /* comment".into()));
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    Ok(out)
}

/// Drops a `,` whose next non-whitespace character closes an object or array.
fn strip_trailing_commas(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut pending: Option<String> = None;
    for c in text.chars() {
        if in_string {
            out.push(c);
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        if let Some(buf) = pending.as_mut() {
            if c.is_whitespace() {
                buf.push(c);
                continue;
            }
            let buf = pending.take().unwrap_or_default();
            if c == '}' || c == ']' {
                out.push_str(&buf[1..]);
            } else {
                out.push_str(&buf);
            }
        }
        match c {
            ',' => pending = Some(String::from(",")),
            '"' => {
                in_string = true;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.extend(pending);
    out
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest.json: {0}")]
    Json(String),
    #[error("manifest.json: missing or invalid `{0}`")]
    Field(&'static str),
    #[error("manifest_version {0} is not supported")]
    UnsupportedVersion(u64),
    #[error("invalid file path {0:?}")]
    BadPath(String),
    #[error("invalid match pattern {0:?}")]
    BadPattern(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, url: &str) -> bool {
        MatchPattern::parse(pattern).unwrap().matches(&Url::parse(url).unwrap())
    }

    #[test]
    fn match_patterns() {
        assert!(m("<all_urls>", "https://example.com/a?b"));
        assert!(m("<all_urls>", "file:///tmp/x.html"));
        assert!(!m("<all_urls>", "chrome-extension://abc/popup.html"));
        assert!(m("*://*/*", "http://127.0.0.1:8080/index.html"));
        assert!(!m("*://*/*", "file:///tmp/x"));
        assert!(m("https://*.example.com/*", "https://example.com/"));
        assert!(m("https://*.example.com/*", "https://a.b.example.com/x"));
        assert!(!m("https://*.example.com/*", "https://badexample.com/"));
        assert!(m("https://example.com/foo*", "https://example.com/foobar?q=1"));
        assert!(!m("https://example.com/foo*", "https://example.com/bar"));
        assert!(m("http://localhost:*/*", "http://localhost:3000/x"));
        assert!(m("file:///*", "file:///home/x.html"));
        for bad in ["", "example.com", "https://", "https://*foo.com/", "gopher://x/", "https://a.*.com/"] {
            assert!(MatchPattern::parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn an_explicit_port_must_match_and_none_or_a_star_matches_any() {
        assert!(m("http://localhost:3000/*", "http://localhost:3000/"));
        assert!(!m("http://localhost:3000/*", "http://localhost:8080/"));
        assert!(m("https://example.com:443/*", "https://example.com/"));
        for any in ["http://localhost/*", "http://localhost:*/*"] {
            assert!(m(any, "http://localhost:3000/"), "{any}");
            assert!(m(any, "http://localhost:8080/"), "{any}");
        }
    }

    #[test]
    fn a_pattern_within_a_host_permission_keeps_its_path_on_the_granted_origins() {
        let within = |pattern: &str, grant: &str| MatchPattern::parse(pattern).unwrap().within(&MatchPattern::parse(grant).unwrap()).map(|p| p.as_str().to_owned());
        let some = |s: &str| Some(s.to_owned());
        assert_eq!(within("<all_urls>", "<all_urls>"), some("<all_urls>"));
        assert_eq!(within("<all_urls>", "http://127.0.0.1/*"), some("http://127.0.0.1/*"));
        assert_eq!(within("*://*/*", "https://*.example.com/x"), some("https://*.example.com/*"));
        assert_eq!(within("https://a.example.com/page*", "*://*.example.com/*"), some("https://a.example.com/page*"));
        assert_eq!(within("*://*.example.com/*", "<all_urls>"), some("*://*.example.com/*"));
        assert_eq!(within("*://*.example.com/*", "*://*.a.example.com/*"), some("*://*.a.example.com/*"));
        assert_eq!(within("http://localhost/*", "http://localhost:3000/*"), some("http://localhost:3000/*"));
        assert_eq!(within("file:///home/*", "<all_urls>"), some("file:///home/*"));
        for (pattern, grant) in [
            ("https://a.test/*", "http://a.test/*"),
            ("*://a.test/*", "https://b.test/*"),
            ("*://*.example.com/*", "*://*.other.com/*"),
            ("http://localhost:3000/*", "http://localhost:8080/*"),
            ("urn://x/*", "<all_urls>"),
            ("ftp://x.test/*", "*://*/*"),
        ] {
            assert_eq!(within(pattern, grant), None, "{pattern} within {grant}");
        }
        let pattern = MatchPattern::parse("<all_urls>").unwrap().within(&MatchPattern::parse("https://*.example.com/*").unwrap()).unwrap();
        assert!(pattern.matches(&Url::parse("https://a.example.com/x?y").unwrap()));
        assert!(!pattern.matches(&Url::parse("http://a.example.com/").unwrap()));
    }

    #[test]
    fn rel_paths() {
        assert_eq!(RelPath::parse("./js/a.js").unwrap().as_str(), "js/a.js");
        for bad in ["", "/etc/passwd", "../x", "a/../../x", "a\\b", "C:/x", "a/./b"] {
            assert!(RelPath::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn tolerant_json_strips_comments_and_trailing_commas_outside_strings() {
        let text = "\u{feff}{\n  // line comment\n  \"a\": \"http://x/*y*/,\", /* block\n comment */\n  \"b\": [1, 2, ],\n  \"c\": {\"d\": \"\\\"//\\\"\",},\n}\n";
        let v = parse_tolerant_json(text).unwrap();
        assert_eq!(v, serde_json::json!({"a": "http://x/*y*/,", "b": [1, 2], "c": {"d": "\"//\""}}));
        assert!(parse_tolerant_json("{\"a\": 1 /* never closed").is_err());
        assert!(parse_tolerant_json("{\"a\": }").is_err());
    }

    #[test]
    fn locale_chain_goes_from_region_to_language_to_default_and_drops_unsafe_names() {
        assert_eq!(locale_chain("en_US", Some("de")), ["en_US", "en", "de"]);
        assert_eq!(locale_chain("uk-UA", Some("uk")), ["uk_UA", "uk"]);
        assert_eq!(locale_chain("en", Some("en")), ["en"]);
        assert_eq!(locale_chain("fr", None), ["fr"]);
        assert_eq!(locale_chain("en", Some("../../x")), ["en"]);
        assert_eq!(locale_chain("", Some("en")), ["en"]);
    }

    #[test]
    fn localize_replaces_known_messages_and_keeps_unknown_ones() {
        let messages = |k: &str| (k == "name").then(|| "Probe".to_owned());
        assert_eq!(localize("__MSG_name__", &messages), "Probe");
        assert_eq!(localize("x __MSG_name__ y __MSG_name__", &messages), "x Probe y Probe");
        assert_eq!(localize("__MSG_other__", &messages), "__MSG_other__");
        assert_eq!(localize("__MSG_ unterminated", &messages), "__MSG_ unterminated");
        assert_eq!(localize("plain", &messages), "plain");
    }

    #[test]
    fn message_placeholders_expand() {
        let placeholders = BTreeMap::from([("who".to_owned(), "you")]);
        assert_eq!(expand_placeholders("Hi $WHO$, $$5 $none$", &placeholders), "Hi you, $5 $none$");
    }

    #[test]
    fn array_fields_treat_null_as_empty_and_reject_other_types() {
        let parse = |extra: &str| {
            let text = format!(r#"{{"manifest_version": 2, "name": "A", "version": "1"{extra}}}"#);
            Manifest::parse(&text, &|_| None)
        };
        // Absent and null both read as an empty list.
        for extra in [
            "",
            r#", "content_scripts": null, "web_accessible_resources": null, "declarative_net_request": {"rule_resources": null}, "background": {"scripts": null}"#,
            r#", "declarative_net_request": {}, "background": {}"#,
        ] {
            let m = parse(extra).unwrap();
            assert!(m.content_scripts.is_empty() && m.web_accessible_resources.is_empty() && m.dnr_rulesets.is_empty(), "{extra}");
            assert!(m.background.is_none(), "{extra}");
        }
        for wrong in ["\"a\"", "{}"] {
            for (extra, name) in [
                (format!(r#", "content_scripts": {wrong}"#), "content_scripts"),
                (format!(r#", "web_accessible_resources": {wrong}"#), "web_accessible_resources"),
                (format!(r#", "declarative_net_request": {{"rule_resources": {wrong}}}"#), "declarative_net_request.rule_resources"),
                (format!(r#", "background": {{"scripts": {wrong}}}"#), "background.scripts"),
                (format!(r#", "content_scripts": [{{"matches": {wrong}}}]"#), "content_scripts.matches"),
            ] {
                assert!(matches!(parse(&extra), Err(ManifestError::Field(n)) if n == name), "{extra}");
            }
        }
        let js = |item: &str| parse(&format!(r#", "content_scripts": [{{"matches": ["<all_urls>"], "js": [{item}]}}]"#));
        assert!(matches!(js("1"), Err(ManifestError::Field("content_scripts.js"))));
        assert!(matches!(js("\"../a.js\""), Err(ManifestError::BadPath(p)) if p == "../a.js"));
        assert!(matches!(js("\"nope\"").map(|m| m.content_scripts[0].js[0].as_str().to_owned()), Ok(p) if p == "nope"));
    }

    #[test]
    fn suggested_keys_follow_chromes_grammar() {
        let key = |s: &str| chrome_shortcut(s).map(|c| c.to_string());
        for (written, chord) in [
            ("Ctrl+Shift+Y", "Ctrl+Shift+Y"),
            ("Alt+Shift+P", "Alt+Shift+P"),
            ("Shift+Alt+P", "Alt+Shift+P"),
            ("ctrl+comma", "Ctrl+Comma"),
            ("Alt+0", "Alt+0"),
            ("Ctrl + Period", "Ctrl+Period"),
            ("Ctrl+PageDown", "Ctrl+PageDown"),
            ("Alt+Space", "Alt+Space"),
            ("Ctrl+Insert", "Ctrl+Insert"),
            ("Alt+Left", "Alt+Left"),
            ("Y+Ctrl", "Ctrl+Y"),
        ] {
            assert_eq!(key(written).as_deref(), Some(chord), "{written}");
        }
        for refused in [
            "", "Y", "Shift+Y", "Ctrl+Alt+Y", "Ctrl+Alt+Shift+Y", "Ctrl+Y+Z", "Ctrl+", "Ctrl+F5", "Ctrl+Tab", "Ctrl+Plus",
            "Ctrl+Escape", "Command+Shift+Y", "MacCtrl+Y", "Search+Y", "MediaNextTrack", "Ctrl+MediaPlayPause", "Ctrl+Keypad1",
        ] {
            assert_eq!(key(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn commands_pick_this_platforms_key_in_name_order() {
        let (platform, other) = if cfg!(windows) { ("windows", "linux") } else { ("linux", "windows") };
        let text = format!(
            r#"{{"manifest_version": 3, "name": "A", "version": "1", "action": {{}}, "commands": {{
                "z-string": {{"suggested_key": "Alt+Shift+Z", "description": "Z"}},
                "b-platform": {{"suggested_key": {{"default": "Alt+Shift+D", "{platform}": "Alt+Shift+B", "{other}": "Alt+Shift+O"}}}},
                "c-default": {{"suggested_key": {{"default": "Alt+Shift+C", "{other}": "Alt+Shift+O", "mac": "Command+C"}}}},
                "d-invalid": {{"suggested_key": {{"default": "Alt+Shift+D", "{platform}": "Command+D"}}}},
                "e-none": {{"description": "__MSG_e__"}},
                "f-not-a-string": {{"suggested_key": 5, "description": 5}},
                "g-not-an-object": "Alt+Shift+G",
                "_execute_action": {{"suggested_key": {{"default": "Ctrl+Shift+Y"}}}}
            }}}}"#
        );
        let m = Manifest::parse(&text, &|k| (k == "e").then(|| "Localized".to_owned())).unwrap();
        let commands: Vec<(&str, &str, Option<String>)> =
            m.commands.iter().map(|c| (c.name.as_str(), c.description.as_str(), c.suggested_key.map(|k| k.to_string()))).collect();
        assert_eq!(
            commands,
            [
                ("_execute_action", "", Some("Ctrl+Shift+Y".into())),
                ("b-platform", "", Some("Alt+Shift+B".into())),
                ("c-default", "", Some("Alt+Shift+C".into())),
                ("d-invalid", "", None),
                ("e-none", "Localized", None),
                ("f-not-a-string", "", None),
                ("z-string", "Z", Some("Alt+Shift+Z".into())),
            ]
        );
        assert!(m.commands[0].activates_action() && !m.commands[1].activates_action());
    }

    #[test]
    fn at_most_four_commands_keep_a_suggested_key() {
        let names = ["a", "b", "c", "d", "e", "f"];
        let entries: Vec<String> = names.iter().map(|n| format!(r#""{n}": {{"suggested_key": "Alt+Shift+{}"}}"#, n.to_uppercase())).collect();
        let text = format!(r#"{{"manifest_version": 3, "name": "A", "version": "1", "commands": {{"x": {{}}, {}}}}}"#, entries.join(", "));
        let m = Manifest::parse(&text, &|_| None).unwrap();
        let keyed: Vec<&str> = m.commands.iter().filter(|c| c.suggested_key.is_some()).map(|c| c.name.as_str()).collect();
        assert_eq!(keyed, ["a", "b", "c", "d"]);
        assert_eq!(m.commands.len(), 7, "the commands themselves stay");
    }

    #[test]
    fn action_commands_need_an_action_and_commands_never_fail_the_install() {
        let parse = |extra: &str| {
            Manifest::parse(&format!(r#"{{"manifest_version": 2, "name": "A", "version": "1"{extra}}}"#), &|_| None).unwrap().commands
        };
        let commands = r#", "commands": {"_execute_browser_action": {}, "_execute_page_action": {}, "_execute_action": {}, "run": {}}"#;
        let names = |c: Vec<ManifestCommand>| c.into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(parse(commands)), ["run"]);
        assert_eq!(names(parse(&format!(r#"{commands}, "browser_action": {{}}"#))), ["_execute_action", "_execute_browser_action", "_execute_page_action", "run"]);
        for odd in [r#", "commands": []"#, r#", "commands": "x""#, r#", "commands": null"#, ""] {
            assert!(parse(odd).is_empty(), "{odd}");
        }
    }

    #[test]
    fn manifests_stored_without_commands_still_read() {
        let mut stored = serde_json::to_value(Manifest::parse(r#"{"manifest_version": 3, "name": "A", "version": "1"}"#, &|_| None).unwrap()).unwrap();
        stored.as_object_mut().unwrap().remove("commands");
        assert!(serde_json::from_value::<Manifest>(stored).unwrap().commands.is_empty());

        let stored = r#"[{"name": "a", "description": "", "suggested_key": "Ctrl+MediaPlay"}, {"name": "b", "description": "", "suggested_key": "Alt+Shift+B"}]"#;
        let keys: Vec<Option<String>> =
            serde_json::from_str::<Vec<ManifestCommand>>(stored).unwrap().iter().map(|c| c.suggested_key.map(|k| k.to_string())).collect();
        assert_eq!(keys, [None, Some("Alt+Shift+B".to_owned())], "a key a newer build stored reads as none");
    }

    #[test]
    fn versions_follow_chrome_or_firefox_grammar() {
        for good in ["1", "1.0", "1.2.3.4", "0.0.0.0", "65535.1"] {
            assert!(is_valid_version(good, CHROME_MAX_VERSION_PART), "{good}");
            assert!(is_valid_version(good, FIREFOX_MAX_VERSION_PART), "{good}");
        }
        for bad in ["", "1.", ".1", "1..2", "1.2.3.4.5", "1.02", "01", "1.0a", "2.0b3", "1.-1", " 1", "1000000000"] {
            assert!(!is_valid_version(bad, CHROME_MAX_VERSION_PART), "{bad:?}");
            assert!(!is_valid_version(bad, FIREFOX_MAX_VERSION_PART), "{bad:?}");
        }
        for firefox_only in ["65536", "20240101.1", "2024.10.15.999999999"] {
            assert!(!is_valid_version(firefox_only, CHROME_MAX_VERSION_PART), "{firefox_only}");
            assert!(is_valid_version(firefox_only, FIREFOX_MAX_VERSION_PART), "{firefox_only}");
        }
        assert_eq!(cmp_versions("20240102.0", "20240101.9"), std::cmp::Ordering::Greater);
        assert_eq!(cmp_versions("1.2", "1.2.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(cmp_versions("1.10", "1.9"), std::cmp::Ordering::Greater);
        assert_eq!(cmp_versions("2", "10"), std::cmp::Ordering::Less);
    }
}
