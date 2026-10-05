//! "View page source": the `view-source:` address of a page, and the page that shows a source
//! where the engine has no view-source of its own (WebKitGTK).

use std::fmt::Write;

use crate::Url;
use crate::html::escape;

const PREFIX: &str = "view-source:";

/// Chrome shows the source of http, https and file pages only.
fn has_source(page: &str) -> bool {
    Url::parse(page).is_ok_and(|u| matches!(u.scheme(), "http" | "https" | "file"))
}

/// `view-source:<page>`, or `None` when `page` has no source to show.
pub fn source_url(page: &str) -> Option<String> {
    has_source(page).then(|| format!("{PREFIX}{page}"))
}

/// The page a `view-source:` address shows the source of.
pub fn viewed_url(source: &str) -> Option<&str> {
    let (prefix, page) = source.split_at_checked(PREFIX.len())?;
    (prefix.eq_ignore_ascii_case(PREFIX) && has_source(page)).then_some(page)
}

/// Self-contained HTML showing `source` line by line, numbered like Chrome's view-source. Line
/// numbers cannot be selected, so a copied selection is the source alone.
pub fn source_page(url: &str, source: &str) -> String {
    let text = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut rows = String::new();
    for (i, line) in text.strip_suffix('\n').unwrap_or(&text).split('\n').enumerate() {
        let _ = write!(rows, "<tr><td class=\"n\">{}</td><td>{}</td></tr>", i + 1, escape(line));
    }
    let title = escape(&format!("{PREFIX}{url}"));
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\
         <title>{title}</title><style>\
         :root{{color-scheme:light dark}}\
         body{{margin:0;font-family:monospace}}\
         table{{border-collapse:collapse}}\
         td{{padding:0 5px;white-space:pre;vertical-align:baseline}}\
         td.n{{min-width:31px;padding:0 4px;text-align:right;color:GrayText;border-right:1px solid GrayText;\
         -webkit-user-select:none;user-select:none}}\
         </style></head><body><table><tbody>{rows}</tbody></table></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(page: &str) -> Vec<(&str, &str)> {
        page.split("<tr>")
            .skip(1)
            .map(|row| {
                let row = row.split("</tr>").next().unwrap();
                let number = row.strip_prefix("<td class=\"n\">").unwrap().split("</td>").next().unwrap();
                let line = row.split("</td><td>").nth(1).unwrap().strip_suffix("</td>").unwrap();
                (number, line)
            })
            .collect()
    }

    #[test]
    fn only_http_https_and_file_pages_have_source() {
        for page in ["https://example.com/a?b#c", "http://example.com", "file:///C:/page.html", "HTTPS://example.com/"] {
            assert_eq!(source_url(page), Some(format!("view-source:{page}")), "{page}");
        }
        for page in [
            "about:blank",
            "data:text/html,<p>hi",
            "view-source:https://example.com/",
            "chrome-extension://abc/page.html",
            "javascript:alert(1)",
            "ftp://example.com/",
            "example.com",
            "",
        ] {
            assert_eq!(source_url(page), None, "{page}");
        }
    }

    #[test]
    fn viewed_url_is_the_inverse() {
        for page in ["https://example.com/a?b#c", "file:///home/me/page.html"] {
            assert_eq!(viewed_url(&source_url(page).unwrap()), Some(page));
        }
        assert_eq!(viewed_url("VIEW-SOURCE:http://example.com/"), Some("http://example.com/"));
        for source in [
            "https://example.com/",
            "view-source:about:blank",
            "view-source:view-source:https://example.com/",
            "view-source:",
            "view-sourc",
            "",
        ] {
            assert_eq!(viewed_url(source), None, "{source}");
        }
    }

    #[test]
    fn one_numbered_row_per_line() {
        let page = source_page("https://example.com/", "<!DOCTYPE html>\n\n  <p>hi</p>\n");
        assert_eq!(rows(&page), [("1", "&lt;!DOCTYPE html&gt;"), ("2", ""), ("3", "  &lt;p&gt;hi&lt;/p&gt;")]);
        assert!(page.contains("<title>view-source:https://example.com/</title>"));
        assert!(page.contains("user-select:none") && page.contains("color-scheme:light dark"));
        assert_eq!(rows(&source_page("https://example.com/", "")), [("1", "")]);
        assert_eq!(rows(&source_page("https://example.com/", "a")), [("1", "a")]);
    }

    #[test]
    fn crlf_and_cr_end_lines() {
        let page = source_page("https://example.com/", "a\r\nb\rc\r\n\r\n");
        assert_eq!(rows(&page), [("1", "a"), ("2", "b"), ("3", "c"), ("4", "")]);
    }

    #[test]
    fn page_text_cannot_break_out() {
        let page = source_page(
            "https://example.com/?q=</title><script>alert(1)</script>",
            "</td></tr><script>alert('x')</script>&amp;\"",
        );
        assert!(!page.contains("<script"));
        assert_eq!(
            rows(&page),
            [("1", "&lt;/td&gt;&lt;/tr&gt;&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&amp;amp;&quot;")]
        );
        assert!(page.contains(
            "<title>view-source:https://example.com/?q=&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;</title>"
        ));
    }
}
