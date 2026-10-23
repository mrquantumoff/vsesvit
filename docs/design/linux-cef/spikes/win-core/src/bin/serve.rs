//! Tiny static file server for the test pages: `serve DIR PORT`.
use std::io::{Read as _, Write as _};

fn main() {
    let dir = std::env::args().nth(1).expect("serve DIR PORT");
    let port: u16 = std::env::args().nth(2).and_then(|p| p.parse().ok()).unwrap_or(8765);
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind");
    for stream in listener.incoming().flatten() {
        let dir = dir.clone();
        std::thread::spawn(move || {
            let mut s = stream;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("/");
            let path = path.split('?').next().unwrap_or("/").trim_start_matches('/');
            let path = if path.is_empty() { "page1.html" } else { path };
            let mime = match path.rsplit('.').next() {
                Some("html") => "text/html; charset=utf-8",
                Some("png") => "image/png",
                Some("txt") => "text/plain",
                _ => "application/octet-stream",
            };
            let full = std::path::Path::new(&dir).join(path);
            let resp = match std::fs::read(&full) {
                Ok(body) if !path.contains("..") => {
                    let mut r = format!("HTTP/1.0 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
                    r.extend(body);
                    r
                }
                _ => b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = s.write_all(&resp);
        });
    }
}
