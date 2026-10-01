//! The sync server's routes, blocking.

use serde::de::DeserializeOwned;
use vsesvit_sync_proto::{ACCOUNT_PATH, ApiError, INFO_PATH, Limits, PROTOCOL, Page, RECORDS_PATH, ServerInfo, Upload, Uploaded};

use crate::{Error, Http, network};

/// The largest answer read, but for a download page: ureq's own default.
const MAX_BODY_BYTES: u64 = 10 << 20;
/// The largest download page read, whatever limits a server names.
const MAX_PAGE_BYTES: u64 = 256 << 20;

pub(crate) fn info(http: &Http, server: &str) -> Result<ServerInfo, Error> {
    let url = format!("{server}{INFO_PATH}");
    let response = http.0.get(&url).header("Accept", "application/json").call().map_err(network(&url))?;
    let info: ServerInfo = read(response, &url)?;
    if info.protocol != PROTOCOL {
        return Err(Error::Protocol(info.protocol));
    }
    Ok(info)
}

pub(crate) fn upload(http: &Http, server: &str, token: &str, upload: &Upload) -> Result<Uploaded, Error> {
    let url = format!("{server}{RECORDS_PATH}");
    let request = http.0.post(&url).header("Authorization", &format!("Bearer {token}"));
    read(authorized(request.send_json(upload), &url)?, &url)
}

/// A page of up to `limits.max_batch` records. The server fills one to about
/// `limits.max_request_bytes` of JSON, or past it with one record alone, which is at most half of
/// that before base64.
pub(crate) fn download(http: &Http, server: &str, token: &str, since: u64, limits: Limits) -> Result<Page, Error> {
    let url = format!("{server}{RECORDS_PATH}?since={since}&limit={}", limits.max_batch);
    let request = http.0.get(&url).header("Authorization", &format!("Bearer {token}"));
    let max_bytes = (u64::from(limits.max_request_bytes) * 2).clamp(MAX_BODY_BYTES, MAX_PAGE_BYTES);
    read_at_most(authorized(request.call(), &url)?, &url, max_bytes)
}

pub(crate) fn delete_account(http: &Http, server: &str, token: &str) -> Result<(), Error> {
    let url = format!("{server}{ACCOUNT_PATH}");
    authorized(http.0.delete(&url).header("Authorization", &format!("Bearer {token}")).call(), &url).map(drop)
}

/// A request sent with the session. A session the server no longer knows (signed out elsewhere,
/// unused too long) means signing in again.
fn authorized(sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>, url: &str) -> Result<ureq::http::Response<ureq::Body>, Error> {
    let response = sent.map_err(network(url))?;
    match response.status().as_u16() {
        401 => Err(Error::SignInExpired),
        200..=299 => Ok(response),
        _ => Err(failure(response)),
    }
}

pub(crate) fn read<T: DeserializeOwned>(response: ureq::http::Response<ureq::Body>, url: &str) -> Result<T, Error> {
    read_at_most(response, url, MAX_BODY_BYTES)
}

fn read_at_most<T: DeserializeOwned>(mut response: ureq::http::Response<ureq::Body>, url: &str, max_bytes: u64) -> Result<T, Error> {
    if !response.status().is_success() {
        return Err(failure(response));
    }
    response.body_mut().with_config().limit(max_bytes).read_json().map_err(|e| {
        log::warn!("{url}: {e}");
        Error::Malformed(crate::host_of(url))
    })
}

fn failure(mut response: ureq::http::Response<ureq::Body>) -> Error {
    let status = response.status().as_u16();
    let message = response
        .body_mut()
        .read_json::<ApiError>()
        .map(|e| e.error)
        .unwrap_or_else(|_| response.status().canonical_reason().unwrap_or("error").to_owned());
    Error::Server { status, message }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use vsesvit_sync_proto::Record;

    use super::*;

    /// Answers one request with `status` and `body` as JSON.
    fn serve_once(status: &str, body: Vec<u8>) -> String {
        let status = status.to_owned();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        });
        address
    }

    #[test]
    fn a_download_page_over_ten_mebibytes_is_read() {
        let records = (0..12).map(|i| Record { kind: 1, id: i.to_string(), body: vec![b'x'; 1_000_000] }).collect();
        let page = serde_json::to_vec(&Page { records, cursor: 12, more: false, epoch: 0 }).unwrap();
        assert!(page.len() > 10 << 20);
        let server = serve_once("200 OK", page);
        let limits = Limits { max_batch: 500, max_record_bytes: 1 << 20, max_request_bytes: 32 << 20 };
        assert_eq!(download(&Http::new(), &server, "t", 0, limits).unwrap().records.len(), 12);
    }

    #[test]
    fn deleting_the_account_says_whether_the_server_did() {
        let delete = |status: &str, body: &str| {
            let limits = Limits { max_batch: 1, max_record_bytes: 1, max_request_bytes: 1 };
            let server = serve_once(status, body.as_bytes().to_vec());
            crate::engine::Account::new(server, None, "t".to_owned(), limits).delete_server_data(&Http::new()).map(drop)
        };
        assert!(delete("204 No Content", "").is_ok());
        assert!(matches!(delete("401 Unauthorized", ""), Err(Error::SignInExpired)));
        assert!(matches!(delete("500 Internal Server Error", r#"{"error":"x"}"#), Err(Error::Server { status: 500, message }) if message == "x"));
    }
}
