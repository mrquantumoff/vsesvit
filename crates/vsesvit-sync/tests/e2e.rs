//! Real profiles sign in through a mock OpenID Connect provider and sync through the real server.
//!
//! The server is a separate workspace (`server/`), so this test runs its binary. It is skipped
//! unless `VSESVIT_SYNC_SERVER_BIN` names it; `scripts/sync-e2e.sh` builds and runs both.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::prefs::{Theme, keys};
use vsesvit_core::sync::DataType;
use vsesvit_core::vault::KeyStore;
use vsesvit_core::{OpenOptions, Profile, Url};
use vsesvit_sync::{Account, Error, Http, Round, SignIn};

const CLIENT_ID: &str = "vsesvit-test";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("vsesvit-sync-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct ProviderState {
    user: String,
    /// code -> (user, challenge, redirect_uri)
    codes: HashMap<String, (String, String, String)>,
    /// refresh token -> user
    refresh: HashMap<String, String>,
    issued: u32,
    refreshes: u32,
    /// Seconds an access token lives; 1 makes every call refresh first.
    expires_in: u64,
}

/// Discovery, an authorize endpoint that signs `user` straight in, a token endpoint that checks
/// PKCE and rotates refresh tokens, and userinfo. Access tokens expire at once, so every call
/// refreshes first.
struct MockProvider {
    issuer: String,
    state: Arc<Mutex<ProviderState>>,
}

impl MockProvider {
    fn start() -> MockProvider {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(ProviderState::default()));
        let (issuer2, state2) = (issuer.clone(), Arc::clone(&state));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve(stream, &issuer2, &state2);
            }
        });
        MockProvider { issuer, state }
    }

    fn sign_in_as(&self, user: &str) {
        self.state.lock().unwrap().user = user.to_owned();
    }
}

fn serve(mut stream: TcpStream, issuer: &str, state: &Mutex<ProviderState>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).unwrap();
        if h.trim().is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap();
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
    }
    let mut body = vec![0; headers.get("content-length").map_or(0, |l| l.parse().unwrap())];
    reader.read_exact(&mut body).unwrap();
    let mut parts = line.split(' ');
    let (method, target) = (parts.next().unwrap(), parts.next().unwrap());
    let url = Url::parse(&format!("{issuer}{target}")).unwrap();
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let form: HashMap<String, String> = url::form_urlencoded::parse(&body).into_owned().collect();
    let mut s = state.lock().unwrap();
    let (status, extra, body) = match (method, url.path()) {
        ("GET", "/.well-known/openid-configuration") => (
            "200 OK",
            String::new(),
            serde_json::json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{issuer}/authorize"),
                "token_endpoint": format!("{issuer}/token"),
                "userinfo_endpoint": format!("{issuer}/userinfo"),
            })
            .to_string(),
        ),
        ("GET", "/authorize") => {
            assert_eq!(query["client_id"], CLIENT_ID);
            assert_eq!(query["code_challenge_method"], "S256");
            assert!(query["scope"].split(' ').any(|s| s == "openid"));
            s.issued += 1;
            let code = format!("code-{}", s.issued);
            let user = s.user.clone();
            s.codes.insert(code.clone(), (user, query["code_challenge"].clone(), query["redirect_uri"].clone()));
            let location = format!("{}?code={code}&state={}", query["redirect_uri"], query["state"]);
            ("302 Found", format!("Location: {location}\r\n"), String::new())
        }
        ("POST", "/token") => {
            assert_eq!(form["client_id"], CLIENT_ID);
            let user = match form["grant_type"].as_str() {
                "authorization_code" => {
                    let (user, challenge, redirect) = s.codes.remove(&form["code"]).expect("a code is used once");
                    assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())), challenge);
                    assert_eq!(form["redirect_uri"], redirect);
                    Some(user)
                }
                "refresh_token" => {
                    s.refreshes += 1;
                    s.refresh.remove(&form["refresh_token"])
                }
                other => panic!("grant {other}"),
            };
            match user {
                Some(user) => {
                    s.issued += 1;
                    let refresh = format!("rt-{}", s.issued);
                    s.refresh.insert(refresh.clone(), user.clone());
                    // A JWT naming this app, as Quadrant ID's are: the server refuses tokens that
                    // do not say which client they were issued to.
                    let claims = serde_json::json!({ "client_id": CLIENT_ID, "user": user, "n": s.issued });
                    let access = format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()));
                    let json = serde_json::json!({ "access_token": access, "token_type": "Bearer", "expires_in": s.expires_in.max(1), "refresh_token": refresh });
                    ("200 OK", String::new(), json.to_string())
                }
                None => ("400 Bad Request", String::new(), r#"{"error":"invalid_grant"}"#.to_owned()),
            }
        }
        ("GET", "/userinfo") => {
            let token = headers.get("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or_default();
            let claims: Option<serde_json::Value> = token
                .split('.')
                .nth(1)
                .and_then(|p| URL_SAFE_NO_PAD.decode(p).ok())
                .and_then(|p| serde_json::from_slice(&p).ok());
            match claims.as_ref().and_then(|c| c["user"].as_str()) {
                Some(user) => ("200 OK", String::new(), serde_json::json!({ "sub": user, "name": user }).to_string()),
                None => ("401 Unauthorized", String::new(), String::new()),
            }
        }
        _ => ("404 Not Found", String::new(), String::new()),
    };
    drop(s);
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

struct Server {
    child: Child,
    url: String,
    _dir: TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn start_server(bin: &str, issuer: &str) -> Server {
    static SERVERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = TempDir::new(&format!("server-{}", SERVERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let database = format!("sqlite://{}?mode=rwc", dir.0.join("sync.db").display().to_string().replace('\\', "/"));
    let mut child = Command::new(bin)
        .env("DATABASE_URL", database)
        .env("BIND_ADDRESS", "127.0.0.1:0")
        .env("OIDC_ISSUER", issuer)
        .env("OIDC_CLIENT_ID", CLIENT_ID)
        .env("OIDC_REDIRECT_URIS", format!("http://127.0.0.1:{}/callback", free_port()))
        .env("MAX_BATCH", "7")
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .spawn()
        .expect("the server starts");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let address = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|l| l.split_once("listening on ").map(|(_, a)| a.trim().to_owned()))
        .expect("the server says where it listens");
    std::thread::spawn(move || lines.for_each(drop));
    Server { child, url: format!("http://{address}"), _dir: dir }
}

/// `Basic` keeps each profile's vault key in its database, so a run never touches the developer's
/// keyring.
fn open(dir: &TempDir) -> Profile {
    Profile::open(&dir.0, OpenOptions { key_store: KeyStore::Basic, ..OpenOptions::default() }).unwrap()
}

/// Opens the provider's page as the browser tab would: following its redirect to the loopback.
fn sign_in(profile: &mut Profile, http: &Http, server: &str) -> Account {
    let pending = SignIn::start(http, server).unwrap();
    let authorize = pending.authorize_url().to_owned();
    let worker_http = http.clone();
    let worker = std::thread::spawn(move || pending.finish(&worker_http));
    let mut page = ureq::get(&authorize).call().unwrap();
    assert!(page.body_mut().read_to_string().unwrap().contains("signed in"));
    let account = worker.join().unwrap().unwrap();
    account.save(&mut profile.sync()).unwrap();
    account
}

/// Rounds until nothing is left, as a shell runs them.
fn sync(profile: &mut Profile, http: &Http) {
    sync_types(profile, http, &DataType::ALL);
}

fn sync_types(profile: &mut Profile, http: &Http, types: &[DataType]) {
    let mut account = Account::load(&mut profile.sync()).unwrap().expect("signed in");
    for _ in 0..50 {
        let round = Round::gather(&mut profile.sync(), account, types).unwrap();
        let finished = round.run(http).finish(&mut profile.sync());
        account = finished.account;
        if !finished.result.unwrap().again {
            return;
        }
    }
    panic!("sync did not settle");
}

fn toolbar_titles(profile: &mut Profile) -> Vec<String> {
    profile.bookmarks().children(BookmarkId::TOOLBAR).into_iter().map(|n| n.title).collect()
}

#[test]
fn two_devices_sync_through_the_server_and_another_account_sees_nothing() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let (dir_a, dir_b, dir_c) = (TempDir::new("a"), TempDir::new("b"), TempDir::new("c"));
    let (mut a, mut b, mut c) = (open(&dir_a), open(&dir_b), open(&dir_c));

    for i in 0..20 {
        let url = Url::parse(&format!("https://example.com/{i}")).unwrap();
        a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, &format!("Page {i}"), &url).unwrap();
    }
    a.prefs().set(&keys::THEME, &Theme::Dark).unwrap();

    provider.sign_in_as("alice");
    let account = sign_in(&mut a, &http, &server.url);
    assert_eq!(account.name(), Some("alice"));
    assert_eq!(account.server(), server.url);
    sync(&mut a, &http);

    sign_in(&mut b, &http, &server.url);
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), toolbar_titles(&mut a));
    assert_eq!(toolbar_titles(&mut b).len(), 20);
    assert_eq!(b.prefs().get(&keys::THEME), Theme::Dark);

    let first = b.bookmarks().children(BookmarkId::TOOLBAR)[0].id;
    b.bookmarks().remove(first).unwrap();
    sync(&mut b, &http);
    sync(&mut a, &http);
    assert_eq!(toolbar_titles(&mut a).len(), 19);
    assert!(!toolbar_titles(&mut a).contains(&"Page 0".to_owned()));
    assert!(Account::load(&mut a.sync()).unwrap().unwrap().last_synced().is_some());
    assert!(provider.state.lock().unwrap().refreshes > 0, "expired access tokens were refreshed");

    provider.sign_in_as("bob");
    sign_in(&mut c, &http, &server.url);
    sync(&mut c, &http);
    assert!(toolbar_titles(&mut c).is_empty());

    provider.state.lock().unwrap().refresh.clear();
    let account = Account::load(&mut c.sync()).unwrap().unwrap();
    let round = Round::gather(&mut c.sync(), account, &DataType::ALL).unwrap();
    let finished = round.run(&http).finish(&mut c.sync());
    assert!(matches!(finished.result, Err(Error::SignInExpired)), "a refused refresh token asks for a new sign-in");

    let account = Account::load(&mut a.sync()).unwrap().unwrap();
    let in_flight = Round::gather(&mut a.sync(), account, &DataType::ALL).unwrap().run(&http);
    Account::forget(&mut a.sync()).unwrap();
    assert!(Account::load(&mut a.sync()).unwrap().is_none());
    let finished = in_flight.finish(&mut a.sync());
    assert!(matches!(finished.result, Err(Error::SignedOut)));
    assert!(Account::load(&mut a.sync()).unwrap().is_none(), "a round that outlived its sign-in saves nothing");
}

#[test]
fn a_type_turned_off_neither_goes_up_nor_comes_down_until_it_is_turned_on() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let (dir_a, dir_b) = (TempDir::new("types-a"), TempDir::new("types-b"));
    let (mut a, mut b) = (open(&dir_a), open(&dir_b));
    let no_settings: Vec<DataType> = DataType::ALL.into_iter().filter(|t| *t != DataType::Settings).collect();
    let no_bookmarks: Vec<DataType> = DataType::ALL.into_iter().filter(|t| *t != DataType::Bookmarks).collect();

    provider.sign_in_as("carol");
    sign_in(&mut a, &http, &server.url);
    sign_in(&mut b, &http, &server.url);
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Kept", &Url::parse("https://example.com/").unwrap()).unwrap();
    a.prefs().set(&keys::THEME, &Theme::Dark).unwrap();
    sync_types(&mut a, &http, &no_settings);
    sync_types(&mut b, &http, &no_bookmarks);
    assert!(toolbar_titles(&mut b).is_empty(), "b does not sync bookmarks");
    assert_eq!(b.prefs().get(&keys::THEME), Theme::System, "a did not upload its settings");

    sync(&mut a, &http);
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["Kept"], "turning bookmarks on downloads what was skipped");
    assert_eq!(b.prefs().get(&keys::THEME), Theme::Dark, "turning settings on uploads what changed meanwhile");
}

#[test]
fn a_final_sync_goes_only_with_a_fresh_token_and_never_refreshes() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let (dir_a, dir_b) = (TempDir::new("final-a"), TempDir::new("final-b"));
    let (mut a, mut b) = (open(&dir_a), open(&dir_b));

    provider.sign_in_as("dave");
    sign_in(&mut a, &http, &server.url);
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Stale", &Url::parse("https://example.com/").unwrap()).unwrap();
    let refreshes = provider.state.lock().unwrap().refreshes;
    let account = Account::load(&mut a.sync()).unwrap().unwrap();
    let finished = Round::gather(&mut a.sync(), account, &DataType::ALL).unwrap().run_final(&http).finish(&mut a.sync());
    assert!(matches!(finished.result, Err(Error::NeedsRefresh)));
    assert_eq!(provider.state.lock().unwrap().refreshes, refreshes, "a final sync never refreshes");

    provider.state.lock().unwrap().expires_in = 3600;
    sign_in(&mut a, &http, &server.url);
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Fresh", &Url::parse("https://example.org/").unwrap()).unwrap();
    let account = Account::load(&mut a.sync()).unwrap().unwrap();
    let finished = Round::gather(&mut a.sync(), account, &DataType::ALL).unwrap().run_final(&http).finish(&mut a.sync());
    finished.result.unwrap();

    sign_in(&mut b, &http, &server.url);
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["Stale", "Fresh"], "the final sync uploaded");
}

#[test]
fn the_tokens_are_sealed_and_an_account_saved_before_the_vault_moves_into_it() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let dir = TempDir::new("vault");
    let mut a = open(&dir);
    provider.sign_in_as("erin");
    let account = sign_in(&mut a, &http, &server.url);

    let plain = a.sync().engine_state("account").unwrap().unwrap();
    assert!(!String::from_utf8_lossy(&plain).contains("\"access\""), "the plain JSON holds no tokens");
    assert!(a.sync().secret_state("account.tokens").unwrap().is_some());

    // An account as builds before the vault saved it: the tokens inside the plain JSON.
    let mut legacy: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    let sealed = a.sync().secret_state("account.tokens").unwrap().unwrap();
    legacy["tokens"] = serde_json::from_slice(&sealed).unwrap();
    a.sync().set_secret_state("account.tokens", &[]).unwrap();
    a.sync().set_engine_state("account", legacy.to_string().as_bytes()).unwrap();

    let loaded = Account::load(&mut a.sync()).unwrap().expect("still signed in");
    assert_eq!(loaded, account);
    assert!(a.sync().secret_state("account.tokens").unwrap().is_some(), "loading sealed the tokens");
    assert!(!String::from_utf8_lossy(&a.sync().engine_state("account").unwrap().unwrap()).contains("\"access\""));
    sync(&mut a, &http);

    Account::forget(&mut a.sync()).unwrap();
    assert!(a.sync().secret_state("account.tokens").unwrap().is_none());
    assert!(Account::load(&mut a.sync()).unwrap().is_none());
}
