//! Search suggestions from the default engine as the user types, as in Chrome.
//!
//! [`Omnibox::suggest_request`](crate::search::Omnibox::suggest_request) makes a request on the
//! UI thread, [`SuggestRequest::run`] fetches on a worker thread, and
//! [`Suggestions::add_search_suggestions`](crate::search::Suggestions::add_search_suggestions)
//! lists what it found, back on the UI thread. Every keystroke makes a new request and leaves the earlier ones stale ([`Queries`]): a stale
//! request sends nothing, and a stale result is dropped. What the engine answers is untrusted,
//! so its size is capped and anything but OpenSearch suggestions JSON reads as no suggestions.

use std::sync::{Arc, LazyLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::Url;
use crate::favicons::USER_AGENT;
use crate::search::{NavTarget, SearchEngineId, Suggestion, SuggestionSource, UrlTemplate};

/// Wait after a keystroke before asking the engine, so a word typed quickly costs one request.
pub const DEBOUNCE: Duration = Duration::from_millis(100);
/// Suggestions that arrive later than this are no help to someone typing.
const TIMEOUT: Duration = Duration::from_secs(5);
/// An answer is a few hundred bytes; anything this big is not suggestions.
const MAX_BODY: u64 = 64 * 1024;
/// Suggestions shown at most.
const MAX_ROWS: usize = 4;

/// One for every request, so a keystroke reuses the connection the last one opened.
static AGENT: LazyLock<ureq::Agent> =
    LazyLock::new(|| ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).user_agent(USER_AGENT).build()));

/// One address box's suggestion queries; the newest one wins. Clones share one counter, so a
/// request on a worker thread can tell the user typed again.
#[derive(Clone, Debug, Default)]
pub struct Queries(Arc<AtomicU64>);

impl Queries {
    /// Makes every query asked so far stale: the user typed again, or left the box.
    pub fn cancel(&self) {
        self.next();
    }

    /// Cancels every query asked so far and returns the id of the one asked now.
    pub(crate) fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn is_current(&self, id: u64) -> bool {
        self.0.load(Ordering::Relaxed) == id
    }
}

/// A query for the default engine's suggestions, made by
/// [`Omnibox::suggest_request`](crate::search::Omnibox::suggest_request). `Send`, and holds no
/// profile: [`run`](Self::run) it on a worker thread.
#[derive(Clone, Debug)]
pub struct SuggestRequest {
    pub(crate) engine: SearchEngineId,
    pub(crate) search_url: UrlTemplate,
    pub(crate) url: Url,
    pub(crate) queries: Queries,
    pub(crate) id: u64,
}

impl SuggestRequest {
    /// Blocks: waits [`DEBOUNCE`], then fetches and parses. None when a newer query was asked
    /// first (then nothing is sent at all) or while it fetched. A failed fetch reads as no
    /// suggestions, never an error: the dropdown just has none.
    pub fn run(self) -> Option<SearchSuggestions> {
        std::thread::sleep(DEBOUNCE);
        if !self.queries.is_current(self.id) {
            return None;
        }
        let texts = fetch(&self.url);
        if !self.queries.is_current(self.id) {
            return None;
        }
        Some(SearchSuggestions { rows: rows(&self.engine, &self.search_url, texts), queries: self.queries, id: self.id })
    }
}

/// What the engine suggested for one query, as omnibox rows.
#[derive(Clone, Debug)]
pub struct SearchSuggestions {
    pub(crate) rows: Vec<Suggestion>,
    queries: Queries,
    id: u64,
}

impl SearchSuggestions {
    /// Whether no newer query was asked since: a result that reaches the UI thread after the
    /// user typed again is dropped.
    pub fn is_current(&self) -> bool {
        self.queries.is_current(self.id)
    }
}

/// The suggestions at `url`; none when the engine cannot be reached, answers with an error
/// or too much, or answers something other than suggestions.
fn fetch(url: &Url) -> Vec<String> {
    let body = AGENT.get(url.as_str()).call().and_then(|mut response| {
        let charset = response.body().charset().map(str::to_owned);
        let bytes = response.body_mut().with_config().limit(MAX_BODY).read_to_vec()?;
        Ok(decode(&bytes, charset.as_deref()))
    });
    // The host only: the rest of the URL is what the user typed.
    let host = url.host_str().unwrap_or_default();
    match body.map(|body| parse(&body)) {
        Ok(Some(texts)) => texts,
        Ok(None) => {
            log::debug!("search suggestions from {host}: not OpenSearch suggestions");
            Vec::new()
        }
        Err(e) => {
            log::debug!("search suggestions from {host}: {e}");
            Vec::new()
        }
    }
}

/// `body` as text in `charset`, from its `Content-Type`: UTF-8 unless it says Latin-1, as
/// Google's `client=firefox` endpoint does for non-ASCII suggestions.
fn decode(body: &[u8], charset: Option<&str>) -> String {
    match charset.map(str::to_ascii_lowercase).as_deref() {
        Some("iso-8859-1" | "latin1" | "windows-1252") => body.iter().copied().map(char::from).collect(),
        _ => String::from_utf8_lossy(body).into_owned(),
    }
}

/// The suggestions in OpenSearch suggestions JSON, read as Chrome reads it: an array of the
/// query and the suggested texts, then anything (descriptions, URLs) ignored. Texts are
/// trimmed; blank and repeated ones and other values are left out. None for any other shape.
fn parse(body: &str) -> Option<Vec<String>> {
    let value: Value = serde_json::from_str(body).ok()?;
    let [Value::String(_), Value::Array(found), ..] = value.as_array()?.as_slice() else { return None };
    let mut texts: Vec<String> = Vec::new();
    for text in found.iter().filter_map(Value::as_str).map(str::trim) {
        if texts.len() == MAX_ROWS {
            break;
        }
        if !text.is_empty() && !texts.iter().any(|t| t == text) {
            texts.push(text.to_owned());
        }
    }
    Some(texts)
}

/// `texts` as rows that search `engine`, without any its search URL cannot take.
fn rows(engine: &SearchEngineId, search_url: &UrlTemplate, texts: Vec<String>) -> Vec<Suggestion> {
    texts
        .into_iter()
        .filter_map(|text| {
            let target = NavTarget::Search { engine: engine.clone(), url: search_url.expand(&text)? };
            Some(Suggestion { source: SuggestionSource::SuggestedSearch, title: text.clone(), fill: text, target })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Suggestions;

    #[test]
    fn reads_the_engines_answers() {
        assert_eq!(parse(r#"["rust",["rust lang","rustup"]]"#).unwrap(), ["rust lang", "rustup"], "DuckDuckGo");
        assert_eq!(parse(r#"["rust",["rust lang"],[],{"google:suggesttype":[]}]"#).unwrap(), ["rust lang"], "Google, client=firefox");
        assert_eq!(
            parse(r#"["Rust",["Rust","Rust (programming language)"],["",""],["https://en.wikipedia.org/wiki/Rust","https://en.wikipedia.org/wiki/Rust_(programming_language)"]]"#).unwrap(),
            ["Rust", "Rust (programming language)"],
            "Wikipedia",
        );
        assert_eq!(parse(r#"["other",["a"]]"#).unwrap(), ["a"], "the echoed query need not be the typed text");
        assert_eq!(parse(r#"["q",[]]"#).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn keeps_distinct_non_blank_texts_up_to_the_cap() {
        assert_eq!(parse(r#"["q",[1,"a",null,{"b":1},["c"],"b"]]"#).unwrap(), ["a", "b"]);
        assert_eq!(parse(r#"["q",["  a ","a",""," ","b"]]"#).unwrap(), ["a", "b"]);
        assert_eq!(parse(r#"["q",["a","b","c","d","e","f"]]"#).unwrap(), ["a", "b", "c", "d"]);
    }

    #[test]
    fn anything_else_is_no_suggestions() {
        for body in ["", "not json", r#"["q",["a""#, r#"{"q":["a"]}"#, r#"["q"]"#, r#"[1,["a"]]"#, r#"["q","a"]"#, r#""q""#, "[]"] {
            assert_eq!(parse(body), None, "{body:?}");
        }
    }

    #[test]
    fn decodes_by_the_declared_charset() {
        let latin1 = b"[\"caf\xe9\",[\"caf\xe9 au lait\"]]";
        for charset in ["ISO-8859-1", "iso-8859-1", "latin1", "windows-1252"] {
            assert_eq!(parse(&decode(latin1, Some(charset))).unwrap(), ["caf\u{e9} au lait"], "{charset}");
        }
        let utf8 = "[\"caf\u{e9}\",[\"caf\u{e9} au lait\"]]".as_bytes();
        for charset in [None, Some("utf-8"), Some("UTF-8"), Some("shift_jis")] {
            assert_eq!(parse(&decode(utf8, charset)).unwrap(), ["caf\u{e9} au lait"], "{charset:?}");
        }
        assert_eq!(decode(b"a\xffb", None), "a\u{fffd}b");
    }

    #[test]
    fn rows_search_the_engine_and_skip_a_search_already_listed() {
        let engine = SearchEngineId("e".to_owned());
        let search_url = UrlTemplate("https://e.test/?q={searchTerms}".to_owned());
        let found = rows(&engine, &search_url, vec!["rust".to_owned(), "rust lang".to_owned()]);
        assert_eq!(found[1].source, SuggestionSource::SuggestedSearch);
        assert_eq!((found[1].title.as_str(), found[1].fill.as_str()), ("rust lang", "rust lang"));
        assert_eq!(found[1].target, NavTarget::Search { engine: engine.clone(), url: Url::parse("https://e.test/?q=rust+lang").unwrap() });

        let typed = Suggestion { source: SuggestionSource::Search, title: "Search E for \"rust\"".to_owned(), fill: "rust".to_owned(), target: found[0].target.clone() };
        let mut s = Suggestions { items: vec![typed.clone()], inline: None };
        s.add_search_suggestions(&SearchSuggestions { rows: found.clone(), queries: Queries::default(), id: 0 });
        assert_eq!(s.items, [typed, found[1].clone()], "the typed text's own search is not repeated");
    }
}
