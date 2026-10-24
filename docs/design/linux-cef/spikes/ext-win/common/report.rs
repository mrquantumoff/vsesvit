//! Per-profile HTTP server on 127.0.0.1:`SPIKE_REPORT_PORT`. Chromium reaches it as
//! `*.vsesvit.test` through `--host-resolver-rules` (app.rs), so the same extension code
//! reports to whichever profile it runs in:
//! - `POST /report`: body logged as `report: <body>` and appended to `SPIKE_REPORTS`;
//! - `GET /updates/*`: update manifests and CRX files from `SPIKE_SITE/updates`, logged;
//! - `GET /slow?ms=N&to=PATH`: waits N ms, then serves PATH (default page2.html);
//! - `GET /echo`: answers with the request's Cookie header, logged as `report: echo`;
//! - anything else: files from `SPIKE_SITE`.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn spawn() -> Option<u16> {
    let port: u16 = std::env::var("SPIKE_REPORT_PORT").ok()?.parse().ok()?;
    let site = PathBuf::from(std::env::var("SPIKE_SITE").ok()?);
    let reports_path = std::env::var("SPIKE_REPORTS").unwrap_or_else(|_| site.join("reports.jsonl").to_string_lossy().into_owned());
    let out = Arc::new(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&reports_path).ok()?));
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            crate::log(&format!("report: cannot bind 127.0.0.1:{port}: {e}"));
            return None;
        }
    };
    crate::log(&format!("report: 127.0.0.1:{port} serving {} ; reports -> {reports_path}", site.display()));
    std::thread::Builder::new()
        .name("report-http".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let site = site.clone();
                let out = out.clone();
                std::thread::spawn(move || {
                    let _ = handle(conn, &site, &out);
                });
            }
        })
        .ok()?;
    Some(port)
}

fn query(target: &str, key: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    q.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v.to_string()))
}

fn handle(conn: TcpStream, site: &PathBuf, out: &Mutex<std::fs::File>) -> std::io::Result<()> {
    let mut r = BufReader::new(conn.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (mut len, mut origin, mut host, mut cookie) = (0usize, String::new(), String::new(), String::new());
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        let lower = h.to_ascii_lowercase();
        let val = || h.split_once(':').map(|(_, v)| v.trim().to_string()).unwrap_or_default();
        if lower.starts_with("content-length:") {
            len = val().parse().unwrap_or(0);
        } else if lower.starts_with("origin:") {
            origin = val();
        } else if lower.starts_with("host:") {
            host = val();
        } else if lower.starts_with("cookie:") {
            cookie = val();
        }
    }
    let mut body = vec![0u8; len.min(4 << 20)];
    r.read_exact(&mut body)?;
    let mut w = conn;
    let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: *\r\nCache-Control: no-store\r\n";
    let path = target.split(['?', '#']).next().unwrap_or("/").to_string();
    match (method.as_str(), path.as_str()) {
        ("OPTIONS", _) => write!(w, "HTTP/1.1 204 No Content\r\n{cors}Content-Length: 0\r\nConnection: close\r\n\r\n")?,
        ("POST", "/report") => {
            let text = String::from_utf8_lossy(&body).replace('\n', " ");
            crate::log(&format!("report: {text}  (origin {origin})"));
            if let Ok(mut f) = out.lock() {
                let _ = writeln!(f, "{text}");
            }
            write!(w, "HTTP/1.1 200 OK\r\n{cors}Content-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")?;
        }
        ("GET", "/echo") => {
            crate::log(&format!("report: echo host={host} cookie={cookie:?}"));
            let b = format!("<!doctype html><title>echo</title><pre id=c>{cookie}</pre>");
            write!(w, "HTTP/1.1 200 OK\r\n{cors}Content-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len())?;
        }
        _ => {
            if path == "/slow" {
                let ms: u64 = query(&target, "ms").and_then(|v| v.parse().ok()).unwrap_or(1000);
                crate::log(&format!("report: slow {ms} ms host={host}"));
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
            let rel = if path == "/slow" { query(&target, "to").unwrap_or_else(|| "page2.html".into()) } else { path.trim_start_matches('/').to_string() };
            let rel = if rel.is_empty() { "index.html".to_string() } else { rel };
            let file = site.join(&rel);
            if !rel.contains("..") && file.is_file() {
                if rel.starts_with("updates/") {
                    crate::log(&format!("report: GET {target} host={host}"));
                }
                let data = std::fs::read(&file)?;
                let ctype = match file.extension().and_then(|e| e.to_str()) {
                    Some("html") => "text/html; charset=utf-8",
                    Some("png") => "image/png",
                    Some("js") => "text/javascript",
                    Some("json") => "application/json",
                    Some("xml") => "text/xml",
                    Some("crx") => "application/x-chrome-extension",
                    _ => "application/octet-stream",
                };
                write!(w, "HTTP/1.1 200 OK\r\n{cors}Content-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())?;
                w.write_all(&data)?;
            } else {
                crate::log(&format!("report: 404 {method} {target} host={host}"));
                write!(w, "HTTP/1.1 404 Not Found\r\n{cors}Content-Length: 0\r\nConnection: close\r\n\r\n")?;
            }
        }
    }
    w.flush()
}
