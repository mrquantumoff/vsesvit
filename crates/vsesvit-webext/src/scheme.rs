//! `chrome-extension://<host>/<path>`: extension files served from disk, generated
//! background pages, and the `web_accessible_resources` gate for other origins.

use std::rc::{Rc, Weak};

use vsesvit_core::extensions::manifest::RelPath;
use webkit::prelude::*;
use webkit::{gio, glib, soup};

use crate::extension::{Extension, GENERATED_BACKGROUND, SCHEME};
use crate::mime;
use crate::runtime::Inner;

pub(crate) fn register(inner: &Rc<Inner>) {
    let weak: Weak<Inner> = Rc::downgrade(inner);
    inner.context.register_uri_scheme(SCHEME, move |request| match weak.upgrade() {
        Some(inner) => serve(&inner, request),
        None => respond(request, 500, "Service Unavailable", b"", "text/plain", false),
    });
    if let Some(manager) = inner.context.security_manager() {
        manager.register_uri_scheme_as_secure(SCHEME);
        manager.register_uri_scheme_as_cors_enabled(SCHEME);
    }
}

fn serve(inner: &Inner, request: &webkit::URISchemeRequest) {
    let uri = request.uri().map(String::from).unwrap_or_default();
    let Some((host, path)) = split_uri(&uri) else {
        return respond(request, 400, "Bad Request", b"", "text/plain", false);
    };
    let Some(ext) = inner.extension_by_host(&host) else {
        return respond(request, 404, "Not Found", b"", "text/plain", false);
    };
    let requester = request.web_view();
    let page_url = requester.as_ref().and_then(|v| v.uri()).map(String::from).unwrap_or_default();
    let same_origin = page_url.starts_with(&ext.base_url)
        || page_url == ext.base_url.trim_end_matches('/')
        || requester.as_ref().is_some_and(|v| ext.owns_view(v));
    if !same_origin && !ext.web_accessible(&path, &page_url) {
        log::debug!("{}: {} refused to {}", ext.id.as_str(), path, page_url);
        return respond(request, 403, "Forbidden", b"", "text/plain", false);
    }
    if path == GENERATED_BACKGROUND
        && let Some(html) = ext.generated_background_page()
    {
        return respond(request, 200, "OK", html.as_bytes(), "text/html", !same_origin);
    }
    let body = match RelPath::parse(&path).map(|rel| rel.resolve(&ext.dir)) {
        Ok(file) if file.is_file() => std::fs::read(&file).ok(),
        _ => None,
    };
    match body {
        Some(bytes) => respond(request, 200, "OK", &bytes, mime::for_path(&path), !same_origin),
        None => respond(request, 404, "Not Found", b"", "text/plain", false),
    }
}

/// `chrome-extension://host/a/b?q#f` -> (`host`, `a/b`, percent-decoded).
pub(crate) fn split_uri(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix(SCHEME)?.strip_prefix("://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.is_empty() {
        return None;
    }
    Some((host.to_ascii_lowercase(), percent_decode(path)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

fn respond(request: &webkit::URISchemeRequest, status: u32, reason: &str, body: &[u8], content_type: &str, cors: bool) {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from(body));
    let response = webkit::URISchemeResponse::new(&stream, body.len() as i64);
    response.set_status(status, Some(reason));
    response.set_content_type(content_type);
    let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
    headers.append("Cache-Control", "no-store");
    if cors {
        headers.append("Access-Control-Allow-Origin", "*");
    }
    response.set_http_headers(headers);
    request.finish_with_response(&response);
}

impl Inner {
    pub(crate) fn extension_by_host(&self, host: &str) -> Option<Rc<Extension>> {
        self.extensions.borrow().values().find(|e| e.host == host).cloned()
    }
}
