//! How the address bar writes a page's URL for people: [`readable_url`] decodes what the
//! engine reports percent-encoded or in punycode, and [`simplified_url`] leaves out what the
//! user does not need to see while not editing.

use crate::Url;

/// `url` with its host and path in readable form, as Chrome and Firefox show it:
/// `https://xn--80ak6aa92e.com/%D0%B2%D0%B8%D0%BA%D0%B8` reads `https://яндекс.com/вики`
/// (or so). Only non-ASCII text is decoded, so an escaped `/`, `?`, `#`, `%` or space keeps its
/// escape and the address still means the same when edited and submitted. A punycode label
/// stays encoded when decoding it could imitate another name (see [`safe_label`]), and escapes
/// that decode to invisible or direction-changing characters stay escaped.
pub fn readable_url(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return url.to_owned();
    };
    let serialized = parsed.as_str();
    let (host_start, host_end) = match (parsed.host_str(), host_span(&parsed)) {
        (Some(_), Some(span)) => span,
        _ => return decode_escapes(serialized),
    };
    let host = readable_host(&serialized[host_start..host_end]);
    format!("{}{}{}", &serialized[..host_start], host, decode_escapes(&serialized[host_end..]))
}

/// A serialized host with its punycode labels decoded where that is safe (see [`readable_url`]).
pub(crate) fn readable_host(host: &str) -> String {
    host.split('.').map(readable_label).collect::<Vec<_>>().join(".")
}

/// Where the host sits in the serialized URL.
fn host_span(url: &Url) -> Option<(usize, usize)> {
    let host = url.host_str()?;
    let after_scheme = url.scheme().len() + "://".len();
    let serialized = url.as_str();
    let rest = serialized.get(after_scheme..)?;
    // After any userinfo, which ends at the last '@' before the path.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let userinfo = rest[..authority_end].rfind('@').map_or(0, |at| at + 1);
    let start = after_scheme + userinfo;
    serialized.get(start..start + host.len()).filter(|h| *h == host).map(|_| (start, start + host.len()))
}

fn readable_label(label: &str) -> String {
    if !label.starts_with("xn--") {
        return label.to_owned();
    }
    let (decoded, result) = idna::domain_to_unicode(label);
    if result.is_ok() && safe_label(&decoded) { decoded } else { label.to_owned() }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Cyrillic,
    Greek,
    Other,
}

fn script(c: char) -> Option<Script> {
    match c {
        '0'..='9' | '-' => None,
        'a'..='z' | '\u{00DF}'..='\u{024F}' => Some(Script::Latin),
        '\u{0370}'..='\u{03FF}' => Some(Script::Greek),
        '\u{0400}'..='\u{052F}' => Some(Script::Cyrillic),
        _ => Some(Script::Other),
    }
}

/// Cyrillic and Greek letters that look like Latin ones.
const LOOKALIKES: &str = "аеорсухіјѕԁһӏԛԝαεικνορτυχ";

/// Whether a decoded label is safe to show: letters of one script only, and not a Cyrillic or
/// Greek label made entirely of letters that look Latin (`аррӏе` for `apple`). Anything else,
/// mixed scripts included, keeps its punycode.
fn safe_label(label: &str) -> bool {
    let mut seen = None;
    for s in label.chars().filter_map(script) {
        match seen {
            None => seen = Some(s),
            Some(first) if first != s => return false,
            Some(_) => {}
        }
    }
    match seen {
        Some(Script::Cyrillic | Script::Greek) => {
            label.chars().filter(|c| script(*c).is_some()).any(|c| !LOOKALIKES.contains(c))
        }
        _ => true,
    }
}

/// Decodes runs of percent-escapes that spell non-ASCII UTF-8 text. ASCII escapes stay
/// escaped, as do invalid UTF-8 and characters that are invisible or change text direction.
fn decode_escapes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            let next = text[i..].find('%').map_or(text.len(), |n| i + n);
            out.push_str(&text[i..next]);
            i = next;
            continue;
        }
        // A run of escapes: decode it as bytes, then keep each character's escapes unless the
        // character is safe to show.
        let start = i;
        let mut run = Vec::new();
        while i + 3 <= bytes.len() && bytes[i] == b'%' {
            match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => run.push(hi << 4 | lo),
                _ => break,
            }
            i += 3;
        }
        if run.is_empty() {
            out.push('%');
            i = start + 1;
            continue;
        }
        let mut at = 0;
        while at < run.len() {
            let len = utf8_len(run[at]);
            let decoded =
                run.get(at..at + len).and_then(|b| std::str::from_utf8(b).ok()).and_then(|s| s.chars().next());
            match decoded {
                Some(c) if !c.is_ascii() && visible(c) => {
                    out.push(c);
                    at += len;
                }
                _ => {
                    out.push_str(&text[start + at * 3..start + at * 3 + 3]);
                    at += 1;
                }
            }
        }
    }
    out
}

fn hex(digit: u8) -> Option<u8> {
    (digit as char).to_digit(16).map(|d| d as u8)
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

fn visible(c: char) -> bool {
    !c.is_whitespace()
        && !c.is_control()
        && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFFD}')
}

/// What the address bar shows for a page while the user is not editing it, unless full URLs
/// are on: an `https` address without its scheme, a leading `www.` or a lone trailing `/`
/// (`https://www.example.com/` reads `example.com`), as Brave and Chrome show it. `http://`
/// stays, so an insecure page never reads like a secure one; other schemes show in full.
///
/// Works on the text as shown (after [`readable_url`], or the engine's own display form), so it
/// never re-encodes what was decoded.
pub fn simplified_url(shown: &str) -> String {
    let Some(rest) = shown.strip_prefix("https://") else {
        return shown.to_owned();
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return shown.to_owned();
    }
    let rest = match rest.strip_prefix("www.") {
        Some(bare) if bare[..authority_end - 4].contains('.') => bare,
        _ => rest,
    };
    match rest.strip_suffix('/') {
        Some(trimmed) if !trimmed.contains(['/', '?', '#']) => trimmed.to_owned(),
        _ => rest.to_owned(),
    }
}
