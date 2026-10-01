//! Reading bookmarks from other browsers: a bookmarks HTML export (the Netscape format every
//! browser writes), a Chromium `Bookmarks` file, or a Firefox profile's `places.sqlite`.
//!
//! [`installed_browsers`] finds the profiles of browsers on this machine; [`Source::read`] turns
//! one into an [`ImportItem`] tree for [`Bookmarks::import_folder`](crate::bookmarks::Bookmarks::import_folder).
//! Each source's toolbar items come first and unwrapped; its other root folders ("Other
//! bookmarks", Firefox's "Bookmarks Menu") follow as folders. Links with a scheme the browser
//! cannot open (`javascript:`, Firefox's `place:` queries) are dropped.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::Url;
use crate::bookmarks::ImportItem;
use crate::search::NAVIGABLE_SCHEMES;

/// The folder title for an import from a file.
pub const FILE_FOLDER_TITLE: &str = "Imported";

/// How many folders deep a source's tree may nest. Deeper folders in an HTML file give their
/// items to the deepest folder kept; in a Firefox profile they are left out. A Chromium file
/// nests about 60 deep before its JSON is too deep to read.
pub const MAX_FOLDER_DEPTH: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: std::io::Error },
    #[error("not a bookmarks file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("reading Firefox bookmarks: {0}")]
    Db(#[from] rusqlite::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A bookmarks HTML export or a Chromium `Bookmarks` JSON file, told apart by content.
    File(PathBuf),
    /// A Chromium profile's `Bookmarks` file.
    Chromium(PathBuf),
    /// A Firefox profile's `places.sqlite`.
    Firefox(PathBuf),
}

impl Source {
    pub fn read(&self) -> Result<Vec<ImportItem>, ImportError> {
        match self {
            Source::File(path) => {
                let text = read_text(path)?;
                if text.trim_start().starts_with('{') { parse_chromium(&text) } else { Ok(parse_html(&text)) }
            }
            Source::Chromium(path) => parse_chromium(&read_text(path)?),
            Source::Firefox(path) => read_firefox(path),
        }
    }
}

/// A browser profile found on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// "Google Chrome", or "Google Chrome (Work)" when the browser has several profiles.
    pub name: String,
    pub source: Source,
}

impl Found {
    pub fn folder_title(&self) -> String {
        format!("Imported from {}", self.name)
    }
}

/// Profiles of Chromium-family browsers and Firefox on this machine that have bookmarks. Each
/// is read once to leave out empty ones (a few milliseconds each); one that cannot be read
/// stays, so importing it reports why.
pub fn installed_browsers() -> Vec<Found> {
    let Some(dirs) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (name, user_data) in chromium_user_data(&dirs) {
        found.extend(chromium_profiles(name, &user_data));
    }
    for root in firefox_roots(&dirs) {
        found.extend(firefox_profiles(&root));
    }
    found.retain(|f| !matches!(f.source.read(), Ok(items) if items.is_empty()));
    found
}

fn read_text(path: &Path) -> Result<String, ImportError> {
    let bytes = std::fs::read(path).map_err(|source| ImportError::Io { path: path.to_owned(), source })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn link(title: &str, url: &str, added_ms: Option<i64>) -> Option<ImportItem> {
    let url = Url::parse(url.trim()).ok().filter(|u| NAVIGABLE_SCHEMES.contains(&u.scheme()))?;
    Some(ImportItem::Url { title: title.trim().to_owned(), url, added_ms })
}

/// The toolbar's items unwrapped, then each other non-empty root as a folder.
fn assemble(toolbar: Vec<ImportItem>, others: Vec<(String, Vec<ImportItem>)>) -> Vec<ImportItem> {
    let mut items = toolbar;
    for (title, children) in others {
        if !children.is_empty() {
            items.push(ImportItem::Folder { title, children });
        }
    }
    items
}

// ---------------------------------------------------------------------------
// Netscape bookmarks HTML
// ---------------------------------------------------------------------------

/// Parses a bookmarks HTML export: `<DT><H3>` folders each followed by a `<DL>` of children,
/// `<DT><A HREF>` links and `<HR>` separators. Tolerant of unclosed tags; anything else is
/// ignored.
pub fn parse_html(text: &str) -> Vec<ImportItem> {
    struct Frame {
        /// The folder this `<DL>` belongs to; `None` for the outermost list.
        folder: Option<(String, bool)>,
        items: Vec<ImportItem>,
    }
    enum Capture {
        None,
        Link { href: String, added_ms: Option<i64>, title: String },
        Folder { is_toolbar: bool, title: String },
    }

    let mut stack = vec![Frame { folder: None, items: Vec::new() }];
    let mut toolbar = Vec::new();
    let mut pending: Option<(String, bool)> = None;
    // Lists still open past MAX_FOLDER_DEPTH; their items go to the deepest folder kept.
    let mut flattened = 0usize;
    let mut capture = Capture::None;
    let mut rest = text;

    /// Ends the innermost folder. A top-level toolbar folder's items go to `toolbar`, unwrapped.
    fn close(stack: &mut Vec<Frame>, toolbar: &mut Vec<ImportItem>) {
        if stack.len() < 2 {
            return;
        }
        let frame = stack.pop().expect("checked above");
        let (title, is_toolbar) = frame.folder.expect("inner frames belong to a folder");
        if is_toolbar && stack.len() == 1 {
            toolbar.extend(frame.items);
        } else {
            let parent = stack.last_mut().expect("checked above");
            parent.items.push(ImportItem::Folder { title, children: frame.items });
        }
    }

    while let Some(lt) = rest.find('<') {
        let chars = &rest[..lt];
        match &mut capture {
            Capture::Link { title, .. } | Capture::Folder { title, .. } => title.push_str(chars),
            Capture::None => {}
        }
        rest = &rest[lt..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        let Some(gt) = rest.find('>') else {
            break;
        };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        let (closing, tag) = match tag.strip_prefix('/') {
            Some(t) => (true, t),
            None => (false, tag),
        };
        let name_end = tag.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(tag.len());
        let name = tag[..name_end].to_ascii_lowercase();
        let attrs = &tag[name_end..];
        match (closing, name.as_str()) {
            (false, "a") => {
                let added_ms = attr(attrs, "add_date")
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .filter(|&s| s > 0)
                    .and_then(|s| s.checked_mul(1000));
                capture =
                    Capture::Link { href: attr(attrs, "href").unwrap_or_default(), added_ms, title: String::new() };
            }
            (true, "a") => {
                if let Capture::Link { href, added_ms, title } = std::mem::replace(&mut capture, Capture::None)
                    && let Some(item) = link(&decode_entities(&title), &decode_entities(&href), added_ms)
                {
                    stack.last_mut().expect("the outer frame is never popped").items.push(item);
                }
            }
            (false, "h3") => {
                let is_toolbar = attr(attrs, "personal_toolbar_folder").is_some_and(|v| v.eq_ignore_ascii_case("true"));
                capture = Capture::Folder { is_toolbar, title: String::new() };
            }
            (true, "h3") => {
                if let Capture::Folder { is_toolbar, title } = std::mem::replace(&mut capture, Capture::None) {
                    pending = Some((decode_entities(&title).trim().to_owned(), is_toolbar));
                }
            }
            (false, "dt") => {
                // A folder heading with no list after it is an empty folder.
                if let Some((title, _)) = pending.take() {
                    let frame = stack.last_mut().expect("the outer frame is never popped");
                    frame.items.push(ImportItem::Folder { title, children: Vec::new() });
                }
            }
            (false, "dl") => {
                if let Some(folder) = pending.take() {
                    if stack.len() > MAX_FOLDER_DEPTH {
                        flattened += 1;
                    } else {
                        stack.push(Frame { folder: Some(folder), items: Vec::new() });
                    }
                }
            }
            (true, "dl") if flattened > 0 => flattened -= 1,
            (true, "dl") => close(&mut stack, &mut toolbar),
            (false, "hr") => {
                stack.last_mut().expect("the outer frame is never popped").items.push(ImportItem::Separator)
            }
            _ => {}
        }
    }
    while stack.len() > 1 {
        close(&mut stack, &mut toolbar);
    }
    toolbar.append(&mut stack.pop().expect("the outer frame is never popped").items);
    toolbar
}

/// The value of attribute `name` (lowercase) in a tag's attribute text.
fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return None;
        }
        let key_end = rest.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(rest.len());
        let key = &rest[..key_end];
        rest = rest[key_end..].trim_start();
        let value = if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (value, next) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let body = &after[1..];
                    let end = body.find(q).unwrap_or(body.len());
                    (&body[..end], body.get(end + 1..).unwrap_or(""))
                }
                _ => {
                    let end = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..end], &after[end..])
                }
            };
            rest = next;
            value
        } else {
            ""
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(value.to_owned());
        }
    }
}

pub(crate) fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').filter(|&end| end <= 10).and_then(|end| {
            let entity = &rest[1..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .map(|hex| u32::from_str_radix(hex, 16))
                    .or_else(|| entity.strip_prefix('#').map(str::parse::<u32>))
                    .and_then(Result::ok)
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// Chromium
// ---------------------------------------------------------------------------

/// Microseconds between 1601-01-01 (Chromium's epoch) and 1970-01-01.
const CHROMIUM_EPOCH_OFFSET_US: i64 = 11_644_473_600_000_000;

/// Parses a Chromium `Bookmarks` file (`{"roots": {"bookmark_bar", "other", "synced"}}`).
pub fn parse_chromium(json: &str) -> Result<Vec<ImportItem>, ImportError> {
    use serde::de::Error as _;
    let value: serde_json::Value = serde_json::from_str(json)?;
    let roots = value.get("roots").ok_or_else(|| serde_json::Error::custom("no \"roots\""))?;
    let children = |key: &str| roots.get(key).map(chromium_children).unwrap_or_default();
    Ok(assemble(
        children("bookmark_bar"),
        vec![("Other bookmarks".to_owned(), children("other")), ("Mobile bookmarks".to_owned(), children("synced"))],
    ))
}

fn chromium_children(folder: &serde_json::Value) -> Vec<ImportItem> {
    let Some(children) = folder.get("children").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    let text =
        |node: &serde_json::Value, key: &str| node.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_owned();
    children
        .iter()
        .filter_map(|node| match node.get("type").and_then(|t| t.as_str()) {
            Some("url") => {
                let added_ms = node
                    .get("date_added")
                    .and_then(|d| d.as_str())
                    .and_then(|d| d.parse::<i64>().ok())
                    .filter(|&us| us > 0)
                    .map(|us| (us - CHROMIUM_EPOCH_OFFSET_US) / 1000);
                link(&text(node, "name"), &text(node, "url"), added_ms)
            }
            Some("folder") => Some(ImportItem::Folder { title: text(node, "name"), children: chromium_children(node) }),
            _ => None,
        })
        .collect()
}

/// Each Chromium-family browser's user data directories on this OS.
fn chromium_user_data(dirs: &directories::BaseDirs) -> Vec<(&'static str, PathBuf)> {
    if cfg!(windows) {
        let local = dirs.data_local_dir();
        [
            ("Google Chrome", r"Google\Chrome\User Data"),
            ("Microsoft Edge", r"Microsoft\Edge\User Data"),
            ("Brave", r"BraveSoftware\Brave-Browser\User Data"),
            ("Vivaldi", r"Vivaldi\User Data"),
            ("Chromium", r"Chromium\User Data"),
        ]
        .into_iter()
        .map(|(name, dir)| (name, local.join(dir)))
        .collect()
    } else {
        let (home, config) = (dirs.home_dir(), dirs.config_dir());
        [
            ("Google Chrome", config.join("google-chrome")),
            ("Google Chrome", home.join(".var/app/com.google.Chrome/config/google-chrome")),
            ("Microsoft Edge", config.join("microsoft-edge")),
            ("Microsoft Edge", home.join(".var/app/com.microsoft.Edge/config/microsoft-edge")),
            ("Brave", config.join("BraveSoftware/Brave-Browser")),
            ("Brave", home.join(".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser")),
            ("Vivaldi", config.join("vivaldi")),
            ("Chromium", config.join("chromium")),
            ("Chromium", home.join("snap/chromium/common/chromium")),
            ("Chromium", home.join(".var/app/org.chromium.Chromium/config/chromium")),
        ]
        .into_iter()
        .collect()
    }
}

/// The profiles under one user data directory that have a `Bookmarks` file, "Default" first,
/// named from `Local State` when there are several.
fn chromium_profiles(browser: &str, user_data: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(user_data) else {
        return Vec::new();
    };
    let mut profiles: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().join("Bookmarks")))
        .filter(|(_, bookmarks)| bookmarks.is_file())
        .collect();
    profiles.sort_by_key(|(dir, _)| (dir != "Default", dir.clone()));
    let names: HashMap<String, String> = std::fs::read_to_string(user_data.join("Local State"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.pointer("/profile/info_cache").and_then(|c| c.as_object()).cloned())
        .map(|cache| {
            cache.into_iter().filter_map(|(dir, info)| Some((dir, info.get("name")?.as_str()?.to_owned()))).collect()
        })
        .unwrap_or_default();
    let several = profiles.len() > 1;
    profiles
        .into_iter()
        .map(|(dir, bookmarks)| {
            let name =
                if several { format!("{browser} ({})", names.get(&dir).unwrap_or(&dir)) } else { browser.to_owned() };
            Found { name, source: Source::Chromium(bookmarks) }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Firefox
// ---------------------------------------------------------------------------

fn firefox_roots(dirs: &directories::BaseDirs) -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![dirs.config_dir().join(r"Mozilla\Firefox")]
    } else {
        let home = dirs.home_dir();
        vec![
            home.join(".mozilla/firefox"),
            dirs.config_dir().join("mozilla/firefox"),
            home.join("snap/firefox/common/.mozilla/firefox"),
            home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
        ]
    }
}

/// The profiles listed in `<root>/profiles.ini` that have a `places.sqlite`.
fn firefox_profiles(root: &Path) -> Vec<Found> {
    let Ok(ini) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return Vec::new();
    };
    let mut profiles: Vec<(String, PathBuf)> = Vec::new();
    let mut section: Option<(Option<String>, Option<String>, bool)> = None;
    let mut flush = |section: &mut Option<(Option<String>, Option<String>, bool)>| {
        if let Some((name, Some(path), relative)) = section.take() {
            let dir = if relative { root.join(&path) } else { PathBuf::from(&path) };
            let places = dir.join("places.sqlite");
            if places.is_file() {
                profiles.push((name.unwrap_or(path), places));
            }
        }
    };
    for line in ini.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            flush(&mut section);
            if header.starts_with("Profile") {
                section = Some((None, None, true));
            }
        } else if let (Some((name, path, relative)), Some((key, value))) = (&mut section, line.split_once('=')) {
            match key.trim() {
                "Name" => *name = Some(value.trim().to_owned()),
                "Path" => *path = Some(value.trim().to_owned()),
                "IsRelative" => *relative = value.trim() == "1",
                _ => {}
            }
        }
    }
    flush(&mut section);
    let several = profiles.len() > 1;
    profiles
        .into_iter()
        .map(|(profile, places)| {
            let name = if several { format!("Mozilla Firefox ({profile})") } else { "Mozilla Firefox".to_owned() };
            Found { name, source: Source::Firefox(places) }
        })
        .collect()
}

/// Reads a copy of `places.sqlite` (and its WAL), because a running Firefox holds the
/// database with an exclusive lock.
fn read_firefox(places: &Path) -> Result<Vec<ImportItem>, ImportError> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| ImportError::Io { path, source }
    };
    let dir = private_temp_dir().map_err(io(&std::env::temp_dir()))?;
    let result = (|| {
        let copy = dir.join("places.sqlite");
        std::fs::copy(places, &copy).map_err(io(places))?;
        let wal = places.with_file_name("places.sqlite-wal");
        if wal.is_file() {
            std::fs::copy(&wal, dir.join("places.sqlite-wal")).map_err(io(&wal))?;
        }
        let conn = rusqlite::Connection::open(&copy)?;
        firefox_tree(&conn)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// A new directory under the system temp directory that only this user can open. The copy
/// holds the whole browsing history, and on Linux `/tmp` is shared with every other user.
fn private_temp_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("vsesvit-import-{}", uuid::Uuid::new_v4()));
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700).create(&dir)?;
    #[cfg(not(unix))]
    std::fs::create_dir(&dir)?;
    Ok(dir)
}

fn firefox_tree(conn: &rusqlite::Connection) -> Result<Vec<ImportItem>, ImportError> {
    struct Row {
        id: i64,
        kind: i64,
        title: String,
        url: Option<String>,
        added_us: Option<i64>,
    }
    let mut stmt = conn.prepare(
        "SELECT b.id, b.parent, b.type, IFNULL(b.title, ''), p.url, b.dateAdded, b.guid
         FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk
         ORDER BY b.parent, b.position",
    )?;
    let mut children: HashMap<i64, Vec<Row>> = HashMap::new();
    let mut roots: HashMap<String, i64> = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(1)?,
            r.get::<_, String>(6)?,
            Row { id: r.get(0)?, kind: r.get(2)?, title: r.get(3)?, url: r.get(4)?, added_us: r.get(5)? },
        ))
    })?;
    for row in rows {
        let (parent, guid, row) = row?;
        roots.insert(guid, row.id);
        children.entry(parent).or_default().push(row);
    }
    /// A damaged profile's parent links can loop, so a folder already read is skipped.
    fn build(id: i64, children: &HashMap<i64, Vec<Row>>, depth: usize, seen: &mut HashSet<i64>) -> Vec<ImportItem> {
        seen.insert(id);
        let Some(rows) = children.get(&id).filter(|_| depth < MAX_FOLDER_DEPTH) else {
            return Vec::new();
        };
        rows.iter()
            .filter_map(|row| match row.kind {
                1 => link(&row.title, row.url.as_deref()?, row.added_us.filter(|&us| us > 0).map(|us| us / 1000)),
                2 if !seen.contains(&row.id) => {
                    Some(ImportItem::Folder { title: row.title.clone(), children: build(row.id, children, depth + 1, seen) })
                }
                3 => Some(ImportItem::Separator),
                _ => None,
            })
            .collect()
    }
    let mut seen = HashSet::new();
    let mut root = |guid: &str| {
        roots.get(guid).copied().filter(|id| !seen.contains(id)).map(|id| build(id, &children, 0, &mut seen)).unwrap_or_default()
    };
    Ok(assemble(
        root("toolbar_____"),
        vec![
            ("Bookmarks Menu".to_owned(), root("menu________")),
            ("Other Bookmarks".to_owned(), root("unfiled_____")),
            ("Mobile Bookmarks".to_owned(), root("mobile______")),
        ],
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_copy_of_a_firefox_profile_is_private() {
        let dir = super::private_temp_dir().unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(mode, 0o700);
    }
}
