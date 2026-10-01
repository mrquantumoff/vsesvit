//! The HTTP routes of `vsesvit_sync_proto`: signing in (`/v1/auth/*`) and the records.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Form, FromRef, FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use reqwest::Url;
use sea_orm::{DatabaseConnection, DbErr};
use serde::Deserialize;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use vsesvit_sync_proto::{
    ACCOUNT_PATH, AUTHORIZE_PATH, ApiError, CALLBACK_PATH, INFO_PATH, MAX_ID_BYTES, OAuthError, PROTOCOL, Page, RECORDS_PATH, SESSION_PATH,
    ServerInfo, TOKEN_PATH, TokenResponse, Upload, Uploaded,
};

use crate::auth::{AuthError, NewLogin, Provider, SignInKey, challenge, random_token};
use crate::config::Config;
use crate::store::{self, AccountId, DownloadError, Quota, UploadError, token_hash};

/// Longest `state` a browser may send through a sign-in.
const MAX_STATE_BYTES: usize = 1024;
/// A request still running after this is answered 408. Uploads are one transaction, and a sign-in's
/// callback waits on the provider, which gives up after 15 seconds per call.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub provider: Arc<Provider>,
    pub sign_in_key: Arc<SignInKey>,
    pub info: Arc<ServerInfo>,
    pub quota: Quota,
    pub session_idle: chrono::Duration,
    /// Added to every account's epoch.
    pub epoch: u64,
}

impl AppState {
    pub fn new(db: DatabaseConnection, config: &Config) -> AppState {
        AppState {
            db,
            provider: Arc::new(Provider::new(config.oidc.clone(), config.callback())),
            sign_in_key: Arc::new(SignInKey::random()),
            info: Arc::new(ServerInfo { protocol: PROTOCOL, limits: config.limits }),
            quota: config.quota,
            session_idle: config.session_idle,
            epoch: config.epoch.into(),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let body_limit = state.info.limits.max_request_bytes as usize;
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(INFO_PATH, get(info))
        .route(AUTHORIZE_PATH, get(authorize))
        .route(CALLBACK_PATH, get(callback))
        .route(TOKEN_PATH, post(token))
        .route(SESSION_PATH, delete(end_session))
        .route(RECORDS_PATH, post(upload).get(download))
        .route(ACCOUNT_PATH, delete(delete_account))
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn info(State(info): State<Arc<ServerInfo>>) -> Json<ServerInfo> {
    Json((*info).clone())
}

// Signing in.

#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

/// The browser's sign-in starts here: the server sends the browser on to its provider, with the
/// sign-in sealed into the provider's `state`, so it stores nothing for one nobody finishes. A
/// request it cannot trust gets a page, never a redirect to where it asked.
async fn authorize(State(state): State<AppState>, Query(q): Query<AuthorizeQuery>) -> Response {
    let new = match check_authorize(q) {
        Ok(new) => new,
        Err(reason) => return page(StatusCode::BAD_REQUEST, &format!("This sign-in link is not valid ({reason}). Start again from Vsesvit's Settings.")),
    };
    let (sealed, verifier) = state.sign_in_key.seal(&new);
    match state.provider.authorize_url(&sealed, &verifier).await {
        Ok(url) => Redirect::to(&url).into_response(),
        Err(e) => {
            tracing::warn!("{e}");
            page(StatusCode::BAD_GATEWAY, &format!("Signing in is not possible right now: {e}."))
        }
    }
}

fn check_authorize(q: AuthorizeQuery) -> Result<NewLogin, &'static str> {
    if q.response_type.as_deref() != Some("code") {
        return Err("response_type must be code");
    }
    if q.code_challenge_method.as_deref() != Some("S256") {
        return Err("code_challenge_method must be S256");
    }
    let challenge = q.code_challenge.filter(|c| is_pkce_value(c)).ok_or("no valid code_challenge")?;
    let redirect = q.redirect_uri.filter(|r| is_loopback_redirect(r)).ok_or("redirect_uri must be http on a loopback address and port")?;
    let state = q.state.filter(|s| !s.is_empty() && s.len() <= MAX_STATE_BYTES).ok_or("no valid state")?;
    Ok(NewLogin { client_redirect: redirect, client_state: state, client_challenge: challenge })
}

/// RFC 7636 §4.1: 43 to 128 unreserved characters.
fn is_pkce_value(value: &str) -> bool {
    (43..=128).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

/// RFC 8252 §7.3: a native app receives the code over http on a loopback IP literal, on whatever
/// port it could open.
fn is_loopback_redirect(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else { return false };
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    url.scheme() == "http"
        && loopback
        && url.port().is_some_and(|port| port != 0)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// The provider sends the person back here. The server learns who they are, and sends the browser
/// a one-time code at its loopback address; only the browser that started the sign-in is listening
/// there, so a sign-in link someone else started gets them nothing.
async fn callback(State(state): State<AppState>, Query(q): Query<CallbackQuery>) -> Response {
    let pending = q.state.as_deref().and_then(|s| state.sign_in_key.open(s)).filter(|p| p.started > chrono::Utc::now() - store::LOGIN_LIFETIME);
    let Some(pending) = pending else {
        return page(StatusCode::BAD_REQUEST, "This sign-in has expired or is not valid. Start again from Vsesvit's Settings.");
    };
    let login = pending.login.clone();
    let refused = |error: &str, description: String| redirect_to_browser(&login, &[("error", error), ("error_description", &description)]);
    let code = match (q.error, q.code) {
        (Some(error), _) => return refused("access_denied", q.error_description.unwrap_or(error)),
        (None, None) => return refused("server_error", "the provider sent no code".to_owned()),
        (None, Some(code)) => code,
    };
    let person = match state.provider.person(&code, &pending.upstream_verifier).await {
        Ok(person) => person,
        Err(e) => {
            tracing::warn!("sign-in: {e}");
            let error = if matches!(e, AuthError::Refused(_)) { "access_denied" } else { "server_error" };
            return refused(error, e.to_string());
        }
    };
    let one_time = random_token();
    let authorized = async {
        let account = store::account(&state.db, state.provider.issuer(), &person.subject).await?;
        store::authorize_login(&state.db, pending, account, person.name, token_hash(&one_time)).await
    };
    match authorized.await {
        Ok(()) => redirect_to_browser(&login, &[("code", &one_time)]),
        Err(e) => database_page(e),
    }
}

fn redirect_to_browser(login: &NewLogin, params: &[(&str, &str)]) -> Response {
    let mut url = Url::parse(&login.client_redirect).expect("checked when the sign-in started");
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in params {
            query.append_pair(name, value);
        }
        query.append_pair("state", &login.client_state);
    }
    Redirect::to(url.as_str()).into_response()
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
}

/// The browser trades its one-time code, with the PKCE verifier only it holds, for a session.
async fn token(State(state): State<AppState>, Form(form): Form<TokenForm>) -> Response {
    if form.grant_type.as_deref() != Some("authorization_code") {
        return oauth_error("unsupported_grant_type", "only authorization_code is supported");
    }
    let (Some(code), Some(redirect), Some(verifier)) = (form.code, form.redirect_uri, form.code_verifier) else {
        return oauth_error("invalid_request", "code, redirect_uri and code_verifier are required");
    };
    let login = match store::take_login(&state.db, &token_hash(&code)).await {
        Ok(Some(login)) => login,
        Ok(None) => return oauth_error("invalid_grant", "the code is unknown, used or expired"),
        Err(e) => return Error::Db(e).into_response(),
    };
    let account = match login.account_id {
        Some(account) if login.client_redirect == redirect && challenge(&verifier) == login.client_challenge => account,
        _ => return oauth_error("invalid_grant", "the code was issued for another sign-in"),
    };
    let session = random_token();
    if let Err(e) = store::start_session(&state.db, account, token_hash(&session)).await {
        return Error::Db(e).into_response();
    }
    let body = TokenResponse { access_token: session, token_type: "Bearer".to_owned(), name: login.name };
    ([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

fn oauth_error(error: &str, description: &str) -> Response {
    let body = OAuthError { error: error.to_owned(), error_description: Some(description.to_owned()) };
    (StatusCode::BAD_REQUEST, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

async fn end_session(State(state): State<AppState>, caller: Caller) -> Result<StatusCode, Error> {
    store::end_session(&state.db, &caller.token_hash).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A page for the person in the sign-in tab.
fn page(status: StatusCode, message: &str) -> Response {
    let message = message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Vsesvit sync</title></head>\
         <body style=\"font-family:system-ui,sans-serif;margin:4em auto;max-width:32em\"><p>{message}</p></body></html>"
    );
    (status, [(header::CACHE_CONTROL, "no-store")], Html(body)).into_response()
}

fn database_page(e: DbErr) -> Response {
    tracing::error!("database: {e}");
    page(StatusCode::INTERNAL_SERVER_ERROR, "The sync server could not sign you in because its database failed. Try again later.")
}

// Records.

async fn upload(State(state): State<AppState>, caller: Caller, Json(upload): Json<Upload>) -> Result<Json<Uploaded>, Error> {
    let limits = state.info.limits;
    if upload.records.len() > limits.max_batch as usize {
        return Err(Error::Bad(StatusCode::PAYLOAD_TOO_LARGE, format!("more than {} records", limits.max_batch)));
    }
    if let Some(r) = upload.records.iter().find(|r| r.body.len() > limits.max_record_bytes as usize) {
        return Err(Error::Bad(StatusCode::PAYLOAD_TOO_LARGE, format!("record {} is over {} bytes", r.id, limits.max_record_bytes)));
    }
    if upload.records.iter().any(|r| r.id.is_empty() || r.id.len() > MAX_ID_BYTES) {
        return Err(Error::Bad(StatusCode::BAD_REQUEST, "a record id is empty or too long".to_owned()));
    }
    let stored = store::upload(&state.db, caller.account, upload.records, upload.download_cursor, state.quota).await?;
    Ok(Json(Uploaded { stored }))
}

#[derive(Deserialize)]
struct DownloadQuery {
    #[serde(default)]
    since: u64,
    limit: Option<u32>,
}

async fn download(State(state): State<AppState>, caller: Caller, Query(q): Query<DownloadQuery>) -> Result<Json<Page>, Error> {
    let max = state.info.limits.max_batch;
    let limit = q.limit.unwrap_or(max).clamp(1, max);
    let budget = state.info.limits.max_request_bytes as usize;
    let mut page = store::download(&state.db, caller.account, q.since, limit, budget).await?;
    page.epoch += state.epoch;
    Ok(Json(page))
}

async fn delete_account(State(state): State<AppState>, caller: Caller) -> Result<StatusCode, Error> {
    store::delete_records(&state.db, caller.account).await?;
    Ok(StatusCode::NO_CONTENT)
}

impl FromRef<AppState> for Arc<ServerInfo> {
    fn from_ref(state: &AppState) -> Self {
        state.info.clone()
    }
}

/// The account whose session the request's bearer token is.
struct Caller {
    account: AccountId,
    token_hash: Vec<u8>,
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Caller, Error> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or(Error::Unauthorized)?;
        let token_hash = token_hash(token);
        let account = store::session_account(&state.db, &token_hash, state.session_idle).await?.ok_or(Error::Unauthorized)?;
        Ok(Caller { account, token_hash })
    }
}

pub enum Error {
    Unauthorized,
    Db(DbErr),
    Bad(StatusCode, String),
}

impl From<DbErr> for Error {
    fn from(e: DbErr) -> Error {
        Error::Db(e)
    }
}

impl From<UploadError> for Error {
    fn from(e: UploadError) -> Error {
        match e {
            UploadError::Db(e) => Error::Db(e),
            UploadError::OverQuota => Error::Bad(StatusCode::INSUFFICIENT_STORAGE, e.to_string()),
            UploadError::CursorAhead => Error::Bad(StatusCode::CONFLICT, e.to_string()),
        }
    }
}

impl From<DownloadError> for Error {
    fn from(e: DownloadError) -> Error {
        match e {
            DownloadError::Db(e) => Error::Db(e),
            DownloadError::CursorAhead => Error::Bad(StatusCode::CONFLICT, e.to_string()),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Error::Unauthorized => (StatusCode::UNAUTHORIZED, "the session is unknown or has ended; sign in again".to_owned()),
            Error::Db(e) => {
                tracing::error!("database: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "the database failed".to_owned())
            }
            Error::Bad(status, message) => (status, message),
        };
        let mut response = (status, Json(ApiError { error: message })).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(header::WWW_AUTHENTICATE, header::HeaderValue::from_static("Bearer"));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(redirect: &str, challenge: &str, method: &str, state: &str) -> AuthorizeQuery {
        AuthorizeQuery {
            response_type: Some("code".to_owned()),
            redirect_uri: Some(redirect.to_owned()),
            state: Some(state.to_owned()),
            code_challenge: Some(challenge.to_owned()),
            code_challenge_method: Some(method.to_owned()),
        }
    }

    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    #[test]
    fn a_sign_in_comes_back_only_to_a_loopback_port_with_s256() {
        assert!(check_authorize(query("http://127.0.0.1:51234/callback", CHALLENGE, "S256", "s")).is_ok());
        assert!(check_authorize(query("http://[::1]:51234/", CHALLENGE, "S256", "s")).is_ok());
        for (redirect, challenge, method, state) in [
            ("https://evil.example/cb", CHALLENGE, "S256", "s"),
            ("http://localhost:51234/cb", CHALLENGE, "S256", "s"),
            ("http://127.0.0.1:51234@evil.example/cb", CHALLENGE, "S256", "s"),
            ("http://127.0.0.1/cb", CHALLENGE, "S256", "s"),
            ("http://10.0.0.2:51234/cb", CHALLENGE, "S256", "s"),
            ("http://127.0.0.1:51234/cb", "short", "S256", "s"),
            ("http://127.0.0.1:51234/cb", CHALLENGE, "plain", "s"),
            ("http://127.0.0.1:51234/cb", CHALLENGE, "S256", ""),
        ] {
            assert!(check_authorize(query(redirect, challenge, method, state)).is_err(), "{redirect} {challenge} {method} {state:?}");
        }
    }
}
