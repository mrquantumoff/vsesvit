//! Tab search (Chrome's Ctrl+Shift+A): the open tabs of every window and the recently closed
//! ones, narrowed to what the user types, the same in both shells.

use crate::Url;
use crate::search::text_rank;

/// A tab the search can list. `used` orders the tabs, larger first: when the shell last
/// activated an open tab, or closed a closed one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed<K> {
    pub key: K,
    pub title: String,
    pub url: String,
    pub used: u64,
}

/// Which tab a row stands for: one to switch to, or one to reopen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit<O, C> {
    Open(O),
    Closed(C),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row<O, C> {
    pub hit: Hit<O, C>,
    /// The tab's title, or its address while it has none.
    pub title: String,
    /// The second line: the address's host, or the whole address when it has none.
    pub site: String,
}

/// The rows for `query`: open tabs, then closed ones. With no query every tab is listed, the
/// most recently used first. Otherwise only tabs whose title or address has the query, case
/// aside, are, ranked as the omnibox ranks bookmarks (title prefix, then title, then address),
/// most recently used first within a rank.
pub fn rows<O, C>(query: &str, open: Vec<Listed<O>>, closed: Vec<Listed<C>>) -> Vec<Row<O, C>> {
    let needle = query.trim().to_lowercase();
    let mut rows = section(&needle, open, Hit::Open);
    rows.extend(section(&needle, closed, Hit::Closed));
    rows
}

fn section<K, O, C>(needle: &str, mut tabs: Vec<Listed<K>>, hit: impl Fn(K) -> Hit<O, C>) -> Vec<Row<O, C>> {
    tabs.sort_by_key(|tab| std::cmp::Reverse(tab.used));
    let mut ranked: Vec<(u8, Row<O, C>)> = tabs
        .into_iter()
        .filter_map(|tab| {
            let rank = if needle.is_empty() { 0 } else { text_rank(needle, &tab.title.to_lowercase(), &tab.url.to_lowercase())? };
            let site = Url::parse(&tab.url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_else(|| tab.url.clone());
            let title = if tab.title.trim().is_empty() { tab.url } else { tab.title };
            Some((rank, Row { hit: hit(tab.key), title, site }))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(key: u32, title: &str, url: &str, used: u64) -> Listed<u32> {
        Listed { key, title: title.into(), url: url.into(), used }
    }

    fn hits(rows: &[Row<u32, u32>]) -> Vec<Hit<u32, u32>> {
        rows.iter().map(|r| r.hit).collect()
    }

    #[test]
    fn no_query_lists_every_tab_most_recently_used_first() {
        let open = vec![tab(1, "One", "https://one.example/", 5), tab(2, "Two", "https://two.example/", 9)];
        let closed = vec![tab(10, "Gone", "https://gone.example/", 3), tab(11, "Later", "https://later.example/", 7)];
        let found = rows(" ", open, closed);
        assert_eq!(hits(&found), [Hit::Open(2), Hit::Open(1), Hit::Closed(11), Hit::Closed(10)]);
    }

    #[test]
    fn a_query_keeps_matching_tabs_ranked_by_where_it_matches() {
        let open = vec![
            tab(1, "Rust news", "https://news.example/", 1),
            tab(2, "Docs", "https://doc.rust-lang.org/", 3),
            tab(3, "Learn Rust", "https://learn.example/", 2),
            tab(4, "Mail", "https://mail.example/", 4),
            tab(5, "Rustaceans", "https://rustaceans.example/", 5),
        ];
        let found = rows("RUST", open, Vec::new());
        assert_eq!(hits(&found), [Hit::Open(5), Hit::Open(1), Hit::Open(3), Hit::Open(2)]);
    }

    #[test]
    fn closed_tabs_follow_open_ones_even_when_they_match_better() {
        let open = vec![tab(1, "Mail", "https://mail.example/inbox", 1)];
        let closed = vec![tab(10, "Inbox", "https://other.example/", 9)];
        assert_eq!(hits(&rows("inbox", open, closed)), [Hit::Open(1), Hit::Closed(10)]);
    }

    #[test]
    fn a_row_shows_the_title_or_the_address_and_the_host() {
        let open = vec![
            tab(1, "Example", "https://www.example.com/a?b", 3),
            tab(2, "  ", "https://untitled.example/", 2),
            tab(3, "Notes", "file:///home/me/notes.txt", 1),
        ];
        let found: Vec<Row<u32, u32>> = rows("", open, Vec::new());
        let shown: Vec<(&str, &str)> = found.iter().map(|r| (r.title.as_str(), r.site.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Example", "www.example.com"),
                ("https://untitled.example/", "untitled.example"),
                ("Notes", "file:///home/me/notes.txt"),
            ]
        );
    }
}
