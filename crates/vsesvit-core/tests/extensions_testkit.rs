//! The self-test inputs: the fixture server and the probe CRX. Run with `--features testkit`.
#![cfg(feature = "testkit")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use vsesvit_core::extensions::crx::{self, VerifyPolicy};
use vsesvit_core::testkit::{self, FixtureServer};

fn get(server: &FixtureServer, target: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").expect("a complete response head");
    (String::from_utf8(response[..split].to_vec()).unwrap(), response[split + 4..].to_vec())
}

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../tests/fixtures/site/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn fixture_server_serves_the_site_and_records_hits() {
    let server = FixtureServer::start().unwrap();
    assert_eq!(server.url("/index.html").as_str(), format!("http://127.0.0.1:{}/index.html", server.port()));

    let (head, body) = get(&server, "/index.html");
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
    assert!(head.contains("Content-Type: text/html; charset=utf-8"), "{head}");
    assert!(head.contains("Cache-Control: no-store"), "{head}");
    assert_eq!(body, fixture("index.html"));
    assert!(String::from_utf8(body).unwrap().contains("<title>Vsesvit fixture</title>"));

    let (head, body) = get(&server, "/allowed.png?cache=1");
    assert!(head.contains("Content-Type: image/png"), "{head}");
    assert_eq!(body, fixture("allowed.png"));
    assert!(get(&server, "/vsesvit-blocked/pixel.png").0.contains("image/png"));
    let (head, body) = get(&server, "/download.bin");
    assert!(head.contains("Content-Type: application/octet-stream"), "{head}");
    assert_eq!(body, fixture("download.bin"));
    assert_eq!(get(&server, "/").1, fixture("index.html"));
    assert!(get(&server, "/favicon.ico").0.starts_with("HTTP/1.1 404"));
    assert!(get(&server, "/../Cargo.toml").0.starts_with("HTTP/1.1 404"));

    assert_eq!(server.hits(), ["/index.html", "/allowed.png", "/vsesvit-blocked/pixel.png", "/download.bin", "/", "/favicon.ico", "/../Cargo.toml"]);
}

#[test]
fn fixture_server_sets_a_cookie_only_on_set_cookie() {
    let server = FixtureServer::start().unwrap();
    let (head, body) = get(&server, "/set-cookie");
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
    assert!(head.contains("\r\nSet-Cookie: served=1; Path=/\r\n"), "{head}");
    assert!(String::from_utf8(body).unwrap().contains("<title>Cookie set</title>"));
    let (head, body) = get(&server, "/cookies.html");
    assert!(!head.contains("Set-Cookie"), "{head}");
    assert_eq!(body, fixture("cookies.html"));
    assert_eq!(get(&server, "/cookie-frame.html").1, fixture("cookie-frame.html"));
}

#[test]
fn an_idle_connection_does_not_block_other_requests() {
    let server = FixtureServer::start().unwrap();
    let _speculative = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    assert!(get(&server, "/page2.html").0.starts_with("HTTP/1.1 200 OK"));
}

#[test]
fn a_tls_handshake_is_refused_at_once() {
    let server = FixtureServer::start().unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    stream.write_all(&[0x16, 0x03, 0x01, 0x02, 0x00]).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("closed before the idle timeout");
    assert!(response.is_empty());
    assert!(server.hits().is_empty());
}

#[test]
fn fixture_server_stops_on_drop() {
    let server = FixtureServer::start().unwrap();
    let port = server.port();
    assert!(get(&server, "/index.html").0.starts_with("HTTP/1.1 200"));
    drop(server);
    assert!(TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)).is_err());
}

#[test]
fn probe_crx_carries_the_probe_files_under_probe_id() {
    let bytes = testkit::probe_crx();
    let parsed = crx::parse(&bytes).unwrap();
    assert_eq!(crx::verify(&parsed, &VerifyPolicy::AnyDeveloperKey).unwrap().id.as_str(), testkit::PROBE_ID);
    let names: Vec<&str> = testkit::PROBE_FILES.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["manifest.json", "background.js", "content.js", "popup.html", "popup.js", "rules.json"]);
    for (name, bytes) in testkit::PROBE_FILES {
        let on_disk = std::fs::read(format!("{}/../../tests/fixtures/extensions/probe/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(*bytes, on_disk.as_slice(), "{name}");
    }
}
