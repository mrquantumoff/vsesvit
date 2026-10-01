//! HTML escaping for the pages the shells build: the new tab page, error pages and
//! extension background pages.

use vsesvit_core::html::escape;

#[test]
fn escapes_markup_and_keeps_other_text() {
    assert_eq!(escape(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    assert_eq!(escape("Привіт, світ"), "Привіт, світ");
    assert_eq!(escape(""), "");
}
