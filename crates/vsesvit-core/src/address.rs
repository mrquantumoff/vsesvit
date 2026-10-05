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
    let Some((host_start, host_end)) = host_span(&parsed) else {
        return decode_escapes(serialized);
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
    Greek,
    Cyrillic,
    Armenian,
    Hebrew,
    Arabic,
    Thaana,
    Devanagari,
    Bengali,
    Gurmukhi,
    Gujarati,
    Oriya,
    Tamil,
    Telugu,
    Kannada,
    Malayalam,
    Sinhala,
    Thai,
    Lao,
    Tibetan,
    Myanmar,
    Georgian,
    Hangul,
    Ethiopic,
    Khmer,
    Hiragana,
    Katakana,
    Bopomofo,
    Han,
    /// Any other script, symbols and emoji: not what names are written in.
    Other,
}

use Script::*;

/// The scripts Unicode recommends for identifiers (UAX #31), by the blocks of their letters.
/// Latin is a-z with Latin-1 and Latin Extended-A and -B only: IPA, Latin Extended Additional
/// and Extended-C to -E are full of letters that imitate a-z (`ɑ`, `ạ`, `ọ`), so they count as
/// [`Other`] and a label that mixes them with a-z keeps its punycode.
const SCRIPTS: &[(char, char, Script)] = &[
    ('a', 'z', Latin),
    ('\u{00DF}', '\u{024F}', Latin),
    ('\u{0370}', '\u{03FF}', Greek),
    ('\u{0400}', '\u{052F}', Cyrillic),
    ('\u{0531}', '\u{058F}', Armenian),
    ('\u{0591}', '\u{05FF}', Hebrew),
    ('\u{0600}', '\u{06FF}', Arabic),
    ('\u{0750}', '\u{077F}', Arabic),
    ('\u{0780}', '\u{07BF}', Thaana),
    ('\u{08A0}', '\u{08FF}', Arabic),
    ('\u{0900}', '\u{097F}', Devanagari),
    ('\u{0980}', '\u{09FF}', Bengali),
    ('\u{0A00}', '\u{0A7F}', Gurmukhi),
    ('\u{0A80}', '\u{0AFF}', Gujarati),
    ('\u{0B00}', '\u{0B7F}', Oriya),
    ('\u{0B80}', '\u{0BFF}', Tamil),
    ('\u{0C00}', '\u{0C7F}', Telugu),
    ('\u{0C80}', '\u{0CFF}', Kannada),
    ('\u{0D00}', '\u{0D7F}', Malayalam),
    ('\u{0D80}', '\u{0DFF}', Sinhala),
    ('\u{0E00}', '\u{0E7F}', Thai),
    ('\u{0E80}', '\u{0EFF}', Lao),
    ('\u{0F00}', '\u{0FFF}', Tibetan),
    ('\u{1000}', '\u{109F}', Myanmar),
    ('\u{10A0}', '\u{10FF}', Georgian),
    ('\u{1100}', '\u{11FF}', Hangul),
    ('\u{1200}', '\u{139F}', Ethiopic),
    ('\u{1780}', '\u{17FF}', Khmer),
    ('\u{19E0}', '\u{19FF}', Khmer),
    ('\u{1C80}', '\u{1C8F}', Cyrillic),
    ('\u{1C90}', '\u{1CBF}', Georgian),
    ('\u{1F00}', '\u{1FFF}', Greek),
    ('\u{2D00}', '\u{2D2F}', Georgian),
    ('\u{2D80}', '\u{2DDF}', Ethiopic),
    ('\u{2DE0}', '\u{2DFF}', Cyrillic),
    ('\u{2E80}', '\u{2FDF}', Han),
    ('\u{3005}', '\u{3007}', Han),
    ('\u{3021}', '\u{3029}', Han),
    ('\u{3038}', '\u{303B}', Han),
    ('\u{3041}', '\u{309F}', Hiragana),
    ('\u{30A0}', '\u{30FF}', Katakana),
    ('\u{3100}', '\u{312F}', Bopomofo),
    ('\u{3130}', '\u{318F}', Hangul),
    ('\u{31A0}', '\u{31BF}', Bopomofo),
    ('\u{31F0}', '\u{31FF}', Katakana),
    ('\u{3400}', '\u{4DBF}', Han),
    ('\u{4E00}', '\u{9FFF}', Han),
    ('\u{A640}', '\u{A69F}', Cyrillic),
    ('\u{A960}', '\u{A97F}', Hangul),
    ('\u{A9E0}', '\u{A9FF}', Myanmar),
    ('\u{AA60}', '\u{AA7F}', Myanmar),
    ('\u{AB00}', '\u{AB2F}', Ethiopic),
    ('\u{AC00}', '\u{D7FF}', Hangul),
    ('\u{F900}', '\u{FAFF}', Han),
    ('\u{FB1D}', '\u{FB4F}', Hebrew),
    ('\u{FB50}', '\u{FDFF}', Arabic),
    ('\u{FE70}', '\u{FEFE}', Arabic),
    ('\u{20000}', '\u{3FFFF}', Han),
];

/// The script of a letter; `None` for digits, the hyphen and marks any script uses.
fn script(c: char) -> Option<Script> {
    match c {
        '0'..='9' | '-' | '\u{00B7}' | '\u{0300}'..='\u{036F}' | '\u{200C}' | '\u{200D}' => None,
        _ => Some(SCRIPTS.iter().find(|(from, to, _)| (*from..=*to).contains(&c)).map_or(Other, |(_, _, s)| *s)),
    }
}

/// Cyrillic, Greek and Armenian letters that look like Latin ones.
const LOOKALIKES: &str = "аеорсухіјѕԁһӏԛԝԍүҽѵѡαεικνορτυχϲϳագզհոսցօ";

/// Latin letters that look like other Latin ones: `ı` like `i`, `ǀ` like `l`, `ĸ` like `k`.
const LATIN_LOOKALIKES: &str = "ıȷǀǁǂǃĸ";

/// Whether a decoded label is safe to show, a short form of Chrome's checks: letters of one
/// script only (or Han with the Japanese, Korean or Chinese script written with it), a script
/// names are written in, and not a label of letters that imitate Latin ones (`аррӏе` for
/// `apple`, `ınstagram`). Anything else, mixed scripts included, keeps its punycode.
fn safe_label(label: &str) -> bool {
    let mut scripts = Vec::new();
    for s in label.chars().filter_map(script) {
        if !scripts.contains(&s) {
            scripts.push(s);
        }
    }
    match scripts[..] {
        [] => true,
        [Other] => false,
        [Latin] => !label.chars().any(|c| LATIN_LOOKALIKES.contains(c)),
        [Cyrillic | Greek | Armenian] => {
            label.chars().filter(|c| script(*c).is_some()).any(|c| !LOOKALIKES.contains(c))
        }
        [_] => true,
        _ => [&[Han, Hiragana, Katakana][..], &[Han, Hangul], &[Han, Bopomofo]]
            .iter()
            .any(|allowed| scripts.iter().all(|s| allowed.contains(s))),
    }
}

/// Decodes runs of percent-escapes that spell non-ASCII UTF-8 text. ASCII escapes stay
/// escaped, as do invalid UTF-8 and characters that are invisible or change text direction
/// (see [`visible`]).
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

/// Not whitespace, a control, a default-ignorable code point (soft hyphen, variation selectors,
/// joiners, tags…), a direction mark, a blank Hangul filler or U+FFFD.
fn visible(c: char) -> bool {
    !c.is_whitespace()
        && !c.is_control()
        && !matches!(c,
            '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}'..='\u{1160}' | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}'
            | '\u{FFA0}' | '\u{FFF0}'..='\u{FFF8}' | '\u{FFFD}' | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}' | '\u{E0000}'..='\u{E0FFF}')
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

/// Whether `host` is `domain` or one of its subdomains.
pub fn covers(domain: &str, host: &str) -> bool {
    host.strip_suffix(domain).is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}
