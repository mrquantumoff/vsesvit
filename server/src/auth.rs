//! Who a bearer token belongs to, asked of the provider's userinfo endpoint.
//!
//! Userinfo is the one check every OpenID Connect provider answers the same way: opaque and JWT
//! access tokens alike, revoked tokens refused, and `sub` the stable user id. A local JWT check
//! cannot stand in for it: Quadrant ID's access tokens carry the granted scopes in `sub`, and the
//! user id only in the ID token and userinfo.
//!
//! Userinfo does not say which app a token was issued to, so the token has to: a JWT naming its
//! client (`client_id`, RFC 9068, and `azp`). Every client it names must be one the server
//! accepts, and a token naming none is refused, so another app the user signed in to cannot read
//! their browser data with its own token. `OIDC_ALLOWED_CLIENT_IDS=*` turns this off, for
//! providers whose access tokens are opaque. The claims are read without checking the signature:
//! a token edited to name another client is one the provider's userinfo then refuses.
//!
//! Answers are cached for a few minutes, refusals too, and calls to the provider are capped, so a
//! flood of made-up tokens costs the provider a bounded number of calls.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tokio::sync::{OnceCell, Semaphore};

/// How long a token's userinfo answer is reused. A token revoked at the provider keeps working
/// here for at most this long.
const CACHE_FOR: Duration = Duration::from_secs(300);
/// How long a refused token stays refused without asking again.
const REFUSED_FOR: Duration = Duration::from_secs(60);
const CACHE_ENTRIES: usize = 10_000;
/// Calls to the provider at once, and how long a request waits for one before giving up.
const PROVIDER_CALLS: usize = 32;
const PROVIDER_WAIT: Duration = Duration::from_secs(10);
/// Larger discovery or userinfo answers are refused.
const MAX_PROVIDER_BODY: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("the access token was refused")]
    Unauthorized,
    #[error("the access token does not name this app as its client")]
    WrongClient,
    #[error("too many sign-in checks at once; try again")]
    Busy,
    #[error("the sign-in provider could not be reached: {0}")]
    Provider(String),
}

pub struct Verifier {
    issuer: String,
    allowed_client_ids: Option<Vec<String>>,
    http: reqwest::Client,
    userinfo_endpoint: OnceCell<String>,
    calls: Semaphore,
    cache: Mutex<HashMap<[u8; 32], Cached>>,
}

struct Cached {
    /// `None`: the provider refused the token.
    subject: Option<String>,
    until: Instant,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    userinfo_endpoint: String,
}

#[derive(Deserialize)]
struct UserInfo {
    sub: String,
}

/// What a JWT access token says about itself; unverified.
#[derive(Debug, PartialEq)]
struct Claims {
    /// Every client the token names, `client_id` and `azp`. `Err` when one is not a string.
    clients: Result<Vec<String>, ()>,
    /// Unix seconds; JWT NumericDate may be fractional.
    exp: Option<f64>,
}

impl Verifier {
    pub fn new(issuer: String, allowed_client_ids: Option<Vec<String>>) -> Verifier {
        // reqwest is built without a TLS provider of its own; an error means one is installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("vsesvit-sync-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("the HTTP client builds");
        Verifier {
            issuer,
            allowed_client_ids,
            http,
            userinfo_endpoint: OnceCell::new(),
            calls: Semaphore::new(PROVIDER_CALLS),
            cache: Mutex::default(),
        }
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The token's subject at the issuer.
    pub async fn subject(&self, token: &str) -> Result<String, AuthError> {
        let key: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let now = Instant::now();
        if let Some(hit) = self.cache.lock().expect("cache lock").get(&key).filter(|c| c.until > now) {
            return hit.subject.clone().ok_or(AuthError::Unauthorized);
        }
        let claims = jwt_claims(token);
        if let Some(allowed) = &self.allowed_client_ids {
            let names_only_allowed = match claims.as_ref().map(|c| &c.clients) {
                Some(Ok(clients)) => !clients.is_empty() && clients.iter().all(|c| allowed.contains(c)),
                _ => false,
            };
            if !names_only_allowed {
                return Err(AuthError::WrongClient);
            }
        }
        let unix_now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
        let expires_in = claims.and_then(|c| c.exp).map(|exp| exp - unix_now);
        if expires_in.is_some_and(|left| left <= 0.0) {
            return Err(AuthError::Unauthorized);
        }
        let subject = match self.ask_userinfo(token).await {
            Ok(subject) => Some(subject),
            Err(AuthError::Unauthorized) => None,
            Err(e) => return Err(e),
        };
        let lifetime = if subject.is_some() { CACHE_FOR } else { REFUSED_FOR };
        let until = now + expires_in.map_or(lifetime, |left| lifetime.min(Duration::from_secs_f64(left)));
        let mut cache = self.cache.lock().expect("cache lock");
        if cache.len() >= CACHE_ENTRIES {
            cache.retain(|_, c| c.until > now);
            if cache.len() >= CACHE_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(key, Cached { subject: subject.clone(), until });
        subject.ok_or(AuthError::Unauthorized)
    }

    async fn ask_userinfo(&self, token: &str) -> Result<String, AuthError> {
        let endpoint = self.userinfo_endpoint.get_or_try_init(|| self.discover()).await?;
        let _call = tokio::time::timeout(PROVIDER_WAIT, self.calls.acquire())
            .await
            .map_err(|_| AuthError::Busy)?
            .expect("the semaphore is never closed");
        let response = self.http.get(endpoint).bearer_auth(token).send().await.map_err(provider)?;
        match response.status().as_u16() {
            200 => {}
            400 | 401 | 403 => return Err(AuthError::Unauthorized),
            status => return Err(AuthError::Provider(format!("userinfo answered {status}"))),
        }
        let info: UserInfo = read_json(response).await?;
        if info.sub.is_empty() {
            return Err(AuthError::Provider("userinfo has an empty sub".to_owned()));
        }
        Ok(info.sub)
    }

    async fn discover(&self) -> Result<String, AuthError> {
        let url = format!("{}/.well-known/openid-configuration", self.issuer);
        let response = self.http.get(&url).send().await.map_err(provider)?;
        if !response.status().is_success() {
            return Err(AuthError::Provider(format!("{url} answered {}", response.status())));
        }
        let discovery: Discovery = read_json(response).await?;
        if discovery.issuer.trim_end_matches('/') != self.issuer {
            return Err(AuthError::Provider(format!("{url} names the issuer {}", discovery.issuer)));
        }
        if !crate::config::is_secure_url(&discovery.userinfo_endpoint) {
            return Err(AuthError::Provider(format!("{url} names an insecure userinfo endpoint")));
        }
        tracing::info!(userinfo = %discovery.userinfo_endpoint, "discovered the sign-in provider");
        Ok(discovery.userinfo_endpoint)
    }
}

fn provider(e: reqwest::Error) -> AuthError {
    AuthError::Provider(e.to_string())
}

/// Reads a JSON body of at most [`MAX_PROVIDER_BODY`] bytes.
async fn read_json<T: DeserializeOwned>(mut response: reqwest::Response) -> Result<T, AuthError> {
    let too_large = || AuthError::Provider("the provider's answer is too large".to_owned());
    if response.content_length().is_some_and(|len| len > MAX_PROVIDER_BODY as u64) {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(provider)? {
        if body.len() + chunk.len() > MAX_PROVIDER_BODY {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|e| AuthError::Provider(format!("unexpected answer: {e}")))
}

/// The claims of a JWT, unverified; `None` for an opaque token or a payload that is not a JSON
/// object.
fn jwt_claims(token: &str) -> Option<Claims> {
    let mut parts = token.split('.');
    let (Some(_), Some(payload), Some(_), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    let payload: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    let clients = ["client_id", "azp"]
        .iter()
        .filter_map(|name| payload.get(*name))
        .map(|value| value.as_str().map(str::to_owned).ok_or(()))
        .collect();
    Some(Claims { clients, exp: payload.get("exp").and_then(serde_json::Value::as_f64) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(payload: &str) -> String {
        format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(payload))
    }

    #[test]
    fn a_jwt_names_its_clients_whatever_the_shape_of_its_other_claims() {
        let claims = jwt_claims(&jwt(r#"{"client_id":"a","azp":"b","exp":4102444800.5,"extra":[1]}"#)).unwrap();
        assert_eq!(claims, Claims { clients: Ok(vec!["a".to_owned(), "b".to_owned()]), exp: Some(4102444800.5) });
        assert_eq!(jwt_claims(&jwt(r#"{"client_id":7}"#)).unwrap().clients, Err(()));
        assert_eq!(jwt_claims(&jwt(r#"{"sub":"x"}"#)).unwrap().clients, Ok(vec![]));
        assert!(jwt_claims("opaque-token").is_none());
        assert!(jwt_claims("a.b.c.d").is_none());
    }

    async fn refused(verifier: &Verifier, payload: &str) -> AuthError {
        verifier.subject(&jwt(payload)).await.unwrap_err()
    }

    #[tokio::test]
    async fn only_tokens_naming_nothing_but_accepted_clients_reach_the_provider() {
        let verifier = Verifier::new("http://127.0.0.1:1".to_owned(), Some(vec!["mine".to_owned()]));
        for payload in [
            r#"{"client_id":"theirs"}"#,
            r#"{"azp":"theirs"}"#,
            r#"{"client_id":"mine","azp":"theirs"}"#,
            r#"{"client_id":"theirs","exp":4102444800.5}"#,
            r#"{"client_id":["mine"]}"#,
            r#"{"sub":"no client"}"#,
        ] {
            assert!(matches!(refused(&verifier, payload).await, AuthError::WrongClient), "{payload}");
        }
        assert!(matches!(verifier.subject("opaque").await, Err(AuthError::WrongClient)));
    }

    #[tokio::test]
    async fn an_expired_token_is_refused_without_asking() {
        let verifier = Verifier::new("http://127.0.0.1:1".to_owned(), None);
        assert!(matches!(refused(&verifier, r#"{"exp":1000.5}"#).await, AuthError::Unauthorized));
    }
}
