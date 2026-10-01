//! Escaping for the HTML pages Vsesvit builds itself: the new tab page, error pages and
//! extension background pages.

/// Escapes text for HTML element content and quoted attribute values.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' =>out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}
