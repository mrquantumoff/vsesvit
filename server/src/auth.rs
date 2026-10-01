//! The server's side of signing someone in with its OpenID Connect provider: discovery, the
//! authorization URL, trading the provider's code for an access token, and asking userinfo who it
//! belongs to. Only a sign-in reaches the provider; requests for records use the server's own
//! sessions (`store::session_account`).
//!
//! Userinfo's `sub` identifies the person: every provider answers it the same way, where an access
//! token's own `sub` need not be the user (some providers put the granted scopes there).
//!
//! The server is the provider's client. With `OIDC_CLIENT_SECRET` it is a confidential one; it
//! always uses PKCE as well, which providers accept from confidential clients too.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tokio::sync::{OnceCell, Semaphore};

use crate::config::{Oidc, is_secure_url};

/// Calls to the provider at once, and how long a sign-in waits for one before giving up.
const PROVIDER_CALLS: usize = 16;
const PROVIDER_WAIT: Duration = Duration::from_secs(10);
/// Larger answers from the provider are refused.
const MAX_PROVIDER_BODY: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("the sign-in provider refused: {0}")]
    Refused(String),
    #[error("too many sign-ins at once; try again")]
    Busy,
    #[error("the sign-in provider could not be reached: {0}")]
    Provider(String),
}

/// Who the provider says signed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub subject: String,
    pub name: Option<String>,
}

pub struct Provider {
    oidc: Oidc,
    /// Where the provider sends people back: `{PUBLIC_URL}/v1/auth/callback`.
    callback: String,
    http: reqwest::Client,
    endpoints: OnceCell<Endpoints>,
    calls: Semaphore,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

struct Endpoints {
    authorization: String,
    token: String,
    userinfo: String,
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct UserInfo {
    sub: String,
    name: Option<String>,
    preferred_username: Option<String>,
    email: Option<String>,
}

impl Provider {
    pub fn new(oidc: Oidc, callback: String) -> Provider {
        // reqwest is built without a TLS provider of its own; an error means one is installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("vsesvit-sync-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("the HTTP client builds");
        Provider { oidc, callback, http, endpoints: OnceCell::new(), calls: Semaphore::new(PROVIDER_CALLS) }
    }

    pub fn issuer(&self) -> &str {
        &self.oidc.issuer
    }

    /// The provider's page for a sign-in whose `state` is its login id, with the PKCE challenge of
    /// `verifier`.
    pub async fn authorize_url(&self, state: &str, verifier: &str) -> Result<String, AuthError> {
        let endpoints = self.endpoints().await?;
        let mut url = reqwest::Url::parse(&endpoints.authorization).map_err(|e| AuthError::Provider(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.oidc.client_id)
            .append_pair("redirect_uri", &self.callback)
            .append_pair("scope", &self.oidc.scopes.join(" "))
            .append_pair("state", state)
            .append_pair("code_challenge", &challenge(verifier))
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    /// Trades the provider's `code` for an access token, and asks userinfo whose it is.
    pub async fn person(&self, code: &str, verifier: &str) -> Result<Person, AuthError> {
        let endpoints = self.endpoints().await?;
        let _call = tokio::time::timeout(PROVIDER_WAIT, self.calls.acquire())
            .await
            .map_err(|_| AuthError::Busy)?
            .expect("the semaphore is never closed");
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.callback.as_str()),
            ("client_id", self.oidc.client_id.as_str()),
            ("code_verifier", verifier),
        ];
        if let Some(secret) = &self.oidc.client_secret {
            form.push(("client_secret", secret));
        }
        let response = self.http.post(&endpoints.token).form(&form).send().await.map_err(provider)?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(match read_json::<TokenError>(response).await {
                Ok(e) => AuthError::Refused(e.error_description.unwrap_or(e.error)),
                Err(_) => AuthError::Provider(format!("the token endpoint answered {status}")),
            });
        }
        let token: TokenAnswer = read_json(response).await?;
        let response = self.http.get(&endpoints.userinfo).bearer_auth(&token.access_token).send().await.map_err(provider)?;
        if !response.status().is_success() {
            return Err(AuthError::Provider(format!("userinfo answered {}", response.status())));
        }
        let info: UserInfo = read_json(response).await?;
        if info.sub.is_empty() {
            return Err(AuthError::Provider("userinfo has an empty sub".to_owned()));
        }
        let name = info.name.or(info.preferred_username).or(info.email).filter(|n| !n.trim().is_empty());
        Ok(Person { subject: info.sub, name })
    }

    async fn endpoints(&self) -> Result<&Endpoints, AuthError> {
        self.endpoints.get_or_try_init(|| self.discover()).await
    }

    async fn discover(&self) -> Result<Endpoints, AuthError> {
        let url = format!("{}/.well-known/openid-configuration", self.oidc.issuer);
        let response = self.http.get(&url).send().await.map_err(provider)?;
        if !response.status().is_success() {
            return Err(AuthError::Provider(format!("{url} answered {}", response.status())));
        }
        let d: Discovery = read_json(response).await?;
        if d.issuer.trim_end_matches('/') != self.oidc.issuer {
            return Err(AuthError::Provider(format!("{url} names the issuer {}", d.issuer)));
        }
        for endpoint in [&d.authorization_endpoint, &d.token_endpoint, &d.userinfo_endpoint] {
            if !is_secure_url(endpoint) {
                return Err(AuthError::Provider(format!("{url} names an insecure endpoint {endpoint}")));
            }
        }
        tracing::info!(authorization = %d.authorization_endpoint, "discovered the sign-in provider");
        Ok(Endpoints { authorization: d.authorization_endpoint, token: d.token_endpoint, userinfo: d.userinfo_endpoint })
    }
}

/// The S256 PKCE challenge of `verifier` (RFC 7636 §4.2).
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// 256 random bits, base64url: a PKCE verifier, a login id, a one-time code or a session token.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    URL_SAFE_NO_PAD.encode(bytes)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_is_rfc_7636s_example() {
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn tokens_are_43_unreserved_characters_and_differ() {
        let t = random_token();
        assert_eq!(t.len(), 43);
        assert!(t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_ne!(t, random_token());
    }
}
