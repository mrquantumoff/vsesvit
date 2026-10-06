//! Writing the bookmarks to a bookmarks HTML file, the Netscape format every browser imports
//! and [`import::parse_html`](crate::import::parse_html) reads back. It is laid out as Chrome
//! writes it: the bookmarks bar as the toolbar folder, "Other bookmarks" unwrapped after it, and
//! "Mobile bookmarks" as a folder when it holds anything.

use std::path::PathBuf;

use crate::bookmarks::{BookmarkId, Bookmarks, NodeKind};
use crate::html::escape;

const HEADER: &str = "<!DOCTYPE NETSCAPE-Bookmark-file-1>
<!-- This is an automatically generated file.
     It will be read and overwritten.
     DO NOT EDIT! -->
<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
";

/// Every bookmark as a bookmarks HTML file.
pub fn html(bookmarks: &Bookmarks) -> String {
    let mut out = String::from(HEADER);
    let title = |id| bookmarks.get(id).map(|n| n.title).unwrap_or_default();
    heading(&mut out, 1, &title(BookmarkId::TOOLBAR), 0, " PERSONAL_TOOLBAR_FOLDER=\"true\"");
    list(&mut out, bookmarks, BookmarkId::TOOLBAR, 2);
    line(&mut out, 1, "</DL><p>");
    list(&mut out, bookmarks, BookmarkId::OTHER, 1);
    if !bookmarks.children(BookmarkId::MOBILE).is_empty() {
        heading(&mut out, 1, &title(BookmarkId::MOBILE), 0, "");
        list(&mut out, bookmarks, BookmarkId::MOBILE, 2);
        line(&mut out, 1, "</DL><p>");
    }
    out.push_str("</DL><p>\n");
    out
}

/// The name Chrome suggests for an export made on that day: `bookmarks_10_6_26.html`.
pub fn file_name(year: i32, month: u32, day: u32) -> String {
    format!("bookmarks_{month}_{day}_{:02}.html", year.rem_euclid(100))
}

/// Where the save dialog opens, as in Chrome: the Documents folder, or home without one.
pub fn default_folder() -> Option<PathBuf> {
    let dirs = directories::UserDirs::new()?;
    Some(dirs.document_dir().unwrap_or(dirs.home_dir()).to_owned())
}

/// The descendants of `folder`, `depth` levels in. Walked with a stack of its own, since sync
/// can nest folders deeper than a recursive walk could go.
fn list(out: &mut String, bookmarks: &Bookmarks, folder: BookmarkId, depth: usize) {
    let mut open = vec![bookmarks.children(folder).into_iter()];
    while !open.is_empty() {
        let indent = depth + open.len() - 1;
        let Some(node) = open.last_mut().and_then(Iterator::next) else {
            open.pop();
            if !open.is_empty() {
                line(out, indent - 1, "</DL><p>");
            }
            continue;
        };
        match node.kind {
            NodeKind::Url => {
                let href = node.url.as_ref().map(|u| escape(u.as_str())).unwrap_or_default();
                line(out, indent, &format!("<DT><A HREF=\"{href}\"{}>{}</A>", add_date(node.added_ms), escape(&node.title)));
            }
            NodeKind::Separator => line(out, indent, "<HR>"),
            NodeKind::Folder => {
                heading(out, indent, &node.title, node.added_ms, "");
                open.push(bookmarks.children(node.id).into_iter());
            }
        }
    }
}

/// A folder's heading and the start of its list.
fn heading(out: &mut String, indent: usize, title: &str, added_ms: i64, attrs: &str) {
    line(out, indent, &format!("<DT><H3{}{attrs}>{}</H3>", add_date(added_ms), escape(title)));
    line(out, indent, "<DL><p>");
}

/// Bookmarks HTML dates are in seconds.
fn add_date(added_ms: i64) -> String {
    if added_ms > 0 { format!(" ADD_DATE=\"{}\"", added_ms / 1000) } else { String::new() }
}

fn line(out: &mut String, indent: usize, text: &str) {
    for _ in 0..indent {
        out.push_str("    ");
    }
    out.push_str(text);
    out.push('\n');
}
