//! The new tab page: a search box for the default engine and tiles for the most visited
//! sites. Shells load [`page`] as HTML at `about:blank`, so a new tab keeps reading as blank
//! everywhere else (empty address bar, "New tab" title, not in session or history).
//!
//! A private window's new tab is [`PRIVATE_PAGE`] instead, as Chrome's incognito one: what
//! private browsing does and does not do, with no search box and no tiles, which would show
//! the normal windows' history and fetch its favicons.

use std::fmt::Write;

use crate::html::escape;
use crate::private::Browsing;
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

/// The page for a new tab in a window of `browsing`'s kind.
pub fn page(p: &mut Profile, browsing: Browsing) -> Result<String, Error> {
    if browsing == Browsing::Private {
        return Ok(PRIVATE_PAGE.to_owned());
    }
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

/// Self-contained HTML, dark whatever the theme as Chrome's incognito page is, in Chrome's
/// words plus the site choices a private session keeps until it ends.
pub const PRIVATE_PAGE: &str = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="color-scheme" content="dark">
<meta name="viewport" content="width=device-width"><title></title><style>
:root { --bg: #1f1f23; --fg: #e8eaed; --muted: #9aa0a6; --card: #2b2c30; }
* { box-sizing: border-box; }
html, body { height: 100%; margin: 0; }
body {
  background: var(--bg); color: var(--fg);
  font: 14px "Segoe UI Variable Text", "Segoe UI", Cantarell, system-ui, sans-serif; line-height: 1.5;
}
main { max-width: 640px; margin: 0 auto; padding: 14vh 24px 48px; }
h1 { font-size: 24px; font-weight: 600; margin: 0 0 12px; }
p { margin: 0 0 24px; color: var(--muted); }
.lists { display: flex; flex-wrap: wrap; gap: 16px; }
section { flex: 1 1 260px; padding: 16px 20px; border-radius: 12px; background: var(--card); }
h2 { font-size: 14px; font-weight: 600; margin: 0 0 8px; }
ul { margin: 0; padding-left: 20px; color: var(--muted); }
</style></head>
<body><main>
<h1>You're browsing privately</h1>
<p>Others who use this device won't see your activity. Downloads and bookmarks are still saved.</p>
<div class="lists">
<section><h2>Once you close all private windows, Vsesvit won't save</h2><ul>
<li>Your browsing history</li>
<li>Cookies and site data</li>
<li>Choices you make for sites, such as permissions and zoom</li>
</ul></section>
<section><h2>Your activity might still be visible to</h2><ul>
<li>Websites you visit</li>
<li>Your employer or school</li>
<li>Your internet service provider</li>
</ul></section>
</div>
</main></body></html>
"#;

/// An address: a scheme with `//` or `about:`, else a host (`x.yy`, `localhost`, IPv4,
/// `[IPv6]`) with an optional port and path, opened over `https://` (`http://` for localhost
/// and IP literals, as [`crate::search::classify`] does). Anything else is a search.
const SCRIPT: &str = r#"
const withScheme = /^([a-z][a-z0-9+.-]*:\/\/|about:)/i;
const hostLike = /^(localhost|[^\/?#:.]+(\.[^\/?#:.]+)*\.[^\/?#:.\d][^\/?#:.]+|\d{1,3}(\.\d{1,3}){3}|\[[0-9a-f:.]+\])(:\d+)?([\/?#].*)?$/i;
const localHost = /^(localhost|\d{1,3}(\.\d{1,3}){3}|\[.*\])$/i;
document.querySelector("form").addEventListener("submit", (event) => {
  event.preventDefault();
  const text = document.querySelector("input").value.trim();
  if (!text) return;
  const host = !/\s/.test(text) && text.match(hostLike);
  if (withScheme.test(text)) location.href = text;
  else if (host) location.href = (localHost.test(host[1]) ? "http://" : "https://") + text;
  else location.href = template.replaceAll("{searchTerms}", encodeURIComponent(text));
});
"#;
