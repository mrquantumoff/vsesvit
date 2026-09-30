//! The sync server's routes, blocking.

use serde::de::DeserializeOwned;
use vsesvit_sync_proto::{ACCOUNT_PATH, ApiError, INFO_PATH, PROTOCOL, Page, RECORDS_PATH, ServerInfo, Upload, Uploaded};

use crate::{Error, Http, network};

pub(crate) fn info(http: &Http, server: &str) -> Result<ServerInfo, Error> {
    let url = format!("{server}{INFO_PATH}");
    let response = http.0.get(&url).header("Accept", "application/json").call().map_err(network(&url))?;
    let info: ServerInfo = read(response, &url)?;
    if info.protocol != PROTOCOL {
        return Err(Error::Protocol(info.protocol));
    }
    Ok(info)
}

/// `Unauthorized` when the access token was refused, so the caller can refresh it and retry.
pub(crate) enum Call<T> {
    Done(T),
    Unauthorized,
}

pub(crate) fn upload(http: &Http, server: &str, token: &str, upload: &Upload) -> Result<Call<Uploaded>, Error> {
    let url = format!("{server}{RECORDS_PATH}");
    let request = http.0.post(&url).header("Authorization", &format!("Bearer {token}"));
    authorized(request.send_json(upload).map_err(network(&url))?, &url)
}

pub(crate) fn download(http: &Http, server: &str, token: &str, since: u64, limit: u32) -> Result<Call<Page>, Error> {
    let url = format!("{server}{RECORDS_PATH}?since={since}&limit={limit}");
    let request = http.0.get(&url).header("Authorization", &format!("Bearer {token}"));
    authorized(request.call().map_err(network(&url))?, &url)
}

pub(crate) fn delete_account(http: &Http, server: &str, token: &str) -> Result<Call<()>, Error> {
    let url = format!("{server}{ACCOUNT_PATH}");
    let response = http.0.delete(&url).header("Authorization", &format!("Bearer {token}")).call().map_err(network(&url))?;
    match response.status().as_u16() {
        401 => Ok(Call::Unauthorized),
        200..=299 => Ok(Call::Done(())),
        _ => Err(failure(response)),
    }
}

fn authorized<T: DeserializeOwned>(response: ureq::http::Response<ureq::Body>, url: &str) -> Result<Call<T>, Error> {
    if response.status().as_u16() == 401 {
        return Ok(Call::Unauthorized);
    }
    read(response, url).map(Call::Done)
}

pub(crate) fn read<T: DeserializeOwned>(mut response: ureq::http::Response<ureq::Body>, url: &str) -> Result<T, Error> {
    if !response.status().is_success() {
        return Err(failure(response));
    }
    response.body_mut().read_json().map_err(|e| {
        log::warn!("{url}: {e}");
        Error::Malformed(crate::host_of(url))
    })
}

fn failure(mut response: ureq::http::Response<ureq::Body>) -> Error {
    let status = response.status().as_u16();
    let message = response
        .body_mut()
        .read_json::<ApiError>()
        .map(|e| e.error)
        .unwrap_or_else(|_| response.status().canonical_reason().unwrap_or("error").to_owned());
    Error::Server { status, message }
}
