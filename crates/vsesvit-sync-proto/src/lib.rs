//! # vsesvit-sync-proto
//!
//! The HTTP API between Vsesvit and a sync server, version 2. JSON bodies; bearer tokens are
//! sessions the server itself issues.
//!
//! | route                             | auth   | request                | response            |
//! |-----------------------------------|--------|------------------------|---------------------|
//! | `GET  /v1/info`                   | none   |                        | [`ServerInfo`]      |
//! | `GET  /v1/auth/authorize?…`       | none   | a browser tab          | redirects           |
//! | `POST /v1/auth/token`             | none   | form, RFC 6749 §4.1.3  | [`TokenResponse`]   |
//! | `DELETE /v1/auth/session`         | bearer |                        | `204`               |
//! | `POST /v1/records`                | bearer | [`Upload`]             | [`Uploaded`]        |
//! | `GET  /v1/records?since=&limit=`  | bearer |                        | [`Page`]            |
//! | `DELETE /v1/account`              | bearer |                        | `204`               |
//!
//! Signing in is the OAuth 2.0 authorization code flow with PKCE for a native app (RFC 7636,
//! RFC 8252), with the sync server as the authorization server: the browser opens
//! `/v1/auth/authorize` with a loopback `redirect_uri` on any port, the server signs the person in
//! with its OpenID Connect provider, and redirects back with a code the browser trades at
//! `/v1/auth/token` for a session. The browser never learns who the provider is, and needs no
//! client id or secret: it talks to the sync server only.
//!
//! The server is the dumbest one `vsesvit_core::sync` is designed for: it keeps the last uploaded
//! body per `(account, kind, id)` and never merges or reads it. Vsesvit's bodies are ciphertext, and
//! its ids opaque (`vsesvit-sync`'s `crypto.rs`). Every stored write takes the
//! account's next sequence number, and a download lists records by it, so a client that stores
//! [`Page::cursor`] sees each later write exactly once. Its own uploads come back too, and
//! `apply` counts them unchanged.
//!
//! The server can go back to an older copy of an account, as when its database is restored from a
//! backup. A client then uploads all it holds and downloads everything again, from `since=0`, when
//! either of two things tells it so. A download whose `since`, or an upload whose
//! [`Upload::download_cursor`], is past every write the server holds for the account is answered
//! `409`; the upload stores nothing, so its own writes cannot hide the gap. And [`Page::epoch`]
//! changes, for every device of the account, once the server has answered such a `409`, or when
//! its operator marks a restore.
//!
//! Errors are a status code and an [`ApiError`] body.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const PROTOCOL: u32 = 2;

pub const INFO_PATH: &str = "/v1/info";
pub const AUTHORIZE_PATH: &str = "/v1/auth/authorize";
pub const TOKEN_PATH: &str = "/v1/auth/token";
/// Where the provider sends the person back to the server; the browser never calls it itself.
pub const CALLBACK_PATH: &str = "/v1/auth/callback";
pub const SESSION_PATH: &str = "/v1/auth/session";
pub const RECORDS_PATH: &str = "/v1/records";
pub const ACCOUNT_PATH: &str = "/v1/account";

/// Bytes of one record's id; core's longest ids are history URLs. The server rejects an upload
/// holding an empty or longer one.
pub const MAX_ID_BYTES: usize = 8 * 1024;

/// What a client learns before it signs in. Served without authentication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub protocol: u32,
    pub limits: Limits,
}

/// A session, from `/v1/auth/token`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Always `Bearer`.
    pub token_type: String,
    /// What to call the person, from the provider: a name, a username or an email address.
    #[serde(default)]
    pub name: Option<String>,
}

/// An OAuth error body (RFC 6749 §5.2), from `/v1/auth/token`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthError {
    pub error: String,
    #[serde(default)]
    pub error_description: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Records per upload and per download page.
    pub max_batch: u32,
    /// Bytes of one record's body. The server rejects an upload holding a larger one.
    pub max_record_bytes: u32,
    /// Bytes of one upload request, JSON and base64 included.
    pub max_request_bytes: u32,
}

/// One record as `vsesvit_core::sync::WireRecord` has it: `kind` is `Kind::code`, `body` is
/// opaque bytes, base64 in JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub kind: u8,
    pub id: String,
    #[serde(serialize_with = "to_base64", deserialize_with = "from_base64")]
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub records: Vec<Record>,
    /// The client's [`Page::cursor`]. Older clients leave it out, and 0 is never ahead.
    #[serde(default)]
    pub download_cursor: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uploaded {
    pub stored: u32,
}

/// Records written after `since`, oldest write first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub records: Vec<Record>,
    /// The `since` of the next request.
    pub cursor: u64,
    /// More records follow `cursor` already.
    pub more: bool,
    /// Changes when the server's copy of the account went back. Older servers leave it out.
    #[serde(default)]
    pub epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

fn to_base64<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&STANDARD.encode(bytes))
}

fn from_base64<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let text = String::deserialize(d)?;
    STANDARD.decode(text).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_body_travels_as_base64() {
        let record = Record { kind: 1, id: "a".to_owned(), body: b"{\"x\":1}".to_vec() };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(json, r#"{"kind":1,"id":"a","body":"eyJ4IjoxfQ=="}"#);
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), record);
    }

    #[test]
    fn a_body_that_is_not_base64_is_refused() {
        assert!(serde_json::from_str::<Record>(r#"{"kind":1,"id":"a","body":"%%"}"#).is_err());
    }
}
