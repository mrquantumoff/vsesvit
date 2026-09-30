//! # vsesvit-sync-proto
//!
//! The HTTP API between Vsesvit and a sync server, version 1. JSON bodies, bearer tokens from the
//! OpenID Connect provider the server names in [`ServerInfo`].
//!
//! | route                       | auth   | request          | response        |
//! |-----------------------------|--------|------------------|-----------------|
//! | `GET  /v1/info`             | none   |                  | [`ServerInfo`]  |
//! | `POST /v1/records`          | bearer | [`Upload`]       | [`Uploaded`]    |
//! | `GET  /v1/records?since=&limit=` | bearer |          | [`Page`]        |
//! | `DELETE /v1/account`        | bearer |                  | `204`           |
//!
//! The server is the dumbest one `vsesvit_core::sync` is designed for: it keeps the last uploaded
//! body per `(account, kind, id)` and never merges or reads it. Every stored write takes the
//! account's next sequence number, and a download lists records by it, so a client that stores
//! [`Page::cursor`] sees each later write exactly once. Its own uploads come back too, and
//! `apply` counts them unchanged.
//!
//! Errors are a status code and an [`ApiError`] body.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const PROTOCOL: u32 = 1;

pub const INFO_PATH: &str = "/v1/info";
pub const RECORDS_PATH: &str = "/v1/records";
pub const ACCOUNT_PATH: &str = "/v1/account";

/// What a client needs before it can sign in. Served without authentication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub protocol: u32,
    pub auth: AuthInfo,
    pub limits: Limits,
}

/// The OpenID Connect provider whose access tokens the server accepts. A client discovers the
/// endpoints from `{issuer}/.well-known/openid-configuration`, signs in with the authorization code
/// flow and PKCE as the public client `client_id`, and asks for `scopes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthInfo {
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    /// Loopback redirect URIs registered for `client_id`, in the order to try them. The client
    /// listens on the first port it can bind. Several, because some providers match the port
    /// exactly and another program may hold one.
    pub redirect_uris: Vec<String>,
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
