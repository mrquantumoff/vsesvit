//! Real profiles sign in to the real server, which signs them in with a mock OpenID Connect
//! provider, and sync through it. The browser side never talks to the provider: the test's
//! "browser tab" follows the redirects from the server to the provider, back to the server, and to
//! the profile's loopback port.
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
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::{Theme, keys};
use vsesvit_core::sync::DataType;
use vsesvit_core::vault::KeyStore;
use vsesvit_core::{OpenOptions, Profile, Url};
use vsesvit_sync::{Account, Error, Http, Round, SignIn};

const CLIENT_ID: &str = "vsesvit-test";
const CLIENT_SECRET: &str = "e2e-secret";

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
    /// Who the provider's page signs in; `deny` refuses.
    user: String,
    /// code -> (user, challenge, redirect_uri)
    codes: HashMap<String, (String, String, String)>,
    issued: u32,
}

/// Discovery, an authorize endpoint that signs `user` straight in, a token endpoint that checks the
/// server's secret and PKCE, and userinfo.
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
            let location = if s.user == "deny" {
                format!("{}?error=access_denied&error_description=refused+by+the+test&state={}", query["redirect_uri"], query["state"])
            } else {
                s.issued += 1;
                let code = format!("code-{}", s.issued);
                let user = s.user.clone();
                s.codes.insert(code.clone(), (user, query["code_challenge"].clone(), query["redirect_uri"].clone()));
                format!("{}?code={code}&state={}", query["redirect_uri"], query["state"])
            };
            ("302 Found", format!("Location: {location}\r\n"), String::new())
        }
        ("POST", "/token") => {
            assert_eq!(form["client_id"], CLIENT_ID);
            assert_eq!(form["client_secret"], CLIENT_SECRET, "the server is a confidential client");
            assert_eq!(form["grant_type"], "authorization_code");
            let (user, challenge, redirect) = s.codes.remove(&form["code"]).expect("a code is used once");
            assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())), challenge);
            assert_eq!(form["redirect_uri"], redirect);
            let claims = serde_json::json!({ "user": user });
            let access = format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()));
            ("200 OK", String::new(), serde_json::json!({ "access_token": access, "token_type": "Bearer", "expires_in": 60 }).to_string())
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
    start_server_with(bin, issuer, &[])
}

/// With `settings` over the defaults.
fn start_server_with(bin: &str, issuer: &str, settings: &[(&str, &str)]) -> Server {
    static SERVERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = TempDir::new(&format!("server-{}", SERVERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let database = format!("sqlite://{}?mode=rwc", dir.0.join("sync.db").display().to_string().replace('\\', "/"));
    let port = free_port();
    let mut child = Command::new(bin)
        .env("DATABASE_URL", database)
        .env("BIND_ADDRESS", format!("127.0.0.1:{port}"))
        .env("PUBLIC_URL", format!("http://127.0.0.1:{port}"))
        .env("OIDC_ISSUER", issuer)
        .env("OIDC_CLIENT_ID", CLIENT_ID)
        .env("OIDC_CLIENT_SECRET", CLIENT_SECRET)
        .env("MAX_BATCH", "7")
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "info")
        .envs(settings.iter().copied())
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

/// Opens the server's sign-in page as the browser tab would, following its redirects through the
/// provider and back to the loopback port, and returns what signing in came to.
fn try_sign_in(http: &Http, server: &str) -> Result<Account, Error> {
    let pending = SignIn::start(http, server).unwrap();
    let authorize = pending.authorize_url().to_owned();
    assert!(authorize.starts_with(server), "the browser opens the sync server, never the provider");
    let worker_http = http.clone();
    let worker = std::thread::spawn(move || pending.finish(&worker_http));
    let mut page = ureq::get(&authorize).call().unwrap();
    let page = page.body_mut().read_to_string().unwrap();
    assert!(page.contains("You can close this tab"), "{page}");
    worker.join().unwrap()
}

fn sign_in(profile: &mut Profile, http: &Http, server: &str) -> Account {
    let account = try_sign_in(http, server).unwrap();
    account.save(&mut profile.sync()).unwrap();
    account
}

/// Rounds until nothing is left, as a shell runs them.
fn sync(profile: &mut Profile, http: &Http) {
    sync_types(profile, http, &DataType::ALL);
}

fn sync_types(profile: &mut Profile, http: &Http, types: &[DataType]) {
    try_sync_types(profile, http, types).unwrap();
}

/// What a sync comes to in Settings.
fn try_sync_types(profile: &mut Profile, http: &Http, types: &[DataType]) -> Result<(), Error> {
    let mut account = Account::load(&mut profile.sync()).unwrap().expect("signed in");
    for _ in 0..50 {
        let round = Round::gather(&mut profile.sync(), account, types).unwrap();
        let finished = round.run(http).finish(&mut profile.sync());
        account = finished.account;
        if !finished.result?.again {
            return Ok(());
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

    provider.sign_in_as("bob");
    sign_in(&mut c, &http, &server.url);
    sync(&mut c, &http);
    assert!(toolbar_titles(&mut c).is_empty());

    let account = Account::load(&mut c.sync()).unwrap().unwrap();
    account.clone().revoke(&http);
    let round = Round::gather(&mut c.sync(), account, &DataType::ALL).unwrap();
    let finished = round.run(&http).finish(&mut c.sync());
    assert!(matches!(finished.result, Err(Error::SignInExpired)), "a session ended on the server asks for a new sign-in");

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
fn a_refusal_at_the_provider_fails_the_sign_in_with_its_reason() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    provider.sign_in_as("deny");
    match try_sign_in(&Http::new(), &server.url) {
        Err(Error::Refused(reason)) => assert_eq!(reason, "refused by the test"),
        other => panic!("{:?}", other.map(|a| a.server().to_owned())),
    }
}

#[test]
fn the_session_is_sealed_and_an_account_from_before_signs_in_again() {
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
    assert_eq!(Account::load(&mut a.sync()).unwrap(), Some(account));

    let plain = a.sync().engine_state("account").unwrap().unwrap();
    let session = a.sync().secret_state("account.session").unwrap().expect("the session is sealed");
    assert!(!session.is_empty());
    assert!(!plain.windows(session.len()).any(|w| w == session.as_slice()), "the plain JSON holds no session");
    sync(&mut a, &http);

    // An account as a version that signed in with the provider itself saved it.
    let mut old: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    old["provider"] = serde_json::json!({ "issuer": provider.issuer, "client_id": CLIENT_ID });
    a.sync().set_engine_state("account", old.to_string().as_bytes()).unwrap();
    a.sync().set_secret_state("account.session", &[]).unwrap();
    a.sync().set_secret_state("account.tokens", br#"{"access":"x","refresh":"y"}"#).unwrap();
    assert!(Account::load(&mut a.sync()).unwrap().is_none(), "it signs in again");

    Account::forget(&mut a.sync()).unwrap();
    assert!(a.sync().secret_state("account.tokens").unwrap().is_none(), "signing out clears what it left");
}

#[test]
fn a_device_whose_uploads_are_refused_still_receives_the_others_changes() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server_with(&bin, &provider.issuer, &[("MAX_ACCOUNT_RECORDS", "20")]);
    let http = Http::new();
    let (dir_a, dir_b) = (TempDir::new("quota-a"), TempDir::new("quota-b"));
    let (mut a, mut b) = (open(&dir_a), open(&dir_b));
    let bookmarks = [DataType::Bookmarks];
    let with_history = [DataType::Bookmarks, DataType::History];

    provider.sign_in_as("frank");
    sign_in(&mut a, &http, &server.url);
    sign_in(&mut b, &http, &server.url);
    let shared = a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Shared", &Url::parse("https://example.com/").unwrap()).unwrap();
    sync_types(&mut a, &http, &bookmarks);
    for i in 0..30 {
        b.history().record_visit(&Url::parse(&format!("https://example.com/{i}")).unwrap(), Transition::Link).unwrap();
    }
    let refused = try_sync_types(&mut b, &http, &with_history).expect_err("b's history fills the account");
    assert!(matches!(refused, Error::Server { status: 507, .. }), "{refused}");
    assert_eq!(toolbar_titles(&mut b), ["Shared"]);

    a.bookmarks().rename(shared, "Renamed").unwrap();
    sync_types(&mut a, &http, &bookmarks);
    let refused = try_sync_types(&mut b, &http, &with_history).expect_err("the refusal is what the sync comes to");
    assert!(matches!(refused, Error::Server { status: 507, .. }), "{refused}");
    assert_eq!(toolbar_titles(&mut b), ["Renamed"], "the download goes on");
    let account = Account::load(&mut b.sync()).unwrap().unwrap();
    let round = Round::gather(&mut b.sync(), account, &with_history).unwrap();
    assert!(!round.is_empty(), "the refused history is still waiting");
}

#[test]
fn a_device_with_stale_limits_learns_the_servers_after_a_refusal() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let (dir_a, dir_b) = (TempDir::new("limits-a"), TempDir::new("limits-b"));
    let (mut a, mut b) = (open(&dir_a), open(&dir_b));

    provider.sign_in_as("grace");
    sign_in(&mut a, &http, &server.url);
    // As if the server took 500 records an upload when this device signed in, and takes 7 now.
    let limits = |profile: &mut Profile| {
        let saved: serde_json::Value = serde_json::from_slice(&profile.sync().engine_state("account").unwrap().unwrap()).unwrap();
        saved["limits"]["max_batch"].as_u64().unwrap()
    };
    let mut saved: serde_json::Value = serde_json::from_slice(&a.sync().engine_state("account").unwrap().unwrap()).unwrap();
    saved["limits"]["max_batch"] = 500.into();
    a.sync().set_engine_state("account", saved.to_string().as_bytes()).unwrap();
    for i in 0..20 {
        let url = Url::parse(&format!("https://example.com/{i}")).unwrap();
        a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, &format!("Page {i}"), &url).unwrap();
    }
    sync_types(&mut a, &http, &[DataType::Bookmarks]);
    assert_eq!(limits(&mut a), 7);

    sign_in(&mut b, &http, &server.url);
    sync_types(&mut b, &http, &[DataType::Bookmarks]);
    assert_eq!(toolbar_titles(&mut b).len(), 20);
}
