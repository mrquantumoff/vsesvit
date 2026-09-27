//! The new tab page: markup escaping, the embedded search template, the empty state.

use vsesvit_core::history::Transition;
use vsesvit_core::new_tab::{self, TopSite};
use vsesvit_core::search::{SearchEngine, SearchEngineId, UrlTemplate};
use vsesvit_core::{OpenOptions, Profile, Url};

fn engine(name: &str, template: &str) -> SearchEngine {
    SearchEngine {
        id: SearchEngineId("custom:test".to_owned()),
        name: name.to_owned(),
        keyword: None,
        search_url: UrlTemplate(template.to_owned()),
        suggest_url: None,
        builtin: false,
    }
}

fn site(url: &str, label: &str) -> TopSite {
    TopSite { url: Url::parse(url).unwrap(), label: label.to_owned() }
}

#[test]
fn tiles_link_to_origins_with_favicons() {
    let html = new_tab::html(&[site("https://a.example/", "a.example"), site("http://localhost:8080/", "localhost")], &engine("DuckDuckGo", "https://duckduckgo.com/?q={searchTerms}"));
    assert_eq!(html.matches("class=\"tile\"").count(), 2);
    assert!(html.contains("href=\"https://a.example/\""));
    assert!(html.contains("src=\"http://localhost:8080/favicon.ico\""));
    assert!(html.contains("placeholder=\"Search with DuckDuckGo or enter address\""));
    assert!(html.contains("const template = \"https://duckduckgo.com/?q={searchTerms}\";"));
    assert!(html.contains("<title></title>"), "the shell's own new tab title applies");
}

#[test]
fn no_tiles_section_without_sites() {
    let html = new_tab::html(&[], &engine("DuckDuckGo", "https://duckduckgo.com/?q={searchTerms}"));
    assert!(!html.contains("class=\"tiles\""));
    assert!(!html.contains("class=\"tile\""));
}

#[test]
fn hostile_strings_are_escaped() {
    let hostile = "\"><script>alert(1)</script>";
    let template = "https://e.example/?q={searchTerms}&x=</script><script>alert(2)</script>\"";
    let html = new_tab::html(&[site("https://a.example/", hostile)], &engine(hostile, template));
    assert_eq!(html.matches("<script>").count(), 1, "only the page's own script");
    assert_eq!(html.matches("</script>").count(), 1);
    assert!(html.contains("title=\"&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;\""));
    assert!(html.contains("placeholder=\"Search with &quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt; or enter address\""));
    assert!(html.contains(r#"const template = "https://e.example/?q={searchTerms}&x=\u003c/script>\u003cscript>alert(2)\u003c/script>\"";"#));
}

#[test]
fn page_reads_history_and_the_default_engine() {
    let dir = std::env::temp_dir().join(format!("vsesvit-ntp-{}", uuid::Uuid::new_v4()));
    let mut p = Profile::open(&dir, OpenOptions::default()).unwrap();
    assert!(!new_tab::page(&mut p).unwrap().contains("class=\"tiles\""));
    p.history().record_visit(&Url::parse("https://www.example.com/a").unwrap(), Transition::Link).unwrap();
    let html = new_tab::page(&mut p).unwrap();
    assert!(html.contains("href=\"https://www.example.com/\" title=\"example.com\""));
    assert!(html.contains("Search with DuckDuckGo or enter address"));
    drop(p);
    let _ = std::fs::remove_dir_all(&dir);
}
