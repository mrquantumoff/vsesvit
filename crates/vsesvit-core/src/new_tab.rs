//! The new tab page: a search box for the default engine and tiles for the most visited
//! sites. Shells load [`page`] as HTML at `about:blank`, so a new tab keeps reading as blank
//! everywhere else (empty address bar, "New tab" title, not in session or history).

use std::fmt::Write;

use crate::search::SearchEngine;
use crate::{Error, Profile, Url};

/// Tiles on the page: two rows of four.
pub const TILES: usize = 8;

/// A most-visited tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopSite {
    /// The origin root, `scheme://host[:port]/`.
    pub url: Url,
    /// The host without a leading `www.`.
    pub label: String,
}

impl TopSite {
    /// The site `url` belongs to; `None` unless it is `http` or `https`.
    pub(crate) fn of(url: &Url) -> Option<TopSite> {
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        let host = url.host_str()?;
        let root = Url::parse(&format!("{}/", url.origin().ascii_serialization())).ok()?;
        Some(TopSite { label: host.strip_prefix("www.").unwrap_or(host).to_owned(), url: root })
    }
}

/// The page for a new tab in this profile.
pub fn page(p: &mut Profile) -> Result<String, Error> {
    let sites = p.history().top_sites(TILES)?;
    let engine = p.search_engines().default_engine()?;
    Ok(html(&sites, &engine))
}

/// Self-contained HTML: nothing is fetched but the tiles' favicons.
pub fn html(sites: &[TopSite], engine: &SearchEngine) -> String {
    let placeholder = escape(&format!("Search with {} or enter address", engine.name));
    // No `<` inside <script>: `</script>` would end it, and `<!--` changes where the HTML
    // parser looks for the end.
    let template = serde_json::to_string(&engine.search_url.0).expect("a string serializes").replace('<', "\\u003c");
    let mut tiles = String::new();
    if !sites.is_empty() {
        tiles.push_str("<nav class=\"tiles\">");
        for site in sites {
            let url = escape(site.url.as_str());
            let label = escape(&site.label);
            let letter = escape(&site.label.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default());
            let hue = hue(site.url.host_str().unwrap_or_default());
            let _ = write!(
                tiles,
                "<a class=\"tile\" href=\"{url}\" title=\"{label}\">\
                 <span class=\"icon\" style=\"background:hsl({hue} 45% 42%)\"><span aria-hidden=\"true\">{letter}</span>\
                 <img src=\"{url}favicon.ico\" alt=\"\" onerror=\"this.remove()\"></span>\
                 <span class=\"label\">{label}</span></a>"
            );
        }
        tiles.push_str("</nav>");
    }
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><meta name=\"color-scheme\" content=\"light dark\">\
         <meta name=\"viewport\" content=\"width=device-width\"><title></title><style>{STYLE}</style></head>\n\
         <body><main><form role=\"search\">\
         <input type=\"search\" aria-label=\"{placeholder}\" placeholder=\"{placeholder}\" autocomplete=\"off\" spellcheck=\"false\">\
         </form>{tiles}</main>\n<script>const template = {template};\n{SCRIPT}</script></body></html>\n"
    )
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// FNV-1a, so a site keeps its colour across runs and devices.
fn hue(host: &str) -> u32 {
    let hash = host.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    hash % 360
}

const STYLE: &str = r#"
:root {
  --bg: #f8f9fb; --fg: #1f1f1f; --muted: #5f6368;
  --box: #ffffff; --box-border: #dadce0; --box-shadow: 0 1px 6px rgb(32 33 36 / 0.18);
  --focus: #0b57d0; --tile-hover: rgb(0 0 0 / 0.06); --icon-bg: #eceff3;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #1f1f23; --fg: #e8eaed; --muted: #9aa0a6;
    --box: #2b2c30; --box-border: #3c4043; --box-shadow: 0 1px 6px rgb(0 0 0 / 0.5);
    --focus: #a8c7fa; --tile-hover: rgb(255 255 255 / 0.08); --icon-bg: #3a3b40;
  }
}
* { box-sizing: border-box; }
html, body { height: 100%; margin: 0; }
body {
  background: var(--bg); color: var(--fg);
  font: 14px "Segoe UI Variable Text", "Segoe UI", Cantarell, system-ui, sans-serif;
}
main {
  min-height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center;
  gap: 40px; padding: 16px 16px 12vh;
}
form { width: min(584px, 100%); }
input {
  width: 100%; height: 48px; padding: 0 22px; border-radius: 24px;
  border: 1px solid var(--box-border); background: var(--box); color: inherit;
  font: inherit; font-size: 16px; box-shadow: var(--box-shadow); outline: none;
}
input:focus { border-color: var(--focus); }
input::placeholder { color: var(--muted); }
.tiles { display: flex; flex-wrap: wrap; justify-content: center; max-width: 464px; }
.tile {
  width: 112px; padding: 16px 8px 12px; border-radius: 8px;
  display: flex; flex-direction: column; align-items: center; gap: 10px;
  color: inherit; text-decoration: none;
}
.tile:hover, .tile:focus-visible { background: var(--tile-hover); outline: none; }
.icon {
  position: relative; width: 48px; height: 48px; border-radius: 50%;
  display: grid; place-items: center; color: #fff; font-size: 20px; font-weight: 600;
}
.icon img {
  position: absolute; inset: 0; width: 48px; height: 48px; padding: 12px;
  border-radius: 50%; background: var(--icon-bg);
}
.label { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
"#;

/// An address: a scheme with `//` or `about:`, else a host (`x.yy`, `localhost`, IPv4,
/// `[IPv6]`) with an optional port and path. Anything else is a search.
const SCRIPT: &str = r#"
const withScheme = /^([a-z][a-z0-9+.-]*:\/\/|about:)/i;
const hostLike = /^(localhost|[^\/?#:.]+(\.[^\/?#:.]+)*\.[^\/?#:.\d][^\/?#:.]+|\d{1,3}(\.\d{1,3}){3}|\[[0-9a-f:.]+\])(:\d+)?([\/?#].*)?$/i;
document.querySelector("form").addEventListener("submit", (event) => {
  event.preventDefault();
  const text = document.querySelector("input").value.trim();
  if (!text) return;
  if (withScheme.test(text)) location.href = text;
  else if (!/\s/.test(text) && hostLike.test(text)) location.href = "https://" + text;
  else location.href = template.replaceAll("{searchTerms}", encodeURIComponent(text));
});
"#;
