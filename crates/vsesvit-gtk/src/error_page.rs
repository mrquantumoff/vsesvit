//! Error pages shown in place of a page that failed to load.
//!
//! They are loaded with `load_alternate_html` under the failing URI, so they run in that
//! page's origin: they carry no script, and every interpolated string is escaped.

use gtk::gio::TlsCertificateFlags;
use url::Url;

pub(crate) fn tls_error(uri: &str, errors: TlsCertificateFlags) -> String {
    let host = host_of(uri);
    let mut reasons: Vec<&str> = TLS_REASONS
        .iter()
        .filter(|(flag, _)| errors.contains(*flag))
        .map(|&(_, reason)| reason)
        .collect();
    if reasons.is_empty() {
        reasons.push("The certificate could not be validated.");
    }
    let mut body = format!(
        "<p>Vsesvit could not confirm that you are connected to <strong>{}</strong>. \
         Someone could be pretending to be this site to steal what you send it.</p><ul>",
        escape(&host)
    );
    for reason in reasons {
        body.push_str(&format!("<li>{}</li>", escape(reason)));
    }
    body.push_str("</ul><p>The page was not loaded.</p>");
    page(
        "Security Warning",
        "This connection is not secure",
        &body,
        uri,
    )
}

pub(crate) fn load_failed(uri: &str, message: &str) -> String {
    let body = format!(
        "<p>Vsesvit could not load <strong>{}</strong>.</p><p class=\"detail\">{}</p>",
        escape(&host_of(uri)),
        escape(message)
    );
    page(
        "Problem Loading Page",
        "Unable to load this page",
        &body,
        uri,
    )
}

pub(crate) fn crashed(uri: &str) -> String {
    let body = format!(
        "<p>The page at <strong>{}</strong> stopped working and was closed.</p>",
        escape(&host_of(uri))
    );
    page("Page Crashed", "This page crashed", &body, uri)
}

const TLS_REASONS: &[(TlsCertificateFlags, &str)] = &[
    (
        TlsCertificateFlags::UNKNOWN_CA,
        "The certificate was not issued by an authority this browser trusts.",
    ),
    (
        TlsCertificateFlags::BAD_IDENTITY,
        "The certificate belongs to a different site.",
    ),
    (
        TlsCertificateFlags::NOT_ACTIVATED,
        "The certificate is not valid yet.",
    ),
    (TlsCertificateFlags::EXPIRED, "The certificate has expired."),
    (
        TlsCertificateFlags::REVOKED,
        "The certificate has been revoked.",
    ),
    (
        TlsCertificateFlags::INSECURE,
        "The certificate uses an insecure algorithm.",
    ),
    (
        TlsCertificateFlags::GENERIC_ERROR,
        "The certificate is not valid.",
    ),
];

fn host_of(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| uri.to_owned())
}

fn page(title: &str, heading: &str, body: &str, retry_uri: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="color-scheme" content="light dark">
<title>{title}</title>
<style>
  :root {{ font-family: system-ui, sans-serif; color: #1e1e1e; background: #fafafb; }}
  @media (prefers-color-scheme: dark) {{ :root {{ color: #eeeeee; background: #222226; }} }}
  body {{ max-width: 36rem; margin: 12vh auto; padding: 0 1.5rem; line-height: 1.5; }}
  h1 {{ font-size: 1.6rem; font-weight: 800; }}
  .detail {{ opacity: 0.7; font-size: 0.9rem; }}
  a.button {{ display: inline-block; margin-top: 1rem; padding: 0.5rem 1.1rem; border-radius: 6px;
             background: #3584e4; color: white; text-decoration: none; font-weight: 700; }}
</style>
</head>
<body>
<h1>{heading}</h1>
{body}
<a class="button" href="{retry}">Try Again</a>
</body>
</html>"#,
        title = escape(title),
        heading = escape(heading),
        retry = escape(retry_uri),
    )
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(
            escape(r#"<a href="x">'&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn tls_page_names_the_host_and_each_problem() {
        let html = tls_error(
            "https://self-signed.example/login",
            TlsCertificateFlags::UNKNOWN_CA | TlsCertificateFlags::EXPIRED,
        );
        assert!(html.contains("<strong>self-signed.example</strong>"));
        assert!(html.contains("not issued by an authority"));
        assert!(html.contains("has expired"));
        assert!(!html.contains("has been revoked"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn tls_page_without_flags_still_explains() {
        let html = tls_error("https://x.example/", TlsCertificateFlags::empty());
        assert!(html.contains("could not be validated"));
    }

    #[test]
    fn hostile_uris_and_messages_are_escaped() {
        let uri = "https://x.example/\"><script>alert(1)</script>";
        let html = load_failed(uri, "<img src=x onerror=alert(1)>");
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img"));
        assert!(html.contains("href=\"https://x.example/&quot;&gt;&lt;script&gt;"));
    }
}
