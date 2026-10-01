//! The routes end to end: a mock OpenID Connect provider on a real port, the real router and a
//! real database. SQLite in memory always; Postgres too when `VSESVIT_SYNC_TEST_POSTGRES` holds
//! the URL of an empty database (the tests drop and recreate its tables, so run them one at a
//! time).
//!
//! A sign-in walks every hop a browser would: the server's authorize, the provider's page, the
//! server's callback, the redirect to the browser's loopback address, and the token exchange.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::BodyExt;
use reqwest::Url;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use serde::de::DeserializeOwned;
use serde_json::json;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use vsesvit_sync_proto::{ApiError, OAuthError, Page, Record, ServerInfo, TokenResponse, Upload, Uploaded};
use vsesvit_sync_server::api::{self, AppState};
use vsesvit_sync_server::config::{Config, DatabaseConfig};
use vsesvit_sync_server::entities::{accounts, logins, sessions};
use vsesvit_sync_server::store;

/// The server's public address in the tests. The provider redirects there, and the test hands those
/// requests to the router itself.
const PUBLIC_URL: &str = "http://127.0.0.1:9";
/// The browser's loopback address. Nothing listens there; the test reads the redirect to it.
const BROWSER: &str = "http://127.0.0.1:50123/callback";
const CLIENT_ID: &str = "vsesvit";
const SECRET: &str = "s3cret";

#[derive(Default)]
struct Mock {
    /// Who the provider's page signs in; `deny` refuses.
    user: String,
    /// code -> (user, PKCE challenge, redirect_uri)
    codes: HashMap<String, (String, String, String)>,
    issued: u32,
}

type Shared = Arc<Mutex<Mock>>;

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

async fn provider() -> (String, Shared) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let discovery = json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "userinfo_endpoint": format!("{issuer}/userinfo"),
    });
    let mock = Shared::default();
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
        .route("/authorize", get(provider_authorize))
        .route("/token", post(provider_token))
        .route("/userinfo", get(provider_userinfo))
        .with_state(mock.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (issuer, mock)
}

async fn provider_authorize(State(mock): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Response {
    assert_eq!(q["client_id"], CLIENT_ID);
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["redirect_uri"], format!("{PUBLIC_URL}/v1/auth/callback"));
    assert!(q["scope"].split(' ').any(|s| s == "openid"));
    let mut m = mock.lock().unwrap();
    let mut back = Url::parse(&q["redirect_uri"]).unwrap();
    if m.user == "deny" {
        back.query_pairs_mut().append_pair("error", "access_denied").append_pair("error_description", "the person said no");
    } else {
        m.issued += 1;
        let code = format!("provider-code-{}", m.issued);
        let user = m.user.clone();
        m.codes.insert(code.clone(), (user, q["code_challenge"].clone(), q["redirect_uri"].clone()));
        back.query_pairs_mut().append_pair("code", &code);
    }
    back.query_pairs_mut().append_pair("state", &q["state"]);
    Redirect::to(back.as_str()).into_response()
}

async fn provider_token(State(mock): State<Shared>, Form(f): Form<HashMap<String, String>>) -> Response {
    let refuse = || (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant" }))).into_response();
    if f.get("client_id").map(String::as_str) != Some(CLIENT_ID) || f.get("client_secret").map(String::as_str) != Some(SECRET) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "invalid_client" }))).into_response();
    }
    let Some((user, challenge_sent, redirect)) = mock.lock().unwrap().codes.remove(&f["code"]) else { return refuse() };
    if challenge(&f["code_verifier"]) != challenge_sent || f["redirect_uri"] != redirect {
        return refuse();
    }
    Json(json!({ "access_token": format!("at-{user}"), "token_type": "Bearer", "expires_in": 60 })).into_response()
}

async fn provider_userinfo(headers: HeaderMap) -> Response {
    let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or_default();
    match token.strip_prefix("Bearer at-") {
        Some(user) => Json(json!({ "sub": format!("{user}-id"), "name": user })).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

struct TestApp {
    router: Router,
    db: DatabaseConnection,
    mock: Shared,
    issuer: String,
}

fn database(url: &str, run_migrations: bool) -> DatabaseConfig {
    DatabaseConfig { url: url.to_owned(), run_migrations, max_connections: 4 }
}

async fn app(database_url: &str, settings: &[(&str, &str)]) -> TestApp {
    let (issuer, mock) = provider().await;
    let config = Config::from_env(|name| match name {
        "OIDC_ISSUER" => Some(issuer.clone()),
        "OIDC_CLIENT_ID" => Some(CLIENT_ID.to_owned()),
        "OIDC_CLIENT_SECRET" => Some(SECRET.to_owned()),
        "PUBLIC_URL" => Some(PUBLIC_URL.to_owned()),
        "MAX_BATCH" => Some("3".to_owned()),
        "MAX_RECORD_BYTES" => Some("16".to_owned()),
        _ => settings.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned()),
    })
    .unwrap();
    let db = vsesvit_sync_server::connect(&database(database_url, true)).await.unwrap();
    TestApp { router: api::router(AppState::new(db.clone(), &config)), db, mock, issuer }
}

async fn each_database(test: impl AsyncFn(TestApp)) {
    each_database_with(&[], test).await;
}

/// Runs `test` on SQLite in memory, then on Postgres when it is configured, from empty tables.
async fn each_database_with(settings: &[(&str, &str)], test: impl AsyncFn(TestApp)) {
    test(app("sqlite::memory:", settings).await).await;
    if let Ok(url) = std::env::var("VSESVIT_SYNC_TEST_POSTGRES") {
        let db = sea_orm::Database::connect(&url).await.unwrap();
        use sea_orm::ConnectionTrait;
        db.execute_unprepared("DROP TABLE IF EXISTS records, sessions, logins, accounts, seaql_migrations").await.unwrap();
        test(app(&url, settings).await).await;
    }
}

/// One request through the router: status, the redirect it names, and the body.
async fn send(app: &TestApp, request: Request<Body>) -> (StatusCode, Option<String>, Vec<u8>) {
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let location = response.headers().get(header::LOCATION).map(|l| l.to_str().unwrap().to_owned());
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, location, body)
}

async fn call<T: DeserializeOwned>(app: &TestApp, method: &str, uri: &str, token: Option<&str>, body: Option<serde_json::Value>) -> (StatusCode, Option<T>) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let (status, _, bytes) = send(app, request).await;
    (status, serde_json::from_slice(&bytes).ok())
}

fn encode(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish()
}

/// Walks a sign-in as `user` up to the browser's loopback address, and returns where the server sent
/// the browser.
async fn authorize(app: &TestApp, user: &str, verifier: &str) -> Url {
    let query = encode(&[
        ("response_type", "code"),
        ("redirect_uri", BROWSER),
        ("state", "browser-state"),
        ("code_challenge", &challenge(verifier)),
        ("code_challenge_method", "S256"),
    ]);
    let request = Request::get(format!("/v1/auth/authorize?{query}")).body(Body::empty()).unwrap();
    let (status, to_provider, _) = send(app, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    app.mock.lock().unwrap().user = user.to_owned();
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let answer = http.get(to_provider.unwrap()).send().await.unwrap();
    let back = answer.headers()[header::LOCATION].to_str().unwrap().to_owned();
    let callback = back.strip_prefix(PUBLIC_URL).expect("the provider sends the person back to the server").to_owned();
    let (status, to_browser, _) = send(app, Request::get(callback).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let to_browser = Url::parse(&to_browser.unwrap()).unwrap();
    assert!(to_browser.as_str().starts_with(BROWSER));
    to_browser
}

fn param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
}

async fn exchange(app: &TestApp, code: &str, verifier: &str) -> (StatusCode, Vec<u8>) {
    let form = encode(&[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", BROWSER), ("code_verifier", verifier)]);
    let request = Request::post("/v1/auth/token")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap();
    let (status, _, body) = send(app, request).await;
    (status, body)
}

/// A session for `user`, as the browser gets one.
async fn sign_in(app: &TestApp, user: &str) -> String {
    let verifier = format!("{user}-verifier-0123456789-0123456789-0123456789");
    let back = authorize(app, user, &verifier).await;
    let (status, body) = exchange(app, &param(&back, "code").unwrap(), &verifier).await;
    assert_eq!(status, StatusCode::OK);
    let session: TokenResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!((session.token_type.as_str(), session.name.as_deref()), ("Bearer", Some(user)));
    session.access_token
}

fn record(kind: u8, id: &str, body: &str) -> Record {
    Record { kind, id: id.to_owned(), body: body.as_bytes().to_vec() }
}

async fn upload(app: &TestApp, token: &str, records: Vec<Record>) -> StatusCode {
    let body = serde_json::to_value(Upload { records, download_cursor: 0 }).unwrap();
    call::<Uploaded>(app, "POST", "/v1/records", Some(token), Some(body)).await.0
}

async fn download(app: &TestApp, token: &str, since: u64) -> Page {
    let (status, page) = call::<Page>(app, "GET", &format!("/v1/records?since={since}"), Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    page.unwrap()
}

fn bodies(page: &Page) -> Vec<(u8, &str, &str)> {
    page.records.iter().map(|r| (r.kind, r.id.as_str(), std::str::from_utf8(&r.body).unwrap())).collect()
}

#[tokio::test]
async fn info_is_public_and_names_no_provider() {
    each_database(async |app| {
        let (status, info) = call::<serde_json::Value>(&app, "GET", "/v1/info", None, None).await;
        assert_eq!(status, StatusCode::OK);
        let info = info.unwrap();
        assert_eq!(info, json!({ "protocol": 2, "limits": { "max_batch": 3, "max_record_bytes": 16, "max_request_bytes": 33554432 } }));
        let _: ServerInfo = serde_json::from_value(info).unwrap();
    })
    .await;
}

#[tokio::test]
async fn a_sign_in_through_the_provider_gives_the_browser_a_session_for_its_records() {
    each_database(async |app| {
        let (status, _) = call::<Page>(&app, "GET", "/v1/records", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call::<Page>(&app, "GET", "/v1/records", Some("made-up"), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let verifier = "alice-verifier-0123456789-0123456789-0123456789";
        let back = authorize(&app, "alice", verifier).await;
        assert_eq!(param(&back, "state").as_deref(), Some("browser-state"));
        assert!(param(&back, "error").is_none());
        let (status, body) = exchange(&app, &param(&back, "code").unwrap(), verifier).await;
        assert_eq!(status, StatusCode::OK);
        let session: TokenResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(upload(&app, &session.access_token, vec![record(1, "a", "x")]).await, StatusCode::OK);
        assert_eq!(bodies(&download(&app, &session.access_token, 0).await), [(1, "a", "x")]);
        assert!(app.mock.lock().unwrap().codes.is_empty(), "the server traded the provider's code");
    })
    .await;
}

#[tokio::test]
async fn a_code_works_once_and_only_with_its_verifier_and_redirect() {
    each_database(async |app| {
        let verifier = "bob-verifier-0123456789-0123456789-0123456789-0";
        let back = authorize(&app, "bob", verifier).await;
        let code = param(&back, "code").unwrap();
        let (status, body) = exchange(&app, &code, "another-verifier-0123456789-0123456789-0123456").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(serde_json::from_slice::<OAuthError>(&body).unwrap().error, "invalid_grant");
        let (status, _) = exchange(&app, &code, verifier).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "a code tried once is gone, right verifier or not");

        let back = authorize(&app, "bob", verifier).await;
        let code = param(&back, "code").unwrap();
        assert_eq!(exchange(&app, &code, verifier).await.0, StatusCode::OK);
        assert_eq!(exchange(&app, &code, verifier).await.0, StatusCode::BAD_REQUEST, "and a used one too");

        let form = encode(&[("grant_type", "refresh_token"), ("refresh_token", "x")]);
        let request = Request::post("/v1/auth/token").header(header::CONTENT_TYPE, "application/x-www-form-urlencoded").body(Body::from(form)).unwrap();
        let (status, _, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(serde_json::from_slice::<OAuthError>(&body).unwrap().error, "unsupported_grant_type");
    })
    .await;
}

#[tokio::test]
async fn a_refusal_at_the_provider_reaches_the_browser_with_its_state() {
    each_database(async |app| {
        let back = authorize(&app, "deny", "deny-verifier-0123456789-0123456789-0123456789").await;
        assert_eq!(param(&back, "error").as_deref(), Some("access_denied"));
        assert_eq!(param(&back, "error_description").as_deref(), Some("the person said no"));
        assert_eq!(param(&back, "state").as_deref(), Some("browser-state"));
        assert!(param(&back, "code").is_none());
    })
    .await;
}

#[tokio::test]
async fn only_the_allowed_subjects_get_an_account() {
    each_database_with(&[("ALLOWED_SUBJECTS", "carol-id, alice-id")], async |app| {
        let back = authorize(&app, "mallory", "mallory-verifier-0123456789-0123456789-0123456789").await;
        assert_eq!(param(&back, "error").as_deref(), Some("access_denied"));
        assert_eq!(param(&back, "state").as_deref(), Some("browser-state"));
        assert!(param(&back, "code").is_none());
        assert_eq!(accounts::Entity::find().count(&app.db).await.unwrap(), 0);
        sign_in(&app, "alice").await;
    })
    .await;
}

#[tokio::test]
async fn a_server_with_all_the_accounts_it_takes_still_lets_their_people_in() {
    each_database_with(&[("MAX_ACCOUNTS", "1")], async |app| {
        sign_in(&app, "alice").await;
        let back = authorize(&app, "bob", "bob-verifier-0123456789-0123456789-0123456789-0").await;
        assert_eq!(param(&back, "error").as_deref(), Some("access_denied"));
        assert!(param(&back, "code").is_none());
        assert_eq!(accounts::Entity::find().count(&app.db).await.unwrap(), 1);
        sign_in(&app, "alice").await;
    })
    .await;
}

#[tokio::test]
async fn a_request_the_server_cannot_trust_gets_a_page_not_a_redirect() {
    each_database(async |app| {
        let query = encode(&[
            ("response_type", "code"),
            ("redirect_uri", "https://evil.example/collect"),
            ("state", "s"),
            ("code_challenge", &challenge("v")),
            ("code_challenge_method", "S256"),
        ]);
        let (status, location, _) = send(&app, Request::get(format!("/v1/auth/authorize?{query}")).body(Body::empty()).unwrap()).await;
        assert_eq!((status, location), (StatusCode::BAD_REQUEST, None));
        let (status, location, _) =
            send(&app, Request::get("/v1/auth/callback?code=x&state=unknown").body(Body::empty()).unwrap()).await;
        assert_eq!((status, location), (StatusCode::BAD_REQUEST, None));
    })
    .await;
}

#[tokio::test]
async fn a_sign_in_nobody_finishes_leaves_nothing_on_the_server() {
    each_database(async |app| {
        let query = encode(&[
            ("response_type", "code"),
            ("redirect_uri", "http://127.0.0.1:1/"),
            ("state", "x"),
            ("code_challenge", &challenge("v")),
            ("code_challenge_method", "S256"),
        ]);
        let mut to_provider = None;
        for _ in 0..20 {
            let (status, location, _) = send(&app, Request::get(format!("/v1/auth/authorize?{query}")).body(Body::empty()).unwrap()).await;
            assert_eq!(status, StatusCode::SEE_OTHER);
            to_provider = location;
        }
        assert_eq!(logins::Entity::find().count(&app.db).await.unwrap(), 0, "anyone may start a sign-in, so it costs no row");

        // The provider sends back the state it was given, which only this server could have made.
        let to_provider = Url::parse(&to_provider.unwrap()).unwrap();
        let state = param(&to_provider, "state").unwrap();
        let mut forged = state.into_bytes();
        forged[0] = if forged[0] == b'A' { b'B' } else { b'A' };
        let callback = format!("/v1/auth/callback?{}", encode(&[("code", "c"), ("state", std::str::from_utf8(&forged).unwrap())]));
        let (status, location, _) = send(&app, Request::get(callback).body(Body::empty()).unwrap()).await;
        assert_eq!((status, location), (StatusCode::BAD_REQUEST, None));
    })
    .await;
}

#[tokio::test]
async fn signing_out_ends_only_that_session() {
    each_database(async |app| {
        let (laptop, phone) = (sign_in(&app, "alice").await, sign_in(&app, "alice").await);
        upload(&app, &laptop, vec![record(1, "a", "x")]).await;
        let (status, _) = call::<()>(&app, "DELETE", "/v1/auth/session", Some(&laptop), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call::<Page>(&app, "GET", "/v1/records", Some(&laptop), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(bodies(&download(&app, &phone, 0).await), [(1, "a", "x")], "the same account, another session");
    })
    .await;
}

#[tokio::test]
async fn a_session_unused_for_too_long_ends() {
    each_database_with(&[("SESSION_IDLE_DAYS", "1")], async |app| {
        let session = sign_in(&app, "alice").await;
        assert_eq!(download(&app, &session, 0).await.records, []);
        let two_days_ago = chrono::Utc::now() - chrono::Duration::days(2);
        sessions::Entity::update_many()
            .col_expr(sessions::Column::LastUsedAt, sea_orm::sea_query::Expr::value(two_days_ago))
            .filter(sessions::Column::TokenHash.eq(store::token_hash(&session)))
            .exec(&app.db)
            .await
            .unwrap();
        let (status, _) = call::<Page>(&app, "GET", "/v1/records", Some(&session), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(sessions::Entity::find_by_id(store::token_hash(&session)).one(&app.db).await.unwrap().is_none(), "and is deleted");
    })
    .await;
}

#[tokio::test]
async fn signing_in_clears_sessions_left_unused_too_long() {
    each_database_with(&[("SESSION_IDLE_DAYS", "1")], async |app| {
        let (abandoned, kept) = (sign_in(&app, "alice").await, sign_in(&app, "carol").await);
        let two_days_ago = chrono::Utc::now() - chrono::Duration::days(2);
        sessions::Entity::update_many()
            .col_expr(sessions::Column::LastUsedAt, sea_orm::sea_query::Expr::value(two_days_ago))
            .filter(sessions::Column::TokenHash.eq(store::token_hash(&abandoned)))
            .exec(&app.db)
            .await
            .unwrap();
        let bob = sign_in(&app, "bob").await;
        assert!(sessions::Entity::find_by_id(store::token_hash(&abandoned)).one(&app.db).await.unwrap().is_none());
        assert_eq!(download(&app, &kept, 0).await.records, []);
        assert_eq!(download(&app, &bob, 0).await.records, []);
    })
    .await;
}

#[tokio::test]
async fn a_download_pages_through_writes_in_order_and_sees_each_rewrite_once() {
    each_database(async |app| {
        let alice = sign_in(&app, "alice").await;
        assert_eq!(upload(&app, &alice, vec![record(1, "a", "a1"), record(2, "b", "b1"), record(1, "a", "a2")]).await, StatusCode::OK);
        assert_eq!(upload(&app, &alice, vec![record(7, "c", "c1"), record(8, "d", "d1")]).await, StatusCode::OK);

        let first = download(&app, &alice, 0).await;
        assert_eq!(bodies(&first), [(2, "b", "b1"), (1, "a", "a2"), (7, "c", "c1")]);
        assert!(first.more);
        let second = download(&app, &alice, first.cursor).await;
        assert_eq!(bodies(&second), [(8, "d", "d1")]);
        assert!(!second.more);

        assert_eq!(upload(&app, &alice, vec![record(2, "b", "b2")]).await, StatusCode::OK);
        let third = download(&app, &alice, second.cursor).await;
        assert_eq!(bodies(&third), [(2, "b", "b2")]);
        assert_eq!(download(&app, &alice, third.cursor).await.records, []);
        assert_eq!(bodies(&download(&app, &alice, 0).await), [(1, "a", "a2"), (7, "c", "c1"), (8, "d", "d1")]);
    })
    .await;
}

#[tokio::test]
async fn a_cursor_past_every_write_of_the_account_is_refused_and_starts_a_new_epoch() {
    each_database(async |app| {
        let alice = sign_in(&app, "alice").await;
        upload(&app, &alice, vec![record(1, "a", "x"), record(1, "b", "x")]).await;
        let page = download(&app, &alice, 0).await;
        assert_eq!(download(&app, &alice, page.cursor).await.records, []);
        // As a device holds it after the server was restored from an older backup.
        let ahead = page.cursor + 1;
        let (status, error) = call::<ApiError>(&app, "GET", &format!("/v1/records?since={ahead}"), Some(&alice), None).await;
        assert_eq!(status, StatusCode::CONFLICT, "{error:?}");
        let after = download(&app, &alice, 0).await;
        assert_ne!(after.epoch, page.epoch, "every device learns of it, not only the one that was ahead");

        // An upload is refused before it stores anything, so its own writes cannot hide the gap.
        let body = json!({ "records": [record(1, "c", "x"), record(1, "d", "x")], "download_cursor": ahead });
        let (status, error) = call::<ApiError>(&app, "POST", "/v1/records", Some(&alice), Some(body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{error:?}");
        assert_eq!(download(&app, &alice, 0).await.records.len(), 2);
        let body = json!({ "records": [record(1, "c", "x")], "download_cursor": page.cursor });
        assert_eq!(call::<Uploaded>(&app, "POST", "/v1/records", Some(&alice), Some(body)).await.0, StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn every_page_carries_the_epoch_the_operator_set() {
    each_database_with(&[("EPOCH", "3")], async |app| {
        let alice = sign_in(&app, "alice").await;
        assert_eq!(download(&app, &alice, 0).await.epoch, 3);
    })
    .await;
}

#[tokio::test]
async fn accounts_do_not_see_each_other() {
    each_database(async |app| {
        let (alice, bob) = (sign_in(&app, "alice").await, sign_in(&app, "bob").await);
        upload(&app, &alice, vec![record(1, "a", "alice's")]).await;
        upload(&app, &bob, vec![record(1, "a", "bob's")]).await;
        assert_eq!(bodies(&download(&app, &alice, 0).await), [(1, "a", "alice's")]);
        assert_eq!(bodies(&download(&app, &bob, 0).await), [(1, "a", "bob's")]);
    })
    .await;
}

#[tokio::test]
async fn uploads_over_the_limits_are_refused_whole() {
    each_database(async |app| {
        let alice = sign_in(&app, "alice").await;
        let four = (0..4).map(|i| record(1, &i.to_string(), "x")).collect();
        assert_eq!(upload(&app, &alice, four).await, StatusCode::PAYLOAD_TOO_LARGE);
        let big = vec![record(1, "ok", "x"), record(1, "big", "seventeen bytes!!")];
        assert_eq!(upload(&app, &alice, big).await, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(upload(&app, &alice, vec![record(1, "", "x")]).await, StatusCode::BAD_REQUEST);
        assert_eq!(download(&app, &alice, 0).await.records, []);
    })
    .await;
}

#[tokio::test]
async fn deleting_the_records_signs_out_every_device_of_the_account() {
    each_database(async |app| {
        let (laptop, phone, bob) = (sign_in(&app, "alice").await, sign_in(&app, "alice").await, sign_in(&app, "bob").await);
        upload(&app, &phone, vec![record(1, "a", "x")]).await;
        let (status, _) = call::<()>(&app, "DELETE", "/v1/account", Some(&laptop), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        for session in [&laptop, &phone] {
            let (status, _) = call::<Page>(&app, "GET", "/v1/records", Some(session), None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "it signs in again, and uploads all it holds");
        }
        assert_eq!(download(&app, &bob, 0).await.records, [], "other accounts stay signed in");
    })
    .await;
}

#[tokio::test]
async fn deleting_the_records_keeps_the_accounts_sequence_counting() {
    each_database(async |app| {
        let (alice, bob) = (sign_in(&app, "alice").await, sign_in(&app, "bob").await);
        upload(&app, &alice, vec![record(1, "a", "x"), record(1, "b", "x"), record(1, "c", "x")]).await;
        upload(&app, &bob, vec![record(1, "a", "y")]).await;
        let held = download(&app, &alice, 0).await.cursor;
        let (status, _) = call::<()>(&app, "DELETE", "/v1/account", Some(&alice), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let alice = sign_in(&app, "alice").await;
        assert_eq!(download(&app, &alice, 0).await.records, []);
        assert_eq!(bodies(&download(&app, &bob, 0).await), [(1, "a", "y")]);
        upload(&app, &alice, vec![record(1, "a", "again")]).await;
        assert_eq!(bodies(&download(&app, &alice, held).await), [(1, "a", "again")]);
    })
    .await;
}

#[tokio::test]
async fn an_account_stores_up_to_its_quota_and_rewrites_that_do_not_grow_it_always_pass() {
    let quota = [("MAX_ACCOUNT_RECORDS", "3"), ("MAX_ACCOUNT_BYTES", "40")];
    each_database_with(&quota, async |app| {
        let (alice, bob) = (sign_in(&app, "alice").await, sign_in(&app, "bob").await);
        let five = "12345";
        let sixteen = "sixteen bytes!!!";
        let three = vec![record(1, "a", five), record(1, "b", five), record(1, "c", five)];
        assert_eq!(upload(&app, &alice, three).await, StatusCode::OK);
        assert_eq!(upload(&app, &alice, vec![record(1, "d", five)]).await, StatusCode::INSUFFICIENT_STORAGE, "a fourth record");
        assert_eq!(upload(&app, &alice, vec![record(1, "a", "54321")]).await, StatusCode::OK);
        let two = vec![record(1, "a", sixteen), record(1, "b", sixteen)];
        assert_eq!(upload(&app, &alice, two.clone()).await, StatusCode::OK, "40 bytes");
        assert_eq!(upload(&app, &alice, vec![record(1, "c", sixteen)]).await, StatusCode::INSUFFICIENT_STORAGE, "51 bytes");
        assert_eq!(upload(&app, &bob, vec![record(1, "c", sixteen)]).await, StatusCode::OK, "quotas are per account");
        call::<()>(&app, "DELETE", "/v1/account", Some(&alice), None).await;
        let alice = sign_in(&app, "alice").await;
        assert_eq!(upload(&app, &alice, two).await, StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn a_download_page_stays_within_its_byte_budget_but_always_moves() {
    each_database(async |app| {
        let alice = sign_in(&app, "alice").await;
        let sixteen = "sixteen bytes!!!";
        upload(&app, &alice, vec![record(1, "a", sixteen), record(1, "b", sixteen), record(1, "c", sixteen)]).await;
        let account = store::account(&app.db, &app.issuer, "alice-id", None).await.unwrap().unwrap();
        let page = store::download(&app.db, account, 0, 10, 200).await.unwrap();
        assert_eq!((page.records.len(), page.more), (2, true));
        let page = store::download(&app.db, account, page.cursor, 10, 1).await.unwrap();
        assert_eq!((page.records.len(), page.more), (1, false), "one record over the budget still goes");
    })
    .await;
}

#[tokio::test]
async fn record_ids_longer_than_an_index_entry_are_stored() {
    each_database(async |app| {
        let alice = sign_in(&app, "alice").await;
        let mut id = String::new();
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        while id.len() < 8000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            id.push_str(&format!("{state:016x}"));
        }
        id.truncate(8000);
        assert_eq!(upload(&app, &alice, vec![record(2, &id, "x"), record(2, &format!("{id}!"), "y")]).await, StatusCode::OK);
        let page = download(&app, &alice, 0).await;
        assert_eq!(page.records.iter().map(|r| r.id.len()).collect::<Vec<_>>(), [8000, 8001]);
    })
    .await;
}

#[tokio::test]
async fn pending_migrations_are_applied_or_refused_as_configured() {
    let dir = std::env::temp_dir().join(format!("vsesvit-sync-migrations-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite://{}?mode=rwc", dir.join("sync.db").display().to_string().replace('\\', "/"));
    let refused = vsesvit_sync_server::connect(&database(&url, false)).await;
    assert!(matches!(refused, Err(vsesvit_sync_server::ConnectError::Pending(names)) if names.contains("create_sessions_and_logins")));
    vsesvit_sync_server::connect(&database(&url, true)).await.unwrap();
    vsesvit_sync_server::connect(&database(&url, false)).await.expect("nothing is pending once applied");
    let _ = std::fs::remove_dir_all(&dir);
}
