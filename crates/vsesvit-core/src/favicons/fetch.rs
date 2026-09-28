//! Fetching a page's icon off the UI thread: the page's declared `<link rel=icon>` (or
//! `/favicon.ico`), decoded and scaled down to the PNG [`super::Favicons::record`] keeps.
//!
//! Everything here reads untrusted bytes from the network, so every size is capped: the
//! page, the icon file, the decoded image's dimensions and allocations.

use std::borrow::Cow;
use std::io::{Cursor, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use ureq::ResponseExt;

use super::MAX_BYTES;
use crate::Url;

/// Pages fetched at once.
const WORKERS: usize = 6;
/// Only the head matters; a page is read up to this much.
const MAX_PAGE_BYTES: u64 = 1024 * 1024;
const MAX_ICON_BYTES: u64 = 512 * 1024;
/// Icon files tried per page: the best declared ones, then `/favicon.ico`.
const MAX_ATTEMPTS: usize = 3;
/// The edge of the stored icon: what a 16px icon needs at 200% scale.
const SIZE: u32 = 32;
/// Some servers refuse clients that do not look like a browser.
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36";

/// The icons of `pages`, fetched on a worker thread. `Send`, and holds no profile: the
/// results go back to [`super::Favicons::commit_fetched`] on the UI thread.
pub struct FaviconFetch {
    pages: Vec<Url>,
}

/// One page's result: a PNG of at most 32x32 and [`MAX_BYTES`], or `None` when the page
/// has no icon that could be fetched and decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    pub page: Url,
    pub png: Option<Vec<u8>>,
}

impl FaviconFetch {
    pub fn new(pages: Vec<Url>) -> Self {
        FaviconFetch { pages }
    }

    /// Blocks until every page is done, [`WORKERS`] at a time. Results are in page order.
    pub fn run(self) -> Vec<Fetched> {
        let agent = agent();
        let next = AtomicUsize::new(0);
        let mut icons = vec![None; self.pages.len()];
        std::thread::scope(|s| {
            let workers: Vec<_> = (0..WORKERS.min(self.pages.len()))
                .map(|_| {
                    s.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(page) = self.pages.get(i) else { break };
                            // Decoders of untrusted bytes can panic; that page just gets no icon.
                            done.push((i, catch_unwind(AssertUnwindSafe(|| page_icon(&agent, page))).ok().flatten()));
                        }
                        done
                    })
                })
                .collect();
            for worker in workers {
                for (i, png) in worker.join().expect("worker panics are caught") {
                    icons[i] = png;
                }
            }
        });
        self.pages.into_iter().zip(icons).map(|(page, png)| Fetched { page, png }).collect()
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

/// The first of the page's candidate icons that fetches and decodes.
fn page_icon(agent: &ureq::Agent, page: &Url) -> Option<Vec<u8>> {
    if !matches!(page.scheme(), "http" | "https") {
        return None;
    }
    let (final_url, html) = match agent.get(page.as_str()).call() {
        Ok(response) => {
            let final_url = Url::parse(&response.get_uri().to_string()).unwrap_or_else(|_| page.clone());
            let mut html = Vec::new();
            let _ = response.into_body().into_reader().take(MAX_PAGE_BYTES).read_to_end(&mut html);
            (final_url, String::from_utf8_lossy(&html).into_owned())
        }
        Err(_) => (page.clone(), String::new()),
    };
    let mut candidates = icon_links(&html, &final_url);
    if let Ok(fallback) = final_url.join("/favicon.ico")
        && !candidates.contains(&fallback)
    {
        candidates.push(fallback);
    }
    candidates.into_iter().take(MAX_ATTEMPTS).find_map(|icon| normalize(&icon_bytes(agent, &icon)?))
}

fn icon_bytes(agent: &ureq::Agent, icon: &Url) -> Option<Vec<u8>> {
    match icon.scheme() {
        "http" | "https" => {
            let mut response = agent.get(icon.as_str()).call().ok()?;
            response.body_mut().with_config().limit(MAX_ICON_BYTES).read_to_vec().ok()
        }
        "data" => {
            let (meta, data) = icon.path().split_once(',')?;
            let meta = meta.strip_suffix(";base64")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
            (!meta.contains("svg") && bytes.len() as u64 <= MAX_ICON_BYTES).then_some(bytes)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Picking the declared icon
// ---------------------------------------------------------------------------

/// One `<link>` in the head that declares an icon.
#[derive(Debug)]
struct IconLink {
    href: Url,
    apple_touch: bool,
    /// From `type` or the file extension: PNG and ICO are what browsers serve best.
    raster: bool,
    /// The declared size to judge by: the smallest one of at least 32, else the largest.
    size: Option<u32>,
}

impl IconLink {
    /// Lower is better: a declared size of at least 32 (the closest to it first), then
    /// icons of unknown size, then smaller ones (the largest first). PNG or ICO, then
    /// `rel=icon` over `apple-touch-icon`, break ties.
    fn cost(&self) -> (u8, u32, u8, u8) {
        let size = match self.size {
            Some(s) if s >= SIZE => (0, s - SIZE),
            None => (1, 0),
            Some(s) => (2, SIZE - s),
        };
        (size.0, size.1, u8::from(!self.raster), u8::from(self.apple_touch))
    }
}

/// The icons `html` declares in its head, best first, resolved against `<base href>` and
/// then `page`. SVG icons are left out: they are not decoded here.
fn icon_links(html: &str, page: &Url) -> Vec<Url> {
    let tags = head_tags(html);
    let base = tags
        .iter()
        .find(|(name, _)| name == "base")
        .and_then(|(_, attrs)| attr(attrs, "href"))
        .and_then(|href| page.join(href).ok())
        .unwrap_or_else(|| page.clone());
    let mut links: Vec<IconLink> = tags
        .iter()
        .filter(|(name, _)| name == "link")
        .filter_map(|(_, attrs)| {
            let rel = attr(attrs, "rel")?.to_ascii_lowercase();
            let apple_touch = rel.split_ascii_whitespace().any(|r| r.starts_with("apple-touch-icon"));
            if !apple_touch && !rel.split_ascii_whitespace().any(|r| r == "icon") {
                return None;
            }
            let href = base.join(attr(attrs, "href")?.trim()).ok()?;
            let mime = attr(attrs, "type").unwrap_or_default().to_ascii_lowercase();
            let path = href.path().to_ascii_lowercase();
            if mime.contains("svg") || path.ends_with(".svg") || (href.scheme() == "data" && path.contains("svg")) {
                return None;
            }
            let raster = ["png", "icon"].iter().any(|t| mime.contains(t)) || path.ends_with(".png") || path.ends_with(".ico");
            let sizes: Vec<u32> = attr(attrs, "sizes")
                .unwrap_or_default()
                .split_ascii_whitespace()
                .filter_map(|s| s.to_ascii_lowercase().split_once('x')?.0.parse().ok())
                .collect();
            let size = sizes.iter().copied().filter(|&s| s >= SIZE).min().or_else(|| sizes.iter().copied().max());
            // Apple touch icons are 180x180 unless they say otherwise.
            let size = size.or(apple_touch.then_some(180));
            Some(IconLink { href, apple_touch, raster, size })
        })
        .collect();
    links.sort_by_key(IconLink::cost);
    links.into_iter().map(|l| l.href).collect()
}

type Attrs = Vec<(String, String)>;

fn attr<'a>(attrs: &'a Attrs, name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// The `<link>` and `<base>` tags before `<body>` or `</head>`, with lowercase attribute
/// names and entity-decoded values. Comments and the text of `<script>`, `<style>`,
/// `<title>` and `<noscript>` are skipped, so a `<link` inside them does not count.
fn head_tags(html: &str) -> Vec<(String, Attrs)> {
    // ASCII lowercasing keeps byte offsets, so `lower` indexes `html` too.
    let lower = html.to_ascii_lowercase();
    let mut tags = Vec::new();
    let mut at = 0;
    while let Some(lt) = lower[at..].find('<').map(|i| at + i) {
        let rest = &lower[lt + 1..];
        if rest.starts_with("!--") {
            let Some(end) = rest.find("-->") else { break };
            at = lt + 1 + end + 3;
            continue;
        }
        let slash = usize::from(rest.starts_with('/'));
        let name_len = slash + rest[slash..].find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len() - slash);
        let name = &rest[..name_len];
        if name_len == slash {
            // Not a tag: a `<` in text, or `<!doctype>`.
            at = lt + 1;
            continue;
        }
        if name == "body" || name == "/head" {
            break;
        }
        let (attrs, end) = parse_attrs(html, lt + 1 + name_len);
        at = end;
        match name {
            "link" | "base" => tags.push((name.to_owned(), attrs)),
            "script" | "style" | "title" | "noscript" => {
                let close = format!("</{name}");
                let Some(i) = lower[at..].find(&close) else { break };
                at += i + close.len();
            }
            _ => {}
        }
    }
    tags
}

/// Attributes from `at` up to the closing `>`. Returns them and the offset after the `>`.
fn parse_attrs(html: &str, mut at: usize) -> (Attrs, usize) {
    let b = html.as_bytes();
    let mut attrs = Vec::new();
    loop {
        while at < b.len() && (b[at].is_ascii_whitespace() || b[at] == b'/') {
            at += 1;
        }
        if at >= b.len() {
            return (attrs, b.len());
        }
        if b[at] == b'>' {
            return (attrs, at + 1);
        }
        let start = at;
        while at < b.len() && !b[at].is_ascii_whitespace() && !matches!(b[at], b'=' | b'>' | b'/') {
            at += 1;
        }
        let name = html[start..at].to_ascii_lowercase();
        while at < b.len() && b[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = "";
        if at < b.len() && b[at] == b'=' {
            at += 1;
            while at < b.len() && b[at].is_ascii_whitespace() {
                at += 1;
            }
            if at < b.len() && matches!(b[at], b'"' | b'\'') {
                let quote = b[at];
                let end = b[at + 1..].iter().position(|&c| c == quote).map_or(b.len(), |i| at + 1 + i);
                value = &html[at + 1..end];
                at = (end + 1).min(b.len());
            } else {
                let start = at;
                while at < b.len() && !b[at].is_ascii_whitespace() && b[at] != b'>' {
                    at += 1;
                }
                value = &html[start..at];
            }
        }
        attrs.push((name, decode_entities(value)));
    }
}

fn decode_entities(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

// ---------------------------------------------------------------------------
// Normalizing the image
// ---------------------------------------------------------------------------

/// Decodes a PNG, ICO, JPEG, GIF, WebP or BMP and returns it as a PNG that fits in 32x32
/// (aspect kept, never scaled up). From an ICO, the entry closest to 32x32 is used.
fn normalize(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    let bytes = match format {
        ImageFormat::Ico => Cow::Owned(best_ico_entry(bytes)?),
        _ => Cow::Borrowed(bytes),
    };
    let mut limits = Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(bytes.as_ref()), format);
    reader.limits(limits);
    let mut image = reader.decode().ok()?;
    if image.width() > SIZE || image.height() > SIZE {
        image = image.resize(SIZE, SIZE, image::imageops::FilterType::Lanczos3);
    }
    let mut png = Vec::new();
    DynamicImage::from(image.into_rgba8()).write_to(&mut Cursor::new(&mut png), ImageFormat::Png).ok()?;
    (png.len() <= MAX_BYTES).then_some(png)
}

/// An ICO with just the entry to use: the smallest of at least 32px, else the largest,
/// the most colors breaking ties. The image decoder would pick the largest one.
fn best_ico_entry(ico: &[u8]) -> Option<Vec<u8>> {
    const HEADER: usize = 6;
    const ENTRY: usize = 16;
    let count = usize::from(u16::from_le_bytes([*ico.get(4)?, *ico.get(5)?]));
    let entries: Vec<&[u8]> = (0..count).map_while(|i| ico.get(HEADER + i * ENTRY..HEADER + (i + 1) * ENTRY)).collect();
    let edge = |e: &[u8]| if e[0] == 0 { 256 } else { u32::from(e[0]) };
    let bpp = |e: &[u8]| u16::from_le_bytes([e[6], e[7]]);
    let best = entries.iter().copied().max_by_key(|&e| {
        let size = edge(e);
        let fit = if size >= SIZE { (1, u32::MAX - size) } else { (0, size) };
        (fit, bpp(e))
    })?;
    let len = u32::from_le_bytes(best[8..12].try_into().ok()?);
    let offset = u32::from_le_bytes(best[12..16].try_into().ok()?);
    let data = ico.get(usize::try_from(offset).ok()?..usize::try_from(offset.checked_add(len)?).ok()?)?;
    let mut single = Vec::with_capacity(HEADER + ENTRY + data.len());
    single.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    single.extend_from_slice(&best[..12]);
    single.extend_from_slice(&((HEADER + ENTRY) as u32).to_le_bytes());
    single.extend_from_slice(data);
    Some(single)
}

#[cfg(test)]
mod tests {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    use image::{ExtendedColorType, Rgba, RgbaImage};

    use super::*;

    fn links(html: &str) -> Vec<String> {
        let page = Url::parse("https://site.example/dir/page").unwrap();
        icon_links(html, &page).into_iter().map(String::from).collect()
    }

    fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::from(RgbaImage::from_pixel(w, h, Rgba(color)))
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn decoded(png: &[u8]) -> RgbaImage {
        image::load_from_memory_with_format(png, ImageFormat::Png).unwrap().into_rgba8()
    }

    #[test]
    fn prefers_raster_icons_of_at_least_32_closest_to_32() {
        let html = r#"<html><head>
            <link rel="apple-touch-icon" href="/apple.png">
            <link rel="icon" type="image/svg+xml" href="/icon.svg">
            <link rel="icon" href="/any.svg" sizes="any">
            <link rel="icon" type="image/png" sizes="16x16" href="/16.png">
            <link rel="icon" type="image/png" sizes="192x192" href="/192.png">
            <LINK REL="Shortcut Icon" HREF="/favicon.ico">
            <link rel=icon type=image/png sizes="16x16 48x48" href=48.png>
            <link rel="stylesheet" href="/style.css">
        </head><body><link rel="icon" href="/in-body.png"></body></html>"#;
        assert_eq!(
            links(html),
            [
                "https://site.example/dir/48.png",
                "https://site.example/apple.png",
                "https://site.example/192.png",
                "https://site.example/favicon.ico",
                "https://site.example/16.png",
            ]
        );
    }

    #[test]
    fn resolves_against_base_href_and_decodes_entities() {
        let html = r#"<head><base href="https://cdn.example/assets/"><link rel="icon" href="i.png?a=1&amp;b=2"></head>"#;
        assert_eq!(links(html), ["https://cdn.example/assets/i.png?a=1&b=2"]);
    }

    #[test]
    fn skips_links_in_comments_and_scripts() {
        let html = r#"<!doctype html><head><!-- <link rel="icon" href="/comment.png"> -->
            <script>document.write('<link rel="icon" href="/script.png">')</script>
            <title>a < b</title><link rel="icon" href="/real.png"></head>"#;
        assert_eq!(links(html), ["https://site.example/real.png"]);
    }

    #[test]
    fn keeps_raster_data_icons_but_not_svg_ones() {
        let html = r#"<head><link rel="icon" href="data:image/svg+xml;base64,PHN2Zz4="><link rel="icon" href="data:image/png;base64,AAAA"></head>"#;
        assert_eq!(links(html), ["data:image/png;base64,AAAA"]);
    }

    #[test]
    fn scales_down_to_32_keeping_the_aspect_and_never_up() {
        let big = decoded(&normalize(&png(64, 64, [200, 10, 10, 255])).unwrap());
        assert_eq!(big.dimensions(), (32, 32));
        assert_eq!(decoded(&normalize(&png(16, 16, [0; 4])).unwrap()).dimensions(), (16, 16));
        assert_eq!(decoded(&normalize(&png(64, 16, [0; 4])).unwrap()).dimensions(), (32, 8));
    }

    #[test]
    fn uses_the_ico_entry_closest_to_32() {
        let frames = [(16, [255, 0, 0, 255]), (48, [0, 255, 0, 255]), (128, [0, 0, 255, 255])].map(|(edge, color)| {
            let pixels = RgbaImage::from_pixel(edge, edge, Rgba(color)).into_raw();
            IcoFrame::as_png(&pixels, edge, edge, ExtendedColorType::Rgba8).unwrap()
        });
        let mut ico = Vec::new();
        IcoEncoder::new(&mut ico).encode_images(&frames).unwrap();

        let icon = decoded(&normalize(&ico).unwrap());
        assert_eq!(icon.dimensions(), (32, 32));
        assert_eq!(icon.get_pixel(16, 16), &Rgba([0, 255, 0, 255]), "the 48px entry");
    }

    #[test]
    fn rejects_what_is_not_a_raster_image() {
        assert_eq!(normalize(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(normalize(b"<!doctype html><title>Not found</title>"), None);
        assert_eq!(normalize(&png(64, 64, [0; 4])[..40]), None, "truncated");
    }
}
