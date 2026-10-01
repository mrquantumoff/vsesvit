//! Signing in to a sync server: the OAuth 2.0 authorization code flow with PKCE for a native app
//! (RFC 7636, RFC 8252), with the sync server as the authorization server. The server's page opens
//! in a browser tab (Vsesvit is the browser, so the shell opens it in a tab of its own), the server
//! signs the person in with whatever provider it uses, and sends the tab back to a loopback port
//! here with a code that only this process, holding the PKCE verifier, can trade for a session.
//!
//! Nothing here knows the provider: no issuer, client id, secret or provider token. The browser
//! talks to the sync server only.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use vsesvit_sync_proto::{AUTHORIZE_PATH, Limits, OAuthError, SESSION_PATH, TOKEN_PATH, TokenResponse};

use crate::engine::Account;
use crate::server::read;
use crate::{Error, Http, network, normalize_base_url, server};

/// How long the sign-in page may wait for the person.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const REDIRECT_PATH: &str = "/callback";
/// The most a request line may take, so a stray local client cannot hold up the redirect, or Cancel.
const MAX_REQUEST_LINE: usize = 8 << 10;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// A sign-in waiting for the server to send the browser back. Blocking; `Send`.
pub struct SignIn {
    server: String,
    limits: Limits,
    listener: TcpListener,
    redirect_uri: String,
    verifier: String,
    state: String,
    authorize_url: String,
    cancelled: Arc<AtomicBool>,
}

impl SignIn {
    /// Checks the server speaks this protocol, and starts listening for the redirect on a free
    /// loopback port.
    pub fn start(http: &Http, server: &str) -> Result<SignIn, Error> {
        let server = normalize_base_url(server)?;
        let info = server::info(http, &server)?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| Error::NoFreePort(e.to_string()))?;
        let port = listener.local_addr().map_err(|e| Error::NoFreePort(e.to_string()))?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}{REDIRECT_PATH}");
        let verifier = random_token();
        let state = random_token();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorize_url = url::Url::parse(&format!("{server}{AUTHORIZE_PATH}")).map_err(|_| Error::InvalidServer(server.clone()))?;
        authorize_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(SignIn {
            server,
            limits: info.limits,
            listener,
            redirect_uri,
            verifier,
            state,
            authorize_url: authorize_url.into(),
            cancelled: Arc::default(),
        })
    }

    /// The server's sign-in page, to open in a tab.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    /// Set it from any thread to make [`SignIn::finish`] return [`Error::Cancelled`].
    pub fn canceller(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Waits for the redirect, then trades the code for a session. The account it returns has
    /// synced nothing yet.
    pub fn finish(self, http: &Http) -> Result<Account, Error> {
        let code = self.wait_for_code()?;
        let url = format!("{}{TOKEN_PATH}", self.server);
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", self.verifier.as_str()),
        ];
        let mut response = http.0.post(&url).header("Accept", "application/json").send_form(form).map_err(network(&url))?;
        if response.status().as_u16() == 400 {
            let reason = response.body_mut().read_json::<OAuthError>().map_or_else(|_| "the server refused".to_owned(), |e| e.error_description.unwrap_or(e.error));
            return Err(Error::Refused(reason));
        }
        let session: TokenResponse = read(response, &url)?;
        Ok(Account::new(self.server, session.name, session.access_token, self.limits))
    }

    fn wait_for_code(&self) -> Result<String, Error> {
        self.listener.set_nonblocking(true).map_err(|e| Error::NoFreePort(e.to_string()))?;
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
                Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                Err(e) => log::warn!("sign-in listener: {e}"),
            }
        }
    }

    /// Reads one request. `None` when it was not the redirect (a favicon, a stray visit, a client
    /// too slow or long-winded), so the wait goes on.
    fn answer(&self, mut stream: TcpStream) -> Option<Result<String, Error>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));
        let started = Instant::now();
        let mut line = Vec::new();
        let mut chunk = [0; 1024];
        let end = loop {
            if let Some(end) = line.iter().position(|&b| b == b'\n') {
                break end;
            }
            if self.cancelled.load(Ordering::Relaxed) {
                return Some(Err(Error::Cancelled));
            }
            let left = REQUEST_TIMEOUT.checked_sub(started.elapsed())?;
            if line.len() > MAX_REQUEST_LINE {
                return None;
            }
            // Short reads, so Cancel is seen while a client is slow. Zero would mean no timeout.
            stream.set_read_timeout(Some(left.clamp(Duration::from_millis(1), Duration::from_millis(100)))).ok()?;
            match stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => line.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => return None,
            }
        };
        let line = std::str::from_utf8(&line[..end]).ok()?;
        let target = line.split(' ').nth(1)?;
        let url = url::Url::parse(&format!("http://localhost{target}")).ok()?;
        if url.path() != REDIRECT_PATH {
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
        respond(&mut stream, "200 OK", "Vsesvit is signing you in to sync. You can close this tab.");
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

/// Best effort: ends the session on the server.
pub(crate) fn sign_out(http: &Http, server: &str, token: &str) {
    let url = format!("{server}{SESSION_PATH}");
    if let Err(e) = http.0.delete(&url).header("Authorization", &format!("Bearer {token}")).call() {
        log::info!("ending the sync session: {e}");
    }
}

/// 256 random bits, base64url: a PKCE verifier (43 characters, RFC 7636 §4.1) or a state.
pub(crate) fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::mpsc;

    use super::*;

    /// A sign-in listening for the redirect with state `s`, waiting on a thread of its own.
    fn waiting() -> (SocketAddr, Arc<AtomicBool>, mpsc::Receiver<Result<String, Error>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let limits = Limits { max_batch: 1, max_record_bytes: 1, max_request_bytes: 1 };
        let state = "s".to_owned();
        let sign_in = SignIn { server: String::new(), limits, listener, redirect_uri: String::new(), verifier: String::new(), state, authorize_url: String::new(), cancelled: Arc::default() };
        let cancelled = sign_in.canceller();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || sender.send(sign_in.wait_for_code()));
        (address, cancelled, receiver)
    }

    /// A local client that sends `chunk` every `every`, never ending its request line, for up to 30 s.
    fn stall(address: SocketAddr, chunk: Vec<u8>, every: Duration) {
        let mut stream = TcpStream::connect(address).unwrap();
        std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(30);
            while Instant::now() < until && stream.write_all(&chunk).is_ok() {
                std::thread::sleep(every);
            }
        });
    }

    /// The browser coming back, just after the stalling client.
    fn redirect(address: SocketAddr) {
        std::thread::sleep(Duration::from_millis(100));
        let mut stream = TcpStream::connect(address).unwrap();
        std::thread::spawn(move || {
            stream.write_all(b"GET /callback?state=s&code=abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
            let _ = stream.read_to_end(&mut Vec::new());
        });
    }

    #[test]
    fn a_trickling_client_does_not_hold_up_the_redirect() {
        let (address, _, result) = waiting();
        stall(address, b"G".to_vec(), Duration::from_secs(1));
        redirect(address);
        assert_eq!(result.recv_timeout(Duration::from_secs(15)).unwrap().unwrap(), "abc");
    }

    #[test]
    fn an_endless_request_line_is_dropped() {
        let (address, _, result) = waiting();
        stall(address, vec![b'a'; 64 << 10], Duration::from_millis(50));
        redirect(address);
        // Well within the time a request may take: dropped for its length.
        assert_eq!(result.recv_timeout(Duration::from_secs(3)).unwrap().unwrap(), "abc");
    }

    #[test]
    fn cancel_works_while_a_client_stalls() {
        let (address, cancelled, result) = waiting();
        stall(address, b"G".to_vec(), Duration::from_secs(1));
        std::thread::sleep(Duration::from_secs(1));
        cancelled.store(true, Ordering::Relaxed);
        assert!(matches!(result.recv_timeout(Duration::from_secs(2)).unwrap(), Err(Error::Cancelled)));
    }

    #[test]
    fn a_verifier_is_43_unreserved_characters() {
        let v = random_token();
        assert_eq!(v.len(), 43);
        assert!(v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_ne!(v, random_token());
    }
}
