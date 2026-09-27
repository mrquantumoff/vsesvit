//! Shared by the integration tests: a scripted HTTP server, a minisign signer and artifact bytes
//! for each format.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use url::Url;
use vsesvit_update::{Config, Format, WindowsInstallMode};

/// Path (query excluded) to status and body.
type Routes = Mutex<HashMap<String, (u16, Vec<u8>)>>;

#[derive(Debug, Clone)]
pub struct Request {
    /// Path and query, exactly as sent.
    pub target: String,
    /// Names lowercased.
    pub headers: HashMap<String, String>,
}

/// Answers each request by its path (query ignored) from routes set with [`Server::route`];
/// anything else is a 404. Every request is recorded.
pub struct Server {
    addr: SocketAddr,
    routes: Arc<Routes>,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Server {
    pub fn start() -> Server {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let routes = Arc::new(Mutex::new(HashMap::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        std::thread::spawn({
            let routes = Arc::clone(&routes);
            let requests = Arc::clone(&requests);
            move || {
                for stream in listener.incoming().flatten() {
                    let _ = serve(stream, &routes, &requests);
                }
            }
        });
        Server { addr, routes, requests }
    }

    pub fn route(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(path.to_owned(), (status, body.into()));
    }

    pub fn url(&self, path_and_query: &str) -> Url {
        Url::parse(&format!("http://{}{path_and_query}", self.addr)).unwrap()
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

fn serve(mut stream: TcpStream, routes: &Routes, requests: &Mutex<Vec<Request>>) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte)? == 0 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head);
    let mut lines = head.lines();
    let target = lines.next().unwrap_or_default().split_whitespace().nth(1).unwrap_or_default().to_owned();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let path = target.split('?').next().unwrap_or_default().to_owned();
    requests.lock().unwrap().push(Request { target, headers });

    let (status, body) = routes.lock().unwrap().get(&path).cloned().unwrap_or((404, b"not found".to_vec()));
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        _ => "Status",
    };
    let content_length = if status == 204 { String::new() } else { format!("Content-Length: {}\r\n", body.len()) };
    write!(stream, "HTTP/1.1 {status} {reason}\r\n{content_length}Connection: close\r\n\r\n")?;
    if status != 204 {
        stream.write_all(&body)?;
    }
    stream.flush()
}

/// A URL nothing listens on.
pub fn dead_url() -> Url {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    Url::parse(&format!("http://{addr}/update")).unwrap()
}

pub struct Signer(minisign::KeyPair);

impl Signer {
    pub fn new() -> Signer {
        Signer(minisign::KeyPair::generate_unencrypted_keypair().unwrap())
    }

    /// Base64 of the public key file, as Tauri configs hold it.
    pub fn pubkey(&self) -> String {
        base64(&self.0.pk.to_box().unwrap().into_string())
    }

    /// Base64 of the `.sig` file, as update responses hold it.
    pub fn sign(&self, data: &[u8], trusted_comment: &str) -> String {
        let signature = minisign::sign(Some(&self.0.pk), &self.0.sk, Cursor::new(data), Some(trusted_comment), None);
        base64(&signature.unwrap().into_string())
    }

    /// Signed the way `cargo xtask sign` and `tauri signer sign` sign.
    pub fn sign_release(&self, data: &[u8], version: &str) -> String {
        self.sign(data, &format!("timestamp:1790000000\tfile:artifact\tversion:{version}"))
    }
}

fn base64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

pub fn config(pubkey: String, endpoints: Vec<Url>) -> Config {
    Config { pubkey, endpoints, windows_install_mode: WindowsInstallMode::Passive, https_only: false }
}

/// The smallest bytes that pass the format's magic check, padded.
pub fn artifact(format: Format) -> Vec<u8> {
    let magic: &[u8] = match format {
        Format::Nsis => b"MZ",
        Format::Deb => b"!<arch>\ndebian-binary",
        Format::Rpm => &[0xED, 0xAB, 0xEE, 0xDB],
        Format::Pacman => &[0x28, 0xB5, 0x2F, 0xFD],
        Format::AppImage => b"\x7FELF\x02\x01\x01\x00AI\x02",
    };
    let mut bytes = magic.to_vec();
    bytes.extend((0..200_000u32).map(|i| (i % 251) as u8));
    bytes
}
