//! A tiny HTTP/1.1 server for `tests/fixtures/site/`, bound to 127.0.0.1 on a random
//! port. It records the path of every request, so a test can prove that a request was
//! made (`/allowed.png`) or was blocked before it left the engine
//! (`/vsesvit-blocked/pixel.png`).

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
    site_file!("allowed.png", "image/png"),
    site_file!("vsesvit-blocked/pixel.png", "image/png"),
    site_file!("download.bin", "application/octet-stream"),
];

const MAX_REQUEST_HEAD: usize = 16 * 1024;
/// Engines open speculative connections that never send a request; each connection
/// thread gives up after this long.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Serves until dropped. Each connection gets its own thread and one response
/// (`Connection: close`, `Cache-Control: no-store`, so every page load hits the server).
pub struct FixtureServer {
    addr: SocketAddr,
    hits: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl FixtureServer {
    pub fn start() -> io::Result<FixtureServer> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let accept = std::thread::Builder::new().name("fixture-server".into()).spawn({
            let hits = Arc::clone(&hits);
            let stop = Arc::clone(&stop);
            move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let hits = Arc::clone(&hits);
                    let _ = std::thread::Builder::new().name("fixture-conn".into()).spawn(move || {
                        let _ = serve(stream, &hits);
                    });
                }
            }
        })?;
        Ok(FixtureServer { addr, hits, stop, accept: Some(accept) })
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

fn serve(mut stream: TcpStream, hits: &Mutex<Vec<String>>) -> io::Result<()> {
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(IDLE_TIMEOUT))?;
    let Some(head) = read_head(&mut stream)? else { return Ok(()) };
    let mut request_line = head.lines().next().unwrap_or_default().split_ascii_whitespace();
    let (method, target) = (request_line.next().unwrap_or_default(), request_line.next().unwrap_or_default());
    let path = request_path(target);
    hits.lock().unwrap_or_else(PoisonError::into_inner).push(path.to_owned());

    let lookup = if path == "/" { "/index.html" } else { path };
    let (status, content_type, body) = match SITE.iter().find(|(p, _, _)| *p == lookup) {
        Some((_, content_type, body)) if method == "GET" || method == "HEAD" => ("200 OK", *content_type, *body),
        _ => ("404 Not Found", "text/plain; charset=utf-8", b"not found".as_slice()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    if method != "HEAD" {
        stream.write_all(body)?;
    }
    stream.flush()
}

/// Reads up to the blank line that ends the request head. `None` if the peer closed or
/// went idle first.
fn read_head(stream: &mut TcpStream) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > MAX_REQUEST_HEAD {
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
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
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
