//! A tiny HTTP/1.1 server for `tests/fixtures/site/`, bound to 127.0.0.1 on a random
//! port, plus `/suggest?q=<terms>`, a search engine's suggestions for the terms,
//! `/set-cookie`, a page that sets a cookie in its response header, and `/stalled.bin`, a
//! download that never finishes. A test adds answers of its own with
//! [`FixtureServer::route`] (`testkit::FixtureStore` serves the extension stores that way).
//! It records the path of every request, so a test can prove that a request was made
//! (`/allowed.png`) or was blocked before it left the engine (`/vsesvit-blocked/pixel.png`).

use std::borrow::Cow;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::Url;

macro_rules! site_file {
    ($path:literal, $content_type:literal) => {
        (
            concat!("/", $path),
            $content_type,
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/site/", $path)).as_slice(),
        )
    };
}

const SITE: &[(&str, &str, &[u8])] = &[
    site_file!("index.html", "text/html; charset=utf-8"),
    site_file!("page2.html", "text/html; charset=utf-8"),
    // A page that plays a video, for the media player and picture-in-picture.
    site_file!("media.html", "text/html; charset=utf-8"),
    site_file!("allowed.png", "image/png"),
    site_file!("vsesvit-blocked/pixel.png", "image/png"),
    // A page that loads an image from `localhost`, a tracker in the self-tests.
    site_file!("trackers.html", "text/html; charset=utf-8"),
    site_file!("tracker/pixel.png", "image/png"),
    // A page that sets a cookie and embeds a frame from `localhost`, another site, that sets one.
    site_file!("cookies.html", "text/html; charset=utf-8"),
    // The frame `cookies.html` embeds.
    site_file!("cookie-frame.html", "text/html; charset=utf-8"),
    // A page whose images say which of the extension harness's declarativeNetRequest rules
    // blocked them.
    site_file!("dnr.html", "text/html; charset=utf-8"),
    // A page with a frame of its own origin and one of another, for the extension harness's
    // webNavigation checks.
    site_file!("frames.html", "text/html; charset=utf-8"),
    site_file!("download.bin", "application/octet-stream"),
    // Scripts, a type that can run code on Windows and on Linux: their downloads wait for Keep.
    ("/dangerous.bat", "application/octet-stream", b"@echo Vsesvit fixture\r\n"),
    ("/dangerous.sh", "application/octet-stream", b"#!/bin/sh\necho Vsesvit fixture\n"),
    // A page that declares its icon, for favicon fetching.
    (
        "/icon.html",
        "text/html; charset=utf-8",
        b"<!doctype html><html><head><title>Icon</title><link rel=\"icon\" href=\"allowed.png\"></head><body></body></html>",
    ),
];

/// `/stalled.bin` announces this many bytes, sends [`STALLED_SENT`] and then nothing more until
/// the client closes the connection: a download that stays in progress.
const STALLED_SIZE: usize = 1_000_000;
pub const STALLED_SENT: usize = 1_000;

/// `/set-cookie`, served with `Set-Cookie: served=1; Path=/`.
const COOKIE_SET_PAGE: &[u8] = b"<!doctype html><html><head><title>Cookie set</title></head><body></body></html>";

const MAX_REQUEST_HEAD: usize = 16 * 1024;
const MAX_REQUEST_BODY: usize = 1 << 20;
/// Engines open speculative connections that never send a request; each connection
/// thread gives up after this long.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);
/// The first byte of a TLS handshake record.
const TLS_HANDSHAKE: u8 = 0x16;

/// A request a [`FixtureServer::route`] handler answers.
#[derive(Clone, Debug)]
pub struct FixtureRequest {
    pub method: String,
    /// The path and query, as the request line has them.
    pub target: String,
    /// Read up to the request's `Content-Length`.
    pub body: Vec<u8>,
}

impl FixtureRequest {
    /// The target without its query.
    pub fn path(&self) -> &str {
        request_path(&self.target)
    }

    /// The query's pairs in order, decoded.
    pub fn query(&self) -> Vec<(String, String)> {
        let query = self.target.split_once('?').map_or("", |(_, query)| query);
        url::form_urlencoded::parse(query.as_bytes()).into_owned().collect()
    }
}

#[derive(Clone, Debug)]
pub struct FixtureResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl FixtureResponse {
    pub fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> FixtureResponse {
        FixtureResponse { status: 200, content_type: content_type.to_owned(), body: body.into() }
    }

    /// An empty response with `status`: 204, 404.
    pub fn status(status: u16) -> FixtureResponse {
        FixtureResponse { status, content_type: "text/plain; charset=utf-8".to_owned(), body: Vec::new() }
    }
}

type Handler = dyn Fn(&FixtureRequest) -> FixtureResponse + Send + Sync;
type Routes = Mutex<Vec<(String, Arc<Handler>)>>;

/// Serves until dropped. Each connection gets its own thread and one response
/// (`Connection: close`, `Cache-Control: no-store`, so every page load hits the server).
pub struct FixtureServer {
    addr: SocketAddr,
    hits: Arc<Mutex<Vec<String>>>,
    routes: Arc<Routes>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl FixtureServer {
    pub fn start() -> io::Result<FixtureServer> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let hits = Arc::new(Mutex::new(Vec::new()));
        let routes = Arc::new(Routes::default());
        let stop = Arc::new(AtomicBool::new(false));
        let accept = std::thread::Builder::new().name("fixture-server".into()).spawn({
            let hits = Arc::clone(&hits);
            let routes = Arc::clone(&routes);
            let stop = Arc::clone(&stop);
            move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let hits = Arc::clone(&hits);
                    let routes = Arc::clone(&routes);
                    let _ = std::thread::Builder::new().name("fixture-conn".into()).spawn(move || {
                        let _ = serve(stream, &hits, &routes);
                    });
                }
            }
        })?;
        Ok(FixtureServer { addr, hits, routes, stop, accept: Some(accept) })
    }

    /// Answers every request whose path starts with `path_prefix` with `handler`, on the
    /// connection's thread. Routes come before the site's files; of two routes that both
    /// match, the one added last answers.
    pub fn route(&self, path_prefix: &str, handler: impl Fn(&FixtureRequest) -> FixtureResponse + Send + Sync + 'static) {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner).push((path_prefix.to_owned(), Arc::new(handler)));
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// `http://127.0.0.1:<port>`
    pub fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// `path` must start with `/`: `server.url("/index.html")`.
    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}{path}", self.origin())).expect("a valid fixture URL")
    }

    /// Request paths seen so far, in arrival order, without query strings. 404s included.
    pub fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // `accept` blocks; a connection of our own wakes it so it can see the flag.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

fn serve(mut stream: TcpStream, hits: &Mutex<Vec<String>>, routes: &Routes) -> io::Result<()> {
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(IDLE_TIMEOUT))?;
    let Some((head, body_start)) = read_head(&mut stream)? else { return Ok(()) };
    let mut request_line = head.lines().next().unwrap_or_default().split_ascii_whitespace();
    let (method, target) = (request_line.next().unwrap_or_default(), request_line.next().unwrap_or_default());
    let path = request_path(target);
    hits.lock().unwrap_or_else(PoisonError::into_inner).push(path.to_owned());

    let routed = {
        let routes = routes.lock().unwrap_or_else(PoisonError::into_inner);
        routes.iter().rev().find(|(prefix, _)| path.starts_with(prefix.as_str())).map(|(_, handler)| Arc::clone(handler))
    };
    if let Some(handler) = routed {
        let body = read_body(&mut stream, &head, body_start)?;
        let response = handler(&FixtureRequest { method: method.to_owned(), target: target.to_owned(), body });
        let status = format!("{} {}", response.status, reason(response.status));
        return respond(stream, &status, "", &response.content_type, &response.body, method != "HEAD");
    }

    let lookup = if path == "/" { "/index.html" } else { path };
    let get = method == "GET" || method == "HEAD";
    if method == "GET" && path == "/stalled.bin" {
        return stall(stream);
    }
    let set_cookie = get && path == "/set-cookie";
    let (status, content_type, body) = match SITE.iter().find(|(p, _, _)| *p == lookup) {
        Some((_, content_type, body)) if get => ("200 OK", *content_type, Cow::Borrowed(*body)),
        None if get && path == "/suggest" => ("200 OK", "application/x-suggestions+json; charset=utf-8", Cow::Owned(suggestions(target))),
        None if set_cookie => ("200 OK", "text/html; charset=utf-8", Cow::Borrowed(COOKIE_SET_PAGE)),
        _ => ("404 Not Found", "text/plain; charset=utf-8", Cow::Borrowed(b"not found".as_slice())),
    };
    let cookie = if set_cookie { "Set-Cookie: served=1; Path=/\r\n" } else { "" };
    respond(stream, status, cookie, content_type, &body, method != "HEAD")
}

/// One response, then the connection closes. `headers` are extra header lines, each ending in CRLF.
fn respond(mut stream: TcpStream, status: &str, headers: &str, content_type: &str, body: &[u8], send_body: bool) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    if send_body {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

fn stall(mut stream: TcpStream) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {STALLED_SIZE}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&[0; STALLED_SENT])?;
    stream.flush()?;
    stream.set_read_timeout(None)?;
    while stream.read(&mut [0; 512])? > 0 {}
    Ok(())
}

/// OpenSearch suggestions for the `q` of `target`: `["<q>",["<q> one","<q> two"]]`.
fn suggestions(target: &str) -> Vec<u8> {
    let query = target.split_once('?').map_or("", |(_, query)| query);
    let terms = url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "q").map(|(_, v)| v.into_owned()).unwrap_or_default();
    serde_json::to_vec(&serde_json::json!([terms, [format!("{terms} one"), format!("{terms} two")]])).expect("JSON of strings")
}

/// Reads up to the blank line that ends the request head, and returns the head and the
/// body bytes that arrived with it. `None` if the peer closed or went idle first, or opened
/// with a TLS handshake: an https load of this plain server, as the HTTPS-only self-tests
/// make, is refused at once rather than left to go idle.
fn read_head(stream: &mut TcpStream) -> io::Result<Option<(String, Vec<u8>)>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let body_start = buf.split_off(end + 4);
            return Ok(Some((String::from_utf8_lossy(&buf).into_owned(), body_start)));
        }
        if buf.len() > MAX_REQUEST_HEAD || buf.first() == Some(&TLS_HANDSHAKE) {
            return Ok(None);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(None),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// `body_start` and the rest of the body, up to the head's `Content-Length` (at most
/// [`MAX_REQUEST_BODY`]); none without one.
fn read_body(stream: &mut TcpStream, head: &str, mut body: Vec<u8>) -> io::Result<Vec<u8>> {
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_REQUEST_BODY);
    let mut chunk = [0u8; 4096];
    while body.len() < length {
        match stream.read(&mut chunk)? {
            0 => break,
            n => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    Ok(body)
}

/// `/a/b?x#y` -> `/a/b`. Absolute-form targets (`http://host/a`) keep only the path.
fn request_path(target: &str) -> &str {
    let target = match target.strip_prefix("http://") {
        Some(rest) => rest.find('/').map_or("/", |i| &rest[i..]),
        None => target,
    };
    let end = target.find(['?', '#']).unwrap_or(target.len());
    &target[..end]
}
