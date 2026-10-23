//! Tiny HTTP server on 127.0.0.1 for the extension spike:
//! - `GET /<path>` serves files from `SPIKE_SITE` (the fixture site, copied to scratch); requests
//!   under `/updates/` (extension update manifests and CRX files) are logged as `report: GET <target>`;
//! - `POST /report` (any body) is logged as `report: <body>` and appended to `SPIKE_REPORTS`
//!   (JSON lines). Test extensions call it with `fetch()` from service workers, content
//!   scripts and extension pages, so every context can report what it saw.
//! CORS is wide open (`Access-Control-Allow-Origin: *`): a report must arrive even from an
//! extension whose host permission for 127.0.0.1 is withheld.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn spawn(port: u16) {
    let site = PathBuf::from(std::env::var("SPIKE_SITE").unwrap_or_else(|_| crate::env_dir().join("site").to_string_lossy().into_owned()));
    let reports_path = std::env::var("SPIKE_REPORTS").unwrap_or_else(|_| crate::env_dir().join("reports.jsonl").to_string_lossy().into_owned());
    let out = Arc::new(Mutex::new(
        std::fs::OpenOptions::new().create(true).append(true).open(&reports_path).expect("reports file"),
    ));
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            crate::log(&format!("report: cannot bind 127.0.0.1:{port}: {e}"));
            return;
        }
    };
    crate::log(&format!("report: http://127.0.0.1:{port}/ serving {} ; reports -> {reports_path}", site.display()));
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
        .ok();
}

fn handle(conn: TcpStream, site: &PathBuf, out: &Mutex<std::fs::File>) -> std::io::Result<()> {
    let mut r = BufReader::new(conn.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut len = 0usize;
    let mut origin = String::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("origin:") {
            origin = h[7..].trim().to_string();
        }
    }
    let mut body = vec![0u8; len.min(4 << 20)];
    r.read_exact(&mut body)?;
    let mut w = conn;
    let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: *\r\n";
    match method.as_str() {
        "OPTIONS" => {
            write!(w, "HTTP/1.1 204 No Content\r\n{cors}Content-Length: 0\r\nConnection: close\r\n\r\n")?;
        }
        "POST" if target.starts_with("/report") => {
            let text = String::from_utf8_lossy(&body).replace('\n', " ");
            crate::log(&format!("report: {text}  (origin {origin})"));
            if let Ok(mut f) = out.lock() {
                let _ = writeln!(f, "{text}");
            }
            write!(w, "HTTP/1.1 200 OK\r\n{cors}Content-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")?;
        }
        _ => {
            let path = target.split(['?', '#']).next().unwrap_or("/");
            let rel = path.trim_start_matches('/');
            let rel = if rel.is_empty() { "index.html" } else { rel };
            let file = site.join(rel);
            let ok = !rel.contains("..") && file.is_file();
            if ok {
                if rel.starts_with("updates/") {
                    crate::log(&format!("report: GET {target}"));
                }
                let data = std::fs::read(&file)?;
                let ctype = match file.extension().and_then(|e| e.to_str()) {
                    Some("html") => "text/html; charset=utf-8",
                    Some("png") => "image/png",
                    Some("js") => "text/javascript",
                    Some("css") => "text/css",
                    Some("json") => "application/json",
                    Some("xml") => "text/xml",
                    Some("crx") => "application/x-chrome-extension",
                    _ => "application/octet-stream",
                };
                write!(w, "HTTP/1.1 200 OK\r\n{cors}Content-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())?;
                w.write_all(&data)?;
            } else {
                crate::log(&format!("report: 404 {method} {target}"));
                write!(w, "HTTP/1.1 404 Not Found\r\n{cors}Content-Length: 0\r\nConnection: close\r\n\r\n")?;
            }
        }
    }
    w.flush()
}
