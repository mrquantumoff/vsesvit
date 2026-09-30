//! The HTTP routes of `vsesvit_sync_proto`.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, FromRef, FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use sea_orm::{DatabaseConnection, DbErr};
use serde::Deserialize;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use vsesvit_sync_proto::{ACCOUNT_PATH, ApiError, INFO_PATH, Page, PROTOCOL, RECORDS_PATH, ServerInfo, Upload, Uploaded};

use crate::auth::{AuthError, Verifier};
use crate::config::Config;
use crate::store::{self, AccountId, Quota, UploadError};

/// Longest record id accepted; core's longest ids are history URLs.
const MAX_ID_BYTES: usize = 8 * 1024;
/// A request still running after this is answered 408. Uploads are one transaction, and the first
/// request of a token also waits on the provider, which gives up after 15 seconds.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub verifier: Arc<Verifier>,
    pub info: Arc<ServerInfo>,
    pub quota: Quota,
}

impl AppState {
    pub fn new(db: DatabaseConnection, config: &Config) -> AppState {
        AppState {
            db,
            verifier: Arc::new(Verifier::new(config.auth.issuer.clone(), config.allowed_client_ids.clone())),
            info: Arc::new(ServerInfo { protocol: PROTOCOL, auth: config.auth.clone(), limits: config.limits }),
            quota: config.quota,
        }
    }
}

pub fn router(state: AppState) -> Router {
    let body_limit = state.info.limits.max_request_bytes as usize;
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(INFO_PATH, get(info))
        .route(RECORDS_PATH, post(upload).get(download))
        .route(ACCOUNT_PATH, axum::routing::delete(delete_account))
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn info(State(info): State<Arc<ServerInfo>>) -> Json<ServerInfo> {
    Json((*info).clone())
}

async fn upload(State(state): State<AppState>, caller: Caller, Json(upload): Json<Upload>) -> Result<Json<Uploaded>, Error> {
    let limits = state.info.limits;
    if upload.records.len() > limits.max_batch as usize {
        return Err(Error::bad(StatusCode::PAYLOAD_TOO_LARGE, format!("more than {} records", limits.max_batch)));
    }
    if let Some(r) = upload.records.iter().find(|r| r.body.len() > limits.max_record_bytes as usize) {
        return Err(Error::bad(StatusCode::PAYLOAD_TOO_LARGE, format!("record {} is over {} bytes", r.id, limits.max_record_bytes)));
    }
    if upload.records.iter().any(|r| r.id.is_empty() || r.id.len() > MAX_ID_BYTES) {
        return Err(Error::bad(StatusCode::BAD_REQUEST, "a record id is empty or too long".to_owned()));
    }
    let stored = store::upload(&state.db, caller.account, upload.records, state.quota).await?;
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
    Ok(Json(store::download(&state.db, caller.account, q.since, limit, budget).await?))
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

/// The account a request's bearer token belongs to, created on its first request.
struct Caller {
    account: AccountId,
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
            .ok_or(Error::Auth(AuthError::Unauthorized))?;
        let subject = state.verifier.subject(token).await?;
        let account = store::account(&state.db, state.verifier.issuer(), &subject).await?;
        Ok(Caller { account })
    }
}

pub enum Error {
    Auth(AuthError),
    Db(DbErr),
    Bad(StatusCode, String),
}

impl Error {
    fn bad(status: StatusCode, message: String) -> Error {
        Error::Bad(status, message)
    }
}

impl From<AuthError> for Error {
    fn from(e: AuthError) -> Error {
        Error::Auth(e)
    }
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
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Error::Auth(e @ AuthError::Unauthorized) => (StatusCode::UNAUTHORIZED, e.to_string()),
            Error::Auth(e @ AuthError::WrongClient) => (StatusCode::FORBIDDEN, e.to_string()),
            Error::Auth(e @ AuthError::Busy) => (StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
            Error::Auth(e @ AuthError::Provider(_)) => {
                tracing::warn!("{e}");
                (StatusCode::BAD_GATEWAY, e.to_string())
            }
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
