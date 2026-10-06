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
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use vsesvit_core::bookmarks::{BookmarkId, InsertAt};
use vsesvit_core::history::Transition;
use vsesvit_core::prefs::{Theme, keys};
use vsesvit_core::crdt::Seq;
use vsesvit_core::sync::{DataType, Kind};
use vsesvit_core::vault::KeyStore;
use vsesvit_core::{OpenOptions, Profile, Url};
use vsesvit_sync::{Account, Encryption, Error, Http, Passphrase, PassphraseJob, Round, SignIn};
use vsesvit_sync_proto::{Page, RECORDS_PATH, Record, Upload};

const CLIENT_ID: &str = "vsesvit-test";
const CLIENT_SECRET: &str = "e2e-secret";
const PASSPHRASE: &str = "correct horse battery staple";

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
    command: Command,
    url: String,
    dir: TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The database files a stopped server leaves, its write-ahead log included.
const DATABASE_FILES: [&str; 2] = ["sync.db", "sync.db-wal"];

impl Server {
    /// Copies the database aside, as a backup taken now.
    fn back_up(&mut self) -> TempDir {
        let backup = TempDir::new(&format!("{}-backup", self.dir.0.file_name().unwrap().to_string_lossy()));
        self.restart(&[], |db| {
            for file in DATABASE_FILES {
                let _ = std::fs::copy(db.join(file), backup.0.join(file));
            }
        });
        backup
    }

    /// Puts `backup` in place of the database, sessions and all, and starts again with `settings`
    /// added.
    fn restore(&mut self, backup: &TempDir, settings: &[(&str, &str)]) {
        self.restart(settings, |db| {
            for file in DATABASE_FILES.into_iter().chain(["sync.db-shm"]) {
                let _ = std::fs::remove_file(db.join(file));
                let _ = std::fs::copy(backup.0.join(file), db.join(file));
            }
        });
    }

    /// Stops the server, lets `offline` change its directory, and starts it again at its address.
    fn restart(&mut self, settings: &[(&str, &str)], offline: impl FnOnce(&Path)) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        offline(&self.dir.0);
        self.command.envs(settings.iter().copied());
        let (child, url) = spawn(&mut self.command);
        self.child = child;
        assert_eq!(url, self.url);
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
    let mut command = Command::new(bin);
    command
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
        .stdout(Stdio::piped());
    let (child, url) = spawn(&mut command);
    Server { child, command, url, dir }
}

/// Starts the server and waits until it listens. Returns its address.
fn spawn(command: &mut Command) -> (Child, String) {
    let mut child = command.spawn().expect("the server starts");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let address = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|l| l.split_once("listening on ").map(|(_, a)| a.trim().to_owned()))
        .expect("the server says where it listens");
    std::thread::spawn(move || lines.for_each(drop));
    (child, format!("http://{address}"))
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

/// Signs in, looks for the account's key record, sets the passphrase or enters it as Settings
/// asks, and syncs with it.
fn sign_in(profile: &mut Profile, http: &Http, server: &str) -> Account {
    sign_in_only(profile, http, server);
    unlock(profile, PASSPHRASE).unwrap();
    sync(profile, http);
    Account::load(&mut profile.sync()).unwrap().unwrap()
}

/// Signs in, and syncs until the device knows what to ask for its passphrase.
fn sign_in_only(profile: &mut Profile, http: &Http, server: &str) {
    let account = try_sign_in(http, server).unwrap();
    account.save_signed_in(&mut profile.sync()).unwrap();
    sync(profile, http);
}

/// Sets, enters or changes the passphrase, whichever the account asks for.
fn unlock(profile: &mut Profile, passphrase: &str) -> Result<(), Error> {
    let account = Account::load(&mut profile.sync()).unwrap().unwrap();
    let job = PassphraseJob::new(account, Passphrase::new(passphrase.to_owned()).unwrap()).expect("the account says what it needs");
    job.run().finish(&mut profile.sync()).map(drop)
}

fn encryption(profile: &mut Profile) -> Encryption {
    Account::load(&mut profile.sync()).unwrap().unwrap().encryption()
}

/// The server as a malicious one sees it: every record the account holds, through the session.
fn server_records(profile: &mut Profile, server: &str) -> Vec<Record> {
    let session = session(profile);
    let mut records = Vec::new();
    let mut since = 0;
    loop {
        let mut response = ureq::get(&format!("{server}{RECORDS_PATH}?since={since}"))
            .header("Authorization", &format!("Bearer {session}"))
            .call()
            .unwrap();
        let page: Page = response.body_mut().read_json().unwrap();
        records.extend(page.records);
        since = page.cursor;
        if !page.more {
            return records;
        }
    }
}

/// Stores `records` as they are, as a malicious server, or an older Vsesvit, would.
fn put_records(profile: &mut Profile, server: &str, records: Vec<Record>) {
    let session = session(profile);
    ureq::post(&format!("{server}{RECORDS_PATH}"))
        .header("Authorization", &format!("Bearer {session}"))
        .send_json(Upload { records, download_cursor: 0 })
        .unwrap();
}

fn session(profile: &mut Profile) -> String {
    String::from_utf8(profile.sync().secret_state("account.session").unwrap().unwrap()).unwrap()
}

/// What an older Vsesvit uploaded of `profile`'s bookmarks: plaintext under their real ids.
fn plaintext_bookmarks(profile: &mut Profile) -> Vec<Record> {
    let batch = profile.sync().changes_since(Kind::Bookmarks, Seq(0), 1000).unwrap();
    batch.records.into_iter().map(|r| Record { kind: r.kind.code(), id: r.id, body: r.body }).collect()
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
        let synced = finished.result?;
        if !synced.again {
            return synced.refused.map_or(Ok(()), Err);
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

    // The page a refused round brings still reaches the shell, which applies what it changed.
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Added", &Url::parse("https://example.com/added").unwrap()).unwrap();
    sync_types(&mut a, &http, &bookmarks);
    let mut account = Account::load(&mut b.sync()).unwrap().unwrap();
    let mut bookmarks_changed = false;
    let mut last = None;
    for _ in 0..50 {
        let round = Round::gather(&mut b.sync(), account, &with_history).unwrap();
        let finished = round.run(&http).finish(&mut b.sync());
        account = finished.account;
        let synced = finished.result.expect("the round's page comes with the refusal");
        bookmarks_changed |= synced.report.changed.bookmarks;
        if !synced.again {
            last = Some(synced);
            break;
        }
    }
    let last = last.expect("sync settles");
    assert!(matches!(last.refused, Some(Error::Server { status: 507, .. })), "the refusal is what the sync comes to");
    assert!(bookmarks_changed, "a round reported a's bookmark");
    assert_eq!(toolbar_titles(&mut b), ["Renamed", "Added"]);
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

#[test]
fn a_device_ahead_of_a_restored_server_uploads_what_it_lost_though_its_new_changes_go_first() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let mut server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let (dir_a, dir_b) = (TempDir::new("restore-a"), TempDir::new("restore-b"));
    let (mut a, mut b) = (open(&dir_a), open(&dir_b));
    let url = Url::parse("https://example.com/").unwrap();

    provider.sign_in_as("heidi");
    sign_in(&mut a, &http, &server.url);
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Before", &url).unwrap();
    sync(&mut a, &http);
    let backup = server.back_up();
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Lost", &url).unwrap();
    sync(&mut a, &http);
    server.restore(&backup, &[]);

    // Uploaded first, these pass a's cursor before its download could see that it is ahead.
    for i in 0..5 {
        a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, &format!("New {i}"), &url).unwrap();
    }
    let http = Http::new();
    sync(&mut a, &http);
    sign_in(&mut b, &http, &server.url);
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["Before", "Lost", "New 0", "New 1", "New 2", "New 3", "New 4"]);
}

/// a and b each upload a bookmark after a backup, and the server goes back to it. b syncs first
/// and uploads more than a's cursor is ahead by, so a's own cursor no longer shows the restore;
/// a fresh device c sees what was lost only if a uploads everything again too. With `b_ahead`, b
/// synced past the backup, so the server sees that b's cursor is ahead.
fn restored_server_test(name: &str, b_ahead: bool, settings: &[(&str, &str)]) {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let mut server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let dirs = [TempDir::new(&format!("{name}-a")), TempDir::new(&format!("{name}-b")), TempDir::new(&format!("{name}-c"))];
    let [mut a, mut b, mut c] = dirs.each_ref().map(open);
    let url = Url::parse("https://example.com/").unwrap();
    let add = |profile: &mut Profile, title: &str| {
        profile.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, title, &url).unwrap();
    };

    provider.sign_in_as(name);
    sign_in(&mut a, &http, &server.url);
    sign_in(&mut b, &http, &server.url);
    for i in 0..20 {
        add(&mut a, &format!("Page {i}"));
    }
    sync(&mut a, &http);
    sync(&mut b, &http);
    let backup = server.back_up();
    if b_ahead {
        add(&mut b, "From b");
        sync(&mut b, &http);
    }
    add(&mut a, "From a");
    sync(&mut a, &http);
    server.restore(&backup, settings);

    let http = Http::new();
    if !b_ahead {
        for i in 0..5 {
            add(&mut b, &format!("From b {i}"));
        }
    }
    sync(&mut b, &http);
    sync(&mut a, &http);
    sign_in(&mut c, &http, &server.url);
    sync(&mut c, &http);
    let titles = toolbar_titles(&mut c);
    assert_eq!(titles.len(), if b_ahead { 22 } else { 26 }, "{titles:?}");
    assert!(titles.contains(&"From a".to_owned()), "{titles:?}");
}

#[test]
fn a_restore_one_device_notices_is_synced_again_by_every_device() {
    restored_server_test("ivan", true, &[]);
}

#[test]
fn a_restore_the_operator_marks_with_epoch_is_synced_again_by_every_device() {
    restored_server_test("judy", false, &[("EPOCH", "1")]);
}

fn holds(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

#[test]
fn the_server_holds_only_ciphertext_and_a_device_without_the_passphrase_syncs_nothing() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let (dir_a, dir_b) = (TempDir::new("sealed-a"), TempDir::new("sealed-b"));
    let (mut a, mut b) = (open(&dir_a), open(&dir_b));
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Secret title", &Url::parse("https://secret.example/path").unwrap()).unwrap();
    a.history().record_visit(&Url::parse("https://secret.example/visited").unwrap(), Transition::Link).unwrap();
    a.prefs().set(&keys::THEME, &Theme::Dark).unwrap();

    provider.sign_in_as("kim");
    sign_in(&mut a, &http, &server.url);
    let records = server_records(&mut a, &server.url);
    assert_eq!(records.iter().filter(|r| r.kind == 200).count(), 1, "one key record");
    assert!(records.iter().filter(|r| r.kind == 201).count() >= 3);
    assert!(records.iter().all(|r| r.kind == 200 || r.kind == 201), "only sealed records");
    for record in &records {
        assert!(!holds(record.id.as_bytes(), "secret") && !holds(&record.body, "secret"), "{record:?}");
        assert!(!holds(&record.body, "dark"));
    }

    sign_in_only(&mut b, &http, &server.url);
    assert_eq!(encryption(&mut b), Encryption::Enter);
    b.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "From b", &Url::parse("https://example.com/b").unwrap()).unwrap();
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["From b"], "nothing comes down without the passphrase");
    assert_eq!(server_records(&mut b, &server.url), records, "and nothing goes up");
    assert!(matches!(unlock(&mut b, "correct horse battery stapler"), Err(Error::WrongPassphrase)));
    assert_eq!(encryption(&mut b), Encryption::Enter, "a wrong passphrase changes nothing");

    unlock(&mut b, PASSPHRASE).unwrap();
    sync(&mut b, &http);
    sync(&mut a, &http);
    let sorted = |mut titles: Vec<String>| {
        titles.sort();
        titles
    };
    assert_eq!(sorted(toolbar_titles(&mut b)), ["From b", "Secret title"]);
    assert_eq!(sorted(toolbar_titles(&mut a)), ["From b", "Secret title"]);
    assert_eq!(b.prefs().get(&keys::THEME), Theme::Dark);
}

#[test]
fn an_account_synced_before_encryption_is_sealed_when_its_passphrase_is_set() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let dirs = [TempDir::new("convert-a"), TempDir::new("convert-retired"), TempDir::new("convert-b")];
    let [mut a, mut retired, mut b] = dirs.each_ref().map(open);
    let url = Url::parse("https://example.com/").unwrap();
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "On a", &url).unwrap();
    retired.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Only on the server", &url).unwrap();

    provider.sign_in_as("lee");
    sign_in_only(&mut a, &http, &server.url);
    // As an older Vsesvit left the account: plaintext from a and from a device that is gone, and
    // a's account saved before it kept whether it trusts plaintext.
    let plaintext: Vec<Record> = plaintext_bookmarks(&mut a).into_iter().chain(plaintext_bookmarks(&mut retired)).collect();
    put_records(&mut a, &server.url, plaintext);
    let mut saved: serde_json::Value = serde_json::from_slice(&a.sync().engine_state("account").unwrap().unwrap()).unwrap();
    saved.as_object_mut().unwrap().remove("plaintext_trusted");
    a.sync().set_engine_state("account", saved.to_string().as_bytes()).unwrap();
    sync(&mut a, &http);
    assert_eq!(encryption(&mut a), Encryption::Set);

    unlock(&mut a, PASSPHRASE).unwrap();
    sync(&mut a, &http);
    assert!(toolbar_titles(&mut a).contains(&"Only on the server".to_owned()), "{:?}", toolbar_titles(&mut a));
    assert_eq!(server_records(&mut a, &server.url).iter().filter(|r| r.kind == 200).count(), 1, "the key record went up after");

    sign_in(&mut b, &http, &server.url);
    let mut titles = toolbar_titles(&mut b);
    titles.sort();
    assert_eq!(titles, ["On a", "Only on the server"]);
}

#[test]
fn a_new_sign_in_seals_none_of_the_plaintext_a_server_shows_it() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let dirs = [TempDir::new("no-convert-a"), TempDir::new("no-convert-made-up"), TempDir::new("no-convert-b")];
    let [mut a, mut made_up, mut b] = dirs.each_ref().map(open);
    made_up.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Made up", &Url::parse("https://evil.example/").unwrap()).unwrap();

    provider.sign_in_as("mia");
    sign_in_only(&mut a, &http, &server.url);
    put_records(&mut a, &server.url, plaintext_bookmarks(&mut made_up));
    sync(&mut a, &http);
    assert_eq!(encryption(&mut a), Encryption::Set);
    unlock(&mut a, PASSPHRASE).unwrap();
    sync(&mut a, &http);
    assert!(toolbar_titles(&mut a).is_empty());
    sign_in(&mut b, &http, &server.url);
    assert!(toolbar_titles(&mut b).is_empty());
}

#[test]
fn a_changed_passphrase_stops_the_other_devices_until_it_is_entered_there() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let dirs = [TempDir::new("change-a"), TempDir::new("change-b"), TempDir::new("change-c")];
    let [mut a, mut b, mut c] = dirs.each_ref().map(open);
    let url = Url::parse("https://example.com/").unwrap();
    let first = a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "First", &url).unwrap();

    provider.sign_in_as("noor");
    sign_in(&mut a, &http, &server.url);
    sign_in(&mut b, &http, &server.url);
    assert_eq!(toolbar_titles(&mut b), ["First"]);

    unlock(&mut a, "a new passphrase").unwrap();
    sync(&mut a, &http);
    sync(&mut b, &http);
    assert_eq!(encryption(&mut b), Encryption::Changed);
    a.bookmarks().rename(first, "Renamed").unwrap();
    sync(&mut a, &http);
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["First"], "b reads nothing sealed with the new key");
    assert!(matches!(unlock(&mut b, PASSPHRASE), Err(Error::WrongPassphrase)), "the old passphrase opens nothing");

    unlock(&mut b, "a new passphrase").unwrap();
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["Renamed"]);
    b.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "From b", &url).unwrap();
    sync(&mut b, &http);
    sync(&mut a, &http);
    assert_eq!(toolbar_titles(&mut a), ["Renamed", "From b"]);

    sign_in_only(&mut c, &http, &server.url);
    unlock(&mut c, "a new passphrase").unwrap();
    sync(&mut c, &http);
    assert_eq!(toolbar_titles(&mut c), ["Renamed", "From b"]);
}

#[test]
fn what_a_malicious_server_replays_moves_or_makes_up_is_refused() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let dirs = [TempDir::new("evil-a"), TempDir::new("evil-b"), TempDir::new("evil-c"), TempDir::new("evil-made-up")];
    let [mut a, mut b, mut c, mut made_up] = dirs.each_ref().map(open);
    let url = Url::parse("https://example.com/").unwrap();
    let x = a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "X", &url).unwrap();
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Y", &url).unwrap();
    made_up.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Made up", &url).unwrap();

    provider.sign_in_as("omar");
    sign_in(&mut a, &http, &server.url);
    sign_in(&mut b, &http, &server.url);
    let before: HashMap<String, Vec<u8>> = server_records(&mut a, &server.url).into_iter().map(|r| (r.id, r.body)).collect();
    a.bookmarks().rename(x, "X2").unwrap();
    sync(&mut a, &http);
    sync(&mut b, &http);
    let after = server_records(&mut a, &server.url);
    let changed: Vec<&Record> = after.iter().filter(|r| r.kind == 201 && before.get(&r.id) != Some(&r.body)).collect();
    assert_eq!(changed.len(), 1, "X's slot");
    let x_slot = changed[0].id.clone();

    // An older ciphertext of X, replayed into its slot: b keeps the newer, and puts it back.
    put_records(&mut a, &server.url, vec![Record { kind: 201, id: x_slot.clone(), body: before[&x_slot].clone() }]);
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["X2", "Y"]);
    sync(&mut b, &http);
    sign_in(&mut c, &http, &server.url);
    assert_eq!(toolbar_titles(&mut c), ["X2", "Y"], "the replay was repaired");

    // Another slot's ciphertext moved into X's: it does not open there.
    let other = after.iter().find(|r| r.kind == 201 && r.id != x_slot).unwrap().body.clone();
    put_records(&mut a, &server.url, vec![Record { kind: 201, id: x_slot.clone(), body: other }]);
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["X2", "Y"]);

    // Plaintext the server made up is never applied.
    put_records(&mut a, &server.url, plaintext_bookmarks(&mut made_up));
    sync(&mut b, &http);
    assert_eq!(toolbar_titles(&mut b), ["X2", "Y"]);

    // An altered key record: a device that syncs puts its own back.
    let key_record = after.iter().find(|r| r.kind == 200).unwrap().clone();
    let mut altered: serde_json::Value = serde_json::from_slice(&key_record.body).unwrap();
    altered["nonce"] = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into();
    put_records(&mut a, &server.url, vec![Record { body: altered.to_string().into_bytes(), ..key_record.clone() }]);
    sync(&mut a, &http);
    let restored = server_records(&mut a, &server.url).into_iter().find(|r| r.kind == 200).unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&restored.body).unwrap(), serde_json::from_slice::<serde_json::Value>(&key_record.body).unwrap());
    assert_eq!(encryption(&mut a), Encryption::Ready);
}

#[test]
fn a_server_restored_from_before_the_passphrase_gets_the_key_record_back() {
    let Ok(bin) = std::env::var("VSESVIT_SYNC_SERVER_BIN") else {
        eprintln!("skipped: VSESVIT_SYNC_SERVER_BIN is not set");
        return;
    };
    let provider = MockProvider::start();
    let mut server = start_server(&bin, &provider.issuer);
    let http = Http::new();
    let (dir_a, dir_b) = (TempDir::new("restore-keys-a"), TempDir::new("restore-keys-b"));
    let (mut a, mut b) = (open(&dir_a), open(&dir_b));
    a.bookmarks().add_url(BookmarkId::TOOLBAR, InsertAt::End, "Kept", &Url::parse("https://example.com/").unwrap()).unwrap();

    provider.sign_in_as("pat");
    sign_in_only(&mut a, &http, &server.url);
    let backup = server.back_up();
    unlock(&mut a, PASSPHRASE).unwrap();
    sync(&mut a, &http);
    server.restore(&backup, &[("EPOCH", "1")]);

    let http = Http::new();
    sync(&mut a, &http);
    assert_eq!(server_records(&mut a, &server.url).iter().filter(|r| r.kind == 200).count(), 1);
    sign_in(&mut b, &http, &server.url);
    assert_eq!(toolbar_titles(&mut b), ["Kept"]);
}
