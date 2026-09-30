//! Signing in: OpenID Connect discovery, then the authorization code flow with PKCE (RFC 7636)
//! for a native app, which receives the code on a loopback redirect (RFC 8252). The provider's
//! page opens in a browser tab; Vsesvit is the browser, so the shell opens it in a tab of its own.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vsesvit_sync_proto::{AuthInfo, ServerInfo};

use crate::engine::Account;
use crate::server::read;
use crate::{Error, Http, network, normalize_base_url, now_secs, server};

/// How long the provider's page may wait for the user.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// A token this close to expiring is refreshed before use.
const EXPIRY_MARGIN_SECS: u64 = 60;

/// The endpoints of a provider, from `{issuer}/.well-known/openid-configuration`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Provider {
    pub issuer: String,
    pub client_id: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
    pub revocation_endpoint: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    /// Unix seconds; `None` when the provider did not say.
    pub expires_at: Option<u64>,
}

impl Tokens {
    pub fn fresh(&self) -> bool {
        self.expires_at.is_none_or(|at| now_secs() + EXPIRY_MARGIN_SECS < at)
    }
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: Option<String>,
    revocation_endpoint: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct UserInfo {
    name: Option<String>,
    preferred_username: Option<String>,
    email: Option<String>,
}

fn discover(http: &Http, auth: &AuthInfo) -> Result<Provider, Error> {
    let issuer = normalize_base_url(&auth.issuer)?;
    let url = format!("{issuer}/.well-known/openid-configuration");
    let response = http.0.get(&url).header("Accept", "application/json").call().map_err(network(&url))?;
    let d: Discovery = read(response, &url)?;
    if d.issuer.trim_end_matches('/') != issuer {
        return Err(Error::Malformed(crate::host_of(&url)));
    }
    for endpoint in [Some(&d.authorization_endpoint), Some(&d.token_endpoint), d.userinfo_endpoint.as_ref(), d.revocation_endpoint.as_ref()]
        .into_iter()
        .flatten()
    {
        normalize_base_url(endpoint)?;
    }
    Ok(Provider {
        issuer,
        client_id: auth.client_id.clone(),
        authorization_endpoint: d.authorization_endpoint,
        token_endpoint: d.token_endpoint,
        userinfo_endpoint: d.userinfo_endpoint,
        revocation_endpoint: d.revocation_endpoint,
    })
}

/// A sign-in waiting for the provider to send the user back. Blocking; `Send`.
pub struct SignIn {
    server: String,
    info: ServerInfo,
    provider: Provider,
    listener: TcpListener,
    redirect_uri: String,
    redirect_path: String,
    verifier: String,
    state: String,
    authorize_url: String,
    cancelled: Arc<AtomicBool>,
}

impl SignIn {
    /// Asks the server which provider it trusts, discovers the provider and starts listening for
    /// the redirect.
    pub fn start(http: &Http, server: &str) -> Result<SignIn, Error> {
        let server = normalize_base_url(server)?;
        let info = server::info(http, &server)?;
        let provider = discover(http, &info.auth)?;
        let (listener, redirect_uri) = listen(&info.auth.redirect_uris)?;
        let redirect_path = url::Url::parse(&redirect_uri).map(|u| u.path().to_owned()).unwrap_or_else(|_| "/".to_owned());
        let verifier = random_token();
        let state = random_token();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorize_url = url::Url::parse(&provider.authorization_endpoint).map_err(|_| Error::Malformed(provider.issuer.clone()))?;
        authorize_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &provider.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", &info.auth.scopes.join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(SignIn {
            server,
            info,
            provider,
            listener,
            redirect_uri,
            redirect_path,
            verifier,
            state,
            authorize_url: authorize_url.into(),
            cancelled: Arc::default(),
        })
    }

    /// The provider's sign-in page, to open in a tab.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    /// Set it from any thread to make [`SignIn::finish`] return [`Error::Cancelled`].
    pub fn canceller(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Waits for the redirect, then trades the code for tokens. The account it returns has synced
    /// nothing yet.
    pub fn finish(self, http: &Http) -> Result<Account, Error> {
        let code = self.wait_for_code()?;
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("client_id", self.provider.client_id.as_str()),
            ("code_verifier", self.verifier.as_str()),
        ];
        let tokens = token_request(http, &self.provider, &form, None)?;
        let name = user_name(http, &self.provider, &tokens.access);
        Ok(Account::new(self.server, name, self.provider, tokens, self.info.limits))
    }

    fn wait_for_code(&self) -> Result<String, Error> {
        self.listener.set_nonblocking(true).map_err(|_| Error::NoFreePort(self.redirect_uri.clone()))?;
        let deadline = Instant::now() + SIGN_IN_TIMEOUT;
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            if Instant::now() > deadline {
                return Err(Error::TimedOut);
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = self.answer(stream) {
                        return result;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                Err(e) => log::warn!("sign-in listener: {e}"),
            }
        }
    }

    /// Reads one request. `None` when it was not the redirect (a favicon, a stray visit), so the
    /// wait goes on.
    fn answer(&self, mut stream: TcpStream) -> Option<Result<String, Error>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).ok()?;
        let target = line.split(' ').nth(1)?;
        let url = url::Url::parse(&format!("http://localhost{target}")).ok()?;
        if url.path() != self.redirect_path {
            respond(&mut stream, "404 Not Found", "Not found.");
            return None;
        }
        let param = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
        if param("state").as_deref() != Some(self.state.as_str()) {
            respond(&mut stream, "400 Bad Request", "This sign-in link is not the one Vsesvit is waiting for.");
            return None;
        }
        if let Some(error) = param("error") {
            let reason = param("error_description").unwrap_or(error);
            respond(&mut stream, "200 OK", "Sign-in was not completed. You can close this tab.");
            return Some(Err(Error::Refused(reason)));
        }
        let code = param("code")?;
        respond(&mut stream, "200 OK", "You are signed in to Vsesvit sync. You can close this tab.");
        Some(Ok(code))
    }
}

fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Vsesvit sync</title></head>\
         <body style=\"font-family:system-ui,sans-serif;margin:4em auto;max-width:32em\"><p>{message}</p></body></html>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Binds the first redirect URI's port that is free.
fn listen(redirect_uris: &[String]) -> Result<(TcpListener, String), Error> {
    for uri in redirect_uris {
        let Ok(url) = url::Url::parse(uri) else { continue };
        let (Some(host), Some(port)) = (url.host(), url.port()) else { continue };
        let ip = match host {
            url::Host::Ipv4(ip) => ip.into(),
            url::Host::Ipv6(ip) => ip.into(),
            url::Host::Domain(_) => continue,
        };
        if let Ok(listener) = TcpListener::bind(SocketAddr::new(ip, port)) {
            return Ok((listener, uri.clone()));
        }
    }
    Err(Error::NoFreePort(redirect_uris.join(", ")))
}

/// 256 random bits, base64url: a PKCE verifier (43 characters, RFC 7636 section 4.1) or a state.
pub(crate) fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Posts a form to the token endpoint. `previous_refresh` stays the refresh token when the
/// provider does not rotate it.
fn token_request(http: &Http, provider: &Provider, form: &[(&str, &str)], previous_refresh: Option<&str>) -> Result<Tokens, Error> {
    let url = &provider.token_endpoint;
    let mut response =
        http.0.post(url).header("Accept", "application/json").send_form(form.iter().copied()).map_err(network(url))?;
    let status = response.status().as_u16();
    if status == 400 || status == 401 {
        let error = response.body_mut().read_json::<TokenError>().ok();
        return Err(match error {
            Some(e) if e.error == "invalid_grant" => Error::SignInExpired,
            Some(e) => Error::Refused(e.error_description.unwrap_or(e.error)),
            None => Error::Refused(format!("the provider answered {status}")),
        });
    }
    let t: TokenResponse = read(response, url)?;
    Ok(Tokens {
        access: t.access_token,
        refresh: t.refresh_token.or_else(|| previous_refresh.map(str::to_owned)),
        expires_at: t.expires_in.map(|s| now_secs() + s),
    })
}

/// A new access token from the refresh token. Without one, the user must sign in again.
pub(crate) fn refresh(http: &Http, provider: &Provider, tokens: &Tokens) -> Result<Tokens, Error> {
    let Some(refresh) = tokens.refresh.as_deref() else {
        return Err(Error::SignInExpired);
    };
    let form = [("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", provider.client_id.as_str())];
    token_request(http, provider, &form, Some(refresh))
}

/// Best effort: tells the provider the refresh token is no longer needed.
pub(crate) fn revoke(http: &Http, provider: &Provider, tokens: &Tokens) {
    let (Some(url), Some(refresh)) = (&provider.revocation_endpoint, &tokens.refresh) else { return };
    let form = [("token", refresh.as_str()), ("token_type_hint", "refresh_token"), ("client_id", provider.client_id.as_str())];
    if let Err(e) = http.0.post(url).send_form(form) {
        log::info!("revoking the refresh token: {e}");
    }
}

/// What to call the account in Settings; `None` when userinfo does not say.
fn user_name(http: &Http, provider: &Provider, access: &str) -> Option<String> {
    let url = provider.userinfo_endpoint.as_ref()?;
    let response = http.0.get(url).header("Authorization", &format!("Bearer {access}")).call().ok()?;
    let info: UserInfo = read(response, url).ok()?;
    info.name.or(info.preferred_username).or(info.email).filter(|n| !n.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verifier_is_43_unreserved_characters() {
        let v = random_token();
        assert_eq!(v.len(), 43);
        assert!(v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_ne!(v, random_token());
    }

    #[test]
    fn the_first_free_redirect_port_is_taken() {
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = format!("http://127.0.0.1:{}/callback", taken.local_addr().unwrap().port());
        let free_port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let free = format!("http://127.0.0.1:{free_port}/callback");
        let (_listener, uri) = listen(&["https://example.com/cb".to_owned(), busy, free.clone()]).unwrap();
        assert_eq!(uri, free);
    }
}
