//! Configuration from environment variables (and a `.env` file, read in `main`). Checked whole at
//! startup, so a bad value stops the server before it listens.
//!
//! | variable                  | default                                   |
//! |---------------------------|-------------------------------------------|
//! | `DATABASE_URL`            | `sqlite://vsesvit-sync.db?mode=rwc`       |
//! | `RUN_MIGRATIONS`          | `true`; `false` refuses to start on pending migrations |
//! | `DATABASE_MAX_CONNECTIONS`| `10`                                      |
//! | `BIND_ADDRESS`            | `0.0.0.0:8080`                            |
//! | `OIDC_ISSUER`             | required                                  |
//! | `OIDC_CLIENT_ID`          | required                                  |
//! | `OIDC_SCOPES`             | `openid profile`                          |
//! | `OIDC_REDIRECT_URIS`      | `http://127.0.0.1:47801/callback` to `:47805` |
//! | `OIDC_ALLOWED_CLIENT_IDS` | `OIDC_CLIENT_ID`; `*` accepts tokens of any client |
//! | `MAX_BATCH`               | `500`                                     |
//! | `MAX_RECORD_BYTES`        | `1048576`                                 |
//! | `MAX_REQUEST_BYTES`       | `33554432`, also a download page's budget |
//! | `MAX_ACCOUNT_BYTES`       | `1073741824`                              |
//! | `MAX_ACCOUNT_RECORDS`     | `1000000`                                 |
//!
//! Lists are separated by spaces or commas.

use std::net::{IpAddr, SocketAddr};

use reqwest::Url;
use vsesvit_sync_proto::{AuthInfo, Limits};

use crate::store::Quota;

#[derive(Clone, Debug)]
pub struct Config {
    pub database: DatabaseConfig,
    pub bind: SocketAddr,
    pub auth: AuthInfo,
    /// `None` accepts an access token issued to any client.
    pub allowed_client_ids: Option<Vec<String>>,
    pub limits: Limits,
    pub quota: Quota,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub url: String,
    pub run_migrations: bool,
    /// Connections the pool opens at most. A Postgres shared with other services, or a role with
    /// a connection limit, needs this below its limit.
    pub max_connections: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} must be set")]
    Missing(&'static str),
    #[error("{name} is not valid: {value:?}")]
    Invalid { name: &'static str, value: String },
}

impl Config {
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
        let var = |name: &str| var(name).filter(|v| !v.trim().is_empty());
        let invalid = |name, value: &str| ConfigError::Invalid { name, value: value.to_owned() };
        let client_id = var("OIDC_CLIENT_ID").ok_or(ConfigError::Missing("OIDC_CLIENT_ID"))?.trim().to_owned();
        // No default provider: which one signs people in is the operator's choice, never a guess.
        let issuer = var("OIDC_ISSUER").ok_or(ConfigError::Missing("OIDC_ISSUER"))?.trim().trim_end_matches('/').to_owned();
        if !is_secure_url(&issuer) {
            return Err(invalid("OIDC_ISSUER", &issuer));
        }
        let scopes = var("OIDC_SCOPES").map_or_else(|| vec!["openid".to_owned(), "profile".to_owned()], |v| list(&v));
        if !scopes.iter().any(|s| s == "openid") {
            return Err(invalid("OIDC_SCOPES", &scopes.join(" ")));
        }
        let redirect_uris = var("OIDC_REDIRECT_URIS")
            .map_or_else(|| (47801..=47805).map(|port| format!("http://127.0.0.1:{port}/callback")).collect(), |v| list(&v));
        if redirect_uris.is_empty() {
            return Err(ConfigError::Missing("OIDC_REDIRECT_URIS"));
        }
        if let Some(bad) = redirect_uris.iter().find(|uri| !is_loopback_redirect(uri)) {
            return Err(invalid("OIDC_REDIRECT_URIS", bad));
        }
        let allowed_client_ids = match var("OIDC_ALLOWED_CLIENT_IDS") {
            Some(v) if v.trim() == "*" => None,
            Some(v) if list(&v).is_empty() => return Err(ConfigError::Missing("OIDC_ALLOWED_CLIENT_IDS")),
            Some(v) => Some(list(&v)),
            None => Some(vec![client_id.clone()]),
        };
        let limits = Limits {
            max_batch: parse(&var, "MAX_BATCH", 500)?,
            max_record_bytes: parse(&var, "MAX_RECORD_BYTES", 1 << 20)?,
            max_request_bytes: parse(&var, "MAX_REQUEST_BYTES", 32 << 20)?,
        };
        if !(1..=10_000).contains(&limits.max_batch) {
            return Err(invalid("MAX_BATCH", &limits.max_batch.to_string()));
        }
        if limits.max_request_bytes < 64 * 1024 {
            return Err(invalid("MAX_REQUEST_BYTES", &limits.max_request_bytes.to_string()));
        }
        // One record, base64 and all, must fit in a request and in a download page.
        if limits.max_record_bytes == 0 || limits.max_record_bytes > limits.max_request_bytes / 2 {
            return Err(invalid("MAX_RECORD_BYTES", &limits.max_record_bytes.to_string()));
        }
        let quota = Quota {
            max_bytes: parse(&var, "MAX_ACCOUNT_BYTES", 1 << 30)?,
            max_records: parse(&var, "MAX_ACCOUNT_RECORDS", 1_000_000)?,
        };
        if quota.max_bytes < i64::from(limits.max_record_bytes) {
            return Err(invalid("MAX_ACCOUNT_BYTES", &quota.max_bytes.to_string()));
        }
        if quota.max_records < 1 {
            return Err(invalid("MAX_ACCOUNT_RECORDS", &quota.max_records.to_string()));
        }
        let max_connections = parse(&var, "DATABASE_MAX_CONNECTIONS", 10)?;
        if !(1..=1000).contains(&max_connections) {
            return Err(invalid("DATABASE_MAX_CONNECTIONS", &max_connections.to_string()));
        }
        Ok(Config {
            database: DatabaseConfig {
                url: var("DATABASE_URL").unwrap_or_else(|| "sqlite://vsesvit-sync.db?mode=rwc".to_owned()),
                run_migrations: parse(&var, "RUN_MIGRATIONS", true)?,
                max_connections,
            },
            bind: parse(&var, "BIND_ADDRESS", "0.0.0.0:8080".parse().expect("valid address"))?,
            auth: AuthInfo { issuer, client_id, scopes, redirect_uris },
            allowed_client_ids,
            limits,
            quota,
        })
    }
}

fn list(value: &str) -> Vec<String> {
    value.split([' ', ',']).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

fn parse<T: std::str::FromStr>(var: &impl Fn(&str) -> Option<String>, name: &'static str, default: T) -> Result<T, ConfigError> {
    match var(name) {
        Some(value) => value.trim().parse().map_err(|_| ConfigError::Invalid { name, value }),
        None => Ok(default),
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        Some(url::Host::Domain(host)) => host == "localhost",
        None => false,
    }
}

/// HTTPS, or HTTP on the loopback interface for a provider run locally; no credentials, query or
/// fragment. OpenID Connect Discovery requires HTTPS, and an HTTP userinfo would hand out tokens.
pub fn is_secure_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else { return false };
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && is_loopback(&url));
    scheme_ok && url.host().is_some() && url.username().is_empty() && url.password().is_none() && url.fragment().is_none()
}

/// RFC 8252 section 7.3: a native client receives the code over http on a loopback IP literal,
/// at a port it can listen on.
fn is_loopback_redirect(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else { return false };
    let ip_literal = matches!(url.host(), Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)));
    url.scheme() == "http"
        && ip_literal
        && is_loopback(&url)
        && url.port().is_some_and(|port| port != 0)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        Config::from_env(|name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned()))
    }

    const ISSUER: (&str, &str) = ("OIDC_ISSUER", "https://idp.example.com/");

    fn with(name: &'static str, value: &'static str) -> Result<Config, ConfigError> {
        // The first match wins, so `name` overrides the defaults after it.
        config(&[(name, value), ("OIDC_CLIENT_ID", "abc"), ISSUER])
    }

    #[test]
    fn the_provider_and_the_client_id_are_required() {
        assert!(matches!(config(&[ISSUER]), Err(ConfigError::Missing("OIDC_CLIENT_ID"))));
        assert!(matches!(config(&[("OIDC_CLIENT_ID", "abc")]), Err(ConfigError::Missing("OIDC_ISSUER"))));
        let c = config(&[("OIDC_CLIENT_ID", "abc"), ISSUER]).unwrap();
        assert_eq!(c.auth.issuer, "https://idp.example.com");
        assert_eq!(c.auth.scopes, ["openid", "profile"]);
        assert_eq!(c.auth.redirect_uris.len(), 5);
        assert_eq!(c.allowed_client_ids, Some(vec!["abc".to_owned()]));
        assert!(c.database.run_migrations);
        assert_eq!(c.database.max_connections, 10);
        assert!(!with("RUN_MIGRATIONS", "false").unwrap().database.run_migrations);
        assert_eq!(with("DATABASE_MAX_CONNECTIONS", "3").unwrap().database.max_connections, 3);
    }

    #[test]
    fn lists_split_on_spaces_and_commas_and_a_star_accepts_every_client() {
        let c = config(&[("OIDC_CLIENT_ID", "abc"), ISSUER, ("OIDC_SCOPES", "openid, email"), ("OIDC_ALLOWED_CLIENT_IDS", "*")]).unwrap();
        assert_eq!(c.auth.scopes, ["openid", "email"]);
        assert_eq!(c.allowed_client_ids, None);
        assert!(with("OIDC_ALLOWED_CLIENT_IDS", " , ").is_err());
    }

    #[test]
    fn providers_are_https_except_on_loopback() {
        assert!(with("OIDC_ISSUER", "http://127.0.0.1:9000/").is_ok());
        assert!(with("OIDC_ISSUER", "http://idp.example.com").is_err());
        assert!(with("OIDC_ISSUER", "https://user@idp.example.com").is_err());
        assert!(with("OIDC_ISSUER", "idp.example.com").is_err());
    }

    #[test]
    fn redirects_are_loopback_ip_literals_with_a_port() {
        assert!(with("OIDC_REDIRECT_URIS", "http://[::1]:47801/cb").is_ok());
        for bad in [
            "https://example.com/cb",
            "http://127.0.0.1:47801@evil.example/callback",
            "http://localhost:47801/cb",
            "http://127.0.0.1/cb",
            "http://10.0.0.1:47801/cb",
            " , ",
        ] {
            assert!(with("OIDC_REDIRECT_URIS", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn limits_that_could_not_work_stop_the_server() {
        assert!(with("OIDC_SCOPES", "profile").is_err());
        for (name, value) in [
            ("MAX_BATCH", "lots"),
            ("MAX_BATCH", "0"),
            ("MAX_RECORD_BYTES", "0"),
            ("MAX_RECORD_BYTES", "33554432"),
            ("MAX_REQUEST_BYTES", "100"),
            ("MAX_ACCOUNT_BYTES", "10"),
            ("MAX_ACCOUNT_RECORDS", "0"),
            ("DATABASE_MAX_CONNECTIONS", "0"),
            ("DATABASE_MAX_CONNECTIONS", "-1"),
        ] {
            assert!(with(name, value).is_err(), "{name}={value}");
        }
    }
}
