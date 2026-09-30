//! The routes end to end: a mock OpenID Connect provider on a real port, the real router and a
//! real database. SQLite in memory always; Postgres too when `VSESVIT_SYNC_TEST_POSTGRES` holds
//! the URL of an empty database.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use serde_json::json;
use sea_orm::DatabaseConnection;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use tower::ServiceExt;
use vsesvit_sync_proto::{Page, Record, ServerInfo, Upload, Uploaded};
use vsesvit_sync_server::api::{self, AppState};
use vsesvit_sync_server::config::{Config, DatabaseConfig};
use vsesvit_sync_server::store;

/// A JWT access token, unsigned: the mock provider trusts it, and the server reads only its claims.
fn jwt(claims: serde_json::Value) -> String {
    format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()))
}

fn token_of(user: &str) -> String {
    jwt(json!({ "client_id": "vsesvit", "user": user }))
}

static ALICE: LazyLock<String> = LazyLock::new(|| token_of("alice"));
static BOB: LazyLock<String> = LazyLock::new(|| token_of("bob"));

/// Serves discovery, and a userinfo that knows the `user` claim `alice` or `bob` and counts its
/// calls.
async fn provider() -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let discovery = json!({ "issuer": issuer, "userinfo_endpoint": format!("{issuer}/userinfo") });
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
        .route(
            "/userinfo",
            get(move |headers: axum::http::HeaderMap| async move {
                counted.fetch_add(1, Ordering::SeqCst);
                let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or_default();
                let claims: Option<serde_json::Value> = token
                    .strip_prefix("Bearer ")
                    .and_then(|t| t.split('.').nth(1))
                    .and_then(|p| URL_SAFE_NO_PAD.decode(p).ok())
                    .and_then(|p| serde_json::from_slice(&p).ok());
                match claims.as_ref().and_then(|c| c["user"].as_str()) {
                    Some(user @ ("alice" | "bob")) => Ok(Json(json!({ "sub": format!("{user}-id") }))),
                    _ => Err(StatusCode::UNAUTHORIZED),
                }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (issuer, calls)
}

fn database(url: &str, run_migrations: bool) -> DatabaseConfig {
    DatabaseConfig { url: url.to_owned(), run_migrations, max_connections: 4 }
}

struct TestApp {
    router: Router,
    db: DatabaseConnection,
    provider_calls: Arc<AtomicUsize>,
}

impl std::ops::Deref for TestApp {
    type Target = Router;
    fn deref(&self) -> &Router {
        &self.router
    }
}

async fn app(database_url: &str, settings: &[(&str, &str)]) -> TestApp {
    let (issuer, provider_calls) = provider().await;
    let config = Config::from_env(|name| match name {
        "OIDC_ISSUER" => Some(issuer.clone()),
        "OIDC_CLIENT_ID" => Some("vsesvit".to_owned()),
        "MAX_BATCH" => Some("3".to_owned()),
        "MAX_RECORD_BYTES" => Some("16".to_owned()),
        _ => settings.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned()),
    })
    .unwrap();
    let db = vsesvit_sync_server::connect(&database(database_url, true)).await.unwrap();
    TestApp { router: api::router(AppState::new(db.clone(), &config)), db, provider_calls }
}

async fn call<T: DeserializeOwned>(app: &Router, method: &str, uri: &str, token: Option<&str>, body: Option<serde_json::Value>) -> (StatusCode, Option<T>) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).ok())
}

fn record(kind: u8, id: &str, body: &str) -> Record {
    Record { kind, id: id.to_owned(), body: body.as_bytes().to_vec() }
}

async fn upload(app: &Router, token: &str, records: Vec<Record>) -> StatusCode {
    let body = serde_json::to_value(Upload { records }).unwrap();
    call::<Uploaded>(app, "POST", "/v1/records", Some(token), Some(body)).await.0
}

async fn download(app: &Router, token: &str, since: u64) -> Page {
    let (status, page) = call::<Page>(app, "GET", &format!("/v1/records?since={since}"), Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    page.unwrap()
}

fn bodies(page: &Page) -> Vec<(u8, &str, &str)> {
    page.records.iter().map(|r| (r.kind, r.id.as_str(), std::str::from_utf8(&r.body).unwrap())).collect()
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
        db.execute_unprepared("DROP TABLE IF EXISTS records, accounts, seaql_migrations").await.unwrap();
        test(app(&url, settings).await).await;
    }
}

#[tokio::test]
async fn info_is_public_and_names_the_provider() {
    each_database(async |app| {
        let (status, info) = call::<ServerInfo>(&app, "GET", "/v1/info", None, None).await;
        assert_eq!(status, StatusCode::OK);
        let info = info.unwrap();
        assert_eq!(info.protocol, 1);
        assert_eq!(info.auth.client_id, "vsesvit");
        assert_eq!(info.limits.max_batch, 3);
    })
    .await;
}

#[tokio::test]
async fn records_need_a_token_the_provider_accepts_issued_to_this_app() {
    each_database(async |app| {
        let status = async |token: Option<String>| call::<Page>(&app, "GET", "/v1/records", token.as_deref(), None).await.0;
        assert_eq!(status(None).await, StatusCode::UNAUTHORIZED);
        assert_eq!(status(Some(token_of("mallory"))).await, StatusCode::UNAUTHORIZED);
        assert_eq!(status(Some(token_of("mallory"))).await, StatusCode::UNAUTHORIZED);
        assert_eq!(app.provider_calls.load(Ordering::SeqCst), 1, "a refused token is not asked about again at once");

        let other_client = jwt(json!({ "client_id": "another-app", "user": "alice", "exp": 4102444800.5 }));
        assert_eq!(status(Some(other_client)).await, StatusCode::FORBIDDEN);
        let both = jwt(json!({ "client_id": "vsesvit", "azp": "another-app", "user": "alice" }));
        assert_eq!(status(Some(both)).await, StatusCode::FORBIDDEN);
        assert_eq!(status(Some("opaque-alice".to_owned())).await, StatusCode::FORBIDDEN);
        let expired = jwt(json!({ "client_id": "vsesvit", "user": "alice", "exp": 1000 }));
        assert_eq!(status(Some(expired)).await, StatusCode::UNAUTHORIZED);
        assert_eq!(app.provider_calls.load(Ordering::SeqCst), 1, "tokens refused here never reach the provider");

        assert_eq!(status(Some(ALICE.clone())).await, StatusCode::OK);
        assert_eq!(status(Some(ALICE.clone())).await, StatusCode::OK);
        assert_eq!(app.provider_calls.load(Ordering::SeqCst), 2, "an accepted token is cached");
    })
    .await;
}

#[tokio::test]
async fn a_download_pages_through_writes_in_order_and_sees_each_rewrite_once() {
    each_database(async |app| {
        assert_eq!(upload(&app, &ALICE, vec![record(1, "a", "a1"), record(2, "b", "b1"), record(1, "a", "a2")]).await, StatusCode::OK);
        assert_eq!(upload(&app, &ALICE, vec![record(7, "c", "c1"), record(8, "d", "d1")]).await, StatusCode::OK);

        let first = download(&app, &ALICE, 0).await;
        assert_eq!(bodies(&first), [(2, "b", "b1"), (1, "a", "a2"), (7, "c", "c1")]);
        assert!(first.more);
        let second = download(&app, &ALICE, first.cursor).await;
        assert_eq!(bodies(&second), [(8, "d", "d1")]);
        assert!(!second.more);

        assert_eq!(upload(&app, &ALICE, vec![record(2, "b", "b2")]).await, StatusCode::OK);
        let third = download(&app, &ALICE, second.cursor).await;
        assert_eq!(bodies(&third), [(2, "b", "b2")]);
        assert_eq!(download(&app, &ALICE, third.cursor).await.records, []);
        assert_eq!(bodies(&download(&app, &ALICE, 0).await), [(1, "a", "a2"), (7, "c", "c1"), (8, "d", "d1")]);
    })
    .await;
}

#[tokio::test]
async fn accounts_do_not_see_each_other() {
    each_database(async |app| {
        upload(&app, &ALICE, vec![record(1, "a", "alice's")]).await;
        upload(&app, &BOB, vec![record(1, "a", "bob's")]).await;
        assert_eq!(bodies(&download(&app, &ALICE, 0).await), [(1, "a", "alice's")]);
        assert_eq!(bodies(&download(&app, &BOB, 0).await), [(1, "a", "bob's")]);
    })
    .await;
}

#[tokio::test]
async fn uploads_over_the_limits_are_refused_whole() {
    each_database(async |app| {
        let four = (0..4).map(|i| record(1, &i.to_string(), "x")).collect();
        assert_eq!(upload(&app, &ALICE, four).await, StatusCode::PAYLOAD_TOO_LARGE);
        let big = vec![record(1, "ok", "x"), record(1, "big", "seventeen bytes!!")];
        assert_eq!(upload(&app, &ALICE, big).await, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(upload(&app, &ALICE, vec![record(1, "", "x")]).await, StatusCode::BAD_REQUEST);
        assert_eq!(download(&app, &ALICE, 0).await.records, []);
    })
    .await;
}

#[tokio::test]
async fn deleting_the_records_keeps_the_cursors_other_devices_hold() {
    each_database(async |app| {
        upload(&app, &ALICE, vec![record(1, "a", "x"), record(1, "b", "x"), record(1, "c", "x")]).await;
        upload(&app, &BOB, vec![record(1, "a", "y")]).await;
        let held = download(&app, &ALICE, 0).await.cursor;
        let (status, _) = call::<()>(&app, "DELETE", "/v1/account", Some(&ALICE), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(download(&app, &ALICE, 0).await.records, []);
        assert_eq!(bodies(&download(&app, &BOB, 0).await), [(1, "a", "y")]);
        upload(&app, &ALICE, vec![record(1, "a", "again")]).await;
        assert_eq!(bodies(&download(&app, &ALICE, held).await), [(1, "a", "again")]);
    })
    .await;
}

#[tokio::test]
async fn an_account_stores_up_to_its_quota_and_rewrites_that_do_not_grow_it_always_pass() {
    let quota = [("MAX_ACCOUNT_RECORDS", "3"), ("MAX_ACCOUNT_BYTES", "40")];
    each_database_with(&quota, async |app| {
        let five = "12345";
        let sixteen = "sixteen bytes!!!";
        let three = vec![record(1, "a", five), record(1, "b", five), record(1, "c", five)];
        assert_eq!(upload(&app, &ALICE, three).await, StatusCode::OK);
        assert_eq!(upload(&app, &ALICE, vec![record(1, "d", five)]).await, StatusCode::INSUFFICIENT_STORAGE, "a fourth record");
        assert_eq!(upload(&app, &ALICE, vec![record(1, "a", "54321")]).await, StatusCode::OK);
        let two = vec![record(1, "a", sixteen), record(1, "b", sixteen)];
        assert_eq!(upload(&app, &ALICE, two.clone()).await, StatusCode::OK, "40 bytes");
        assert_eq!(upload(&app, &ALICE, vec![record(1, "c", sixteen)]).await, StatusCode::INSUFFICIENT_STORAGE, "51 bytes");
        assert_eq!(upload(&app, &BOB, vec![record(1, "c", sixteen)]).await, StatusCode::OK, "quotas are per account");
        call::<()>(&app, "DELETE", "/v1/account", Some(&ALICE), None).await;
        assert_eq!(upload(&app, &ALICE, two).await, StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn a_download_page_stays_within_its_byte_budget_but_always_moves() {
    each_database(async |app| {
        let sixteen = "sixteen bytes!!!";
        upload(&app, &ALICE, vec![record(1, "a", sixteen), record(1, "b", sixteen), record(1, "c", sixteen)]).await;
        let issuer = call::<ServerInfo>(&app, "GET", "/v1/info", None, None).await.1.unwrap().auth.issuer;
        let alice = store::account(&app.db, &issuer, "alice-id").await.unwrap();
        let page = store::download(&app.db, alice, 0, 10, 200).await.unwrap();
        assert_eq!((page.records.len(), page.more), (2, true));
        let page = store::download(&app.db, alice, page.cursor, 10, 1).await.unwrap();
        assert_eq!((page.records.len(), page.more), (1, false), "one record over the budget still goes");
    })
    .await;
}

#[tokio::test]
async fn record_ids_longer_than_an_index_entry_are_stored() {
    each_database(async |app| {
        let mut id = String::new();
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        while id.len() < 8000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            id.push_str(&format!("{state:016x}"));
        }
        id.truncate(8000);
        assert_eq!(upload(&app, &ALICE, vec![record(2, &id, "x"), record(2, &format!("{id}!"), "y")]).await, StatusCode::OK);
        let page = download(&app, &ALICE, 0).await;
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
    assert!(matches!(refused, Err(vsesvit_sync_server::ConnectError::Pending(names)) if names.contains("create_accounts_and_records")));
    vsesvit_sync_server::connect(&database(&url, true)).await.unwrap();
    vsesvit_sync_server::connect(&database(&url, false)).await.expect("nothing is pending once applied");
    let _ = std::fs::remove_dir_all(&dir);
}
