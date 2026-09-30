//! The account a profile is signed in with, and the round: gather local changes (UI thread),
//! upload them and download one page (worker), apply the page (UI thread).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vsesvit_core::crdt::Seq;
use vsesvit_core::sync::{ApplyReport, DataType, Kind, SyncStore, WireRecord};
use vsesvit_sync_proto::{Limits, Page, Record, Upload};

use crate::oidc::{self, Provider, Tokens};
use crate::server::{self, Call};
use crate::{Error, Http, now_secs};

/// The key in core's `sync_state`. An empty value means signed out.
const STATE_KEY: &str = "account";
/// The key in core's sealed `sync_secrets`, which holds the tokens apart from the rest.
const TOKENS_KEY: &str = "account.tokens";

/// A profile's sign-in to one sync server, with how far it has synced. Saved into the profile after
/// every round: the tokens sealed by the profile's vault, the rest as plain JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Random per sign-in, so a round started before a sign-out cannot save over what came after.
    sign_in: String,
    server: String,
    name: Option<String>,
    provider: Provider,
    /// Read from JSON only to move an account saved before the vault into it.
    #[serde(default, skip_serializing)]
    tokens: Tokens,
    limits: Limits,
    download_cursor: u64,
    /// Per `Kind::code`, the `Seq` to pass to `changes_since` next.
    upload_cursors: BTreeMap<u8, u64>,
    /// The types `download_cursor` has been applying. Records of other types were skipped as it
    /// passed them, so turning one on starts the download over.
    #[serde(default = "every_type")]
    downloading: BTreeSet<DataType>,
    last_synced: Option<u64>,
}

fn every_type() -> BTreeSet<DataType> {
    DataType::ALL.into_iter().collect()
}

impl Account {
    pub(crate) fn new(server: String, name: Option<String>, provider: Provider, tokens: Tokens, limits: Limits) -> Account {
        Account {
            sign_in: crate::oidc::random_token(),
            server,
            name,
            provider,
            tokens,
            limits,
            download_cursor: 0,
            upload_cursors: BTreeMap::new(),
            downloading: every_type(),
            last_synced: None,
        }
    }

    /// The server's base URL.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// The name the provider gave, for Settings.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Unix seconds of the last round that completed.
    pub fn last_synced(&self) -> Option<u64> {
        self.last_synced
    }

    /// `Err` when the tokens cannot be unsealed, such as with the system keyring locked: the
    /// profile is still signed in, and loading again once it is unlocked works.
    pub fn load(store: &mut SyncStore<'_>) -> Result<Option<Account>, Error> {
        let Some(bytes) = store.engine_state(STATE_KEY)?.filter(|b| !b.is_empty()) else {
            return Ok(None);
        };
        let mut account: Account = match serde_json::from_slice(&bytes) {
            Ok(account) => account,
            Err(e) => {
                log::warn!("the saved sync account is unreadable, so this device is signed out: {e}");
                return Ok(None);
            }
        };
        match store.secret_state(TOKENS_KEY)? {
            Some(sealed) => match serde_json::from_slice(&sealed) {
                Ok(tokens) => account.tokens = tokens,
                Err(e) => {
                    log::warn!("the saved sync tokens are unreadable, so this device is signed out: {e}");
                    return Ok(None);
                }
            },
            // Saved before the vault, with the tokens in the plain JSON: seal them now.
            None if !account.tokens.access.is_empty() => account.save(store)?,
            None => return Ok(None),
        }
        Ok(Some(account))
    }

    pub fn save(&self, store: &mut SyncStore<'_>) -> Result<(), Error> {
        let tokens = serde_json::to_vec(&self.tokens).expect("the tokens serialize");
        store.set_secret_state(TOKENS_KEY, &tokens)?;
        let bytes = serde_json::to_vec(self).expect("the account serializes");
        Ok(store.set_engine_state(STATE_KEY, &bytes)?)
    }

    /// Signs the profile out. The records stay on this device and on the server. Works with the
    /// keyring locked: forgetting a sealed value needs no key.
    pub fn forget(store: &mut SyncStore<'_>) -> Result<(), Error> {
        store.set_engine_state(STATE_KEY, &[])?;
        Ok(store.set_secret_state(TOKENS_KEY, &[])?)
    }

    /// Worker thread, after [`Account::forget`]: asks the provider to revoke the refresh token, where
    /// it offers that.
    pub fn revoke(self, http: &Http) {
        oidc::revoke(http, &self.provider, &self.tokens);
    }

    /// Worker thread: deletes everything the server holds for the account. Every device keeps its
    /// copy, but uploads only what changes from then on: a device's cursors say the server has
    /// the rest. Signing in again starts a device's cursors over, and uploads everything it holds.
    pub fn delete_server_data(mut self, http: &Http) -> Result<Account, Error> {
        let server = self.server.clone();
        with_token(http, &mut self, Refresh::Allowed, |token| server::delete_account(http, &server, token))?;
        Ok(self)
    }
}

/// Local changes on their way to the server. Built on the UI thread, run on a worker.
pub struct Round {
    account: Account,
    records: Vec<Record>,
    upto: BTreeMap<u8, u64>,
    more_up: bool,
    types: BTreeSet<DataType>,
}

impl Round {
    /// Collects up to one batch of local changes of the `types` this device syncs, oldest first per
    /// kind. A record larger than the server takes is left out and logged; nothing else could send
    /// it. The kinds of other types keep their cursors, so turning a type on uploads what changed
    /// while it was off.
    pub fn gather(store: &mut SyncStore<'_>, mut account: Account, types: &[DataType]) -> Result<Round, Error> {
        let types: BTreeSet<DataType> = types.iter().copied().collect();
        if !types.is_subset(&account.downloading) {
            account.download_cursor = 0;
        }
        account.downloading = types.clone();
        let budget = account.limits.max_batch as usize;
        let mut records = Vec::new();
        let mut upto = BTreeMap::new();
        let mut more_up = false;
        for &kind in Kind::ALL.iter().filter(|k| types.contains(&DataType::of(**k))) {
            if records.len() >= budget {
                more_up = true;
                break;
            }
            let since = Seq(account.upload_cursors.get(&kind.code()).copied().unwrap_or(0));
            let batch = store.changes_since(kind, since, budget - records.len())?;
            upto.insert(kind.code(), batch.upto.0);
            more_up |= batch.more;
            for wire in batch.records {
                if wire.body.len() > account.limits.max_record_bytes as usize {
                    log::warn!("not syncing {kind:?} {}: {} bytes is over the server's limit", wire.id, wire.body.len());
                    continue;
                }
                records.push(Record { kind: kind.code(), id: wire.id, body: wire.body });
            }
        }
        Ok(Round { account, records, upto, more_up, types })
    }

    /// Records this round uploads.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Worker thread: uploads, then downloads one page. Refreshes the access token when it is
    /// about to expire or the server refuses it.
    pub fn run(self, http: &Http) -> Exchanged {
        self.exchange(http, Refresh::Allowed)
    }

    /// Worker thread, as the browser quits: [`Round::run`] without ever refreshing the access
    /// token, so the shell can stop waiting for it at any moment. A refresh rotates the refresh
    /// token, and a round abandoned after one would lose the new token and sign the profile out.
    /// When the access token would need refreshing it fails with [`Error::NeedsRefresh`] at once.
    pub fn run_final(self, http: &Http) -> Exchanged {
        self.exchange(http, Refresh::Forbidden)
    }

    fn exchange(self, http: &Http, refresh: Refresh) -> Exchanged {
        let Round { mut account, records, upto, more_up, types } = self;
        let result = exchange(http, &mut account, records, refresh);
        Exchanged { account, upto, more_up, types, result }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Refresh {
    Allowed,
    Forbidden,
}

fn exchange(http: &Http, account: &mut Account, records: Vec<Record>, refresh: Refresh) -> Result<Page, Error> {
    let server = account.server.clone();
    let limits = account.limits;
    for chunk in chunks(records, limits) {
        let upload = Upload { records: chunk };
        with_token(http, account, refresh, |token| server::upload(http, &server, token, &upload))?;
    }
    let since = account.download_cursor;
    with_token(http, account, refresh, |token| server::download(http, &server, token, since, limits.max_batch))
}

/// Calls with a fresh access token, and once more with a refreshed one if the server refuses it.
fn with_token<T>(
    http: &Http,
    account: &mut Account,
    refresh: Refresh,
    call: impl Fn(&str) -> Result<Call<T>, Error>,
) -> Result<T, Error> {
    if !account.tokens.fresh() {
        if refresh == Refresh::Forbidden {
            return Err(Error::NeedsRefresh);
        }
        account.tokens = oidc::refresh(http, &account.provider, &account.tokens)?;
    }
    if let Call::Done(value) = call(&account.tokens.access)? {
        return Ok(value);
    }
    if refresh == Refresh::Forbidden {
        return Err(Error::NeedsRefresh);
    }
    account.tokens = oidc::refresh(http, &account.provider, &account.tokens)?;
    match call(&account.tokens.access)? {
        Call::Done(value) => Ok(value),
        Call::Unauthorized => Err(Error::SignInExpired),
    }
}

/// Splits an upload under the server's record and request limits.
fn chunks(records: Vec<Record>, limits: Limits) -> Vec<Vec<Record>> {
    let max_bytes = limits.max_request_bytes as usize / 10 * 9;
    let mut chunks: Vec<Vec<Record>> = Vec::new();
    let mut bytes = 0;
    for record in records {
        // base64 grows a body by a third; the rest is the JSON around it.
        let size = record.body.len().div_ceil(3) * 4 + record.id.len() * 2 + 48;
        match chunks.last_mut() {
            Some(chunk) if chunk.len() < limits.max_batch as usize && bytes + size <= max_bytes => {
                bytes += size;
                chunk.push(record);
            }
            _ => {
                bytes = size;
                chunks.push(vec![record]);
            }
        }
    }
    chunks
}

/// A round back from the network, to apply on the UI thread.
pub struct Exchanged {
    account: Account,
    upto: BTreeMap<u8, u64>,
    more_up: bool,
    types: BTreeSet<DataType>,
    result: Result<Page, Error>,
}

/// The account to keep (its tokens may be new even when the round failed) and what the round did.
pub struct Finished {
    pub account: Account,
    pub result: Result<Synced, Error>,
}

pub struct Synced {
    pub report: ApplyReport,
    /// Run another round now: more is waiting on either side, or the merge changed records that
    /// the server should hear about.
    pub again: bool,
}

impl Exchanged {
    /// Applies the downloaded page, moves the cursors and saves the account. When the profile
    /// signed out (or in again) while the round ran, it changes nothing and says
    /// [`Error::SignedOut`].
    pub fn finish(self, store: &mut SyncStore<'_>) -> Finished {
        let Exchanged { mut account, upto, more_up, types, result } = self;
        match Account::load(store) {
            Ok(Some(current)) if current.sign_in == account.sign_in => {}
            Ok(_) => return Finished { account, result: Err(Error::SignedOut) },
            Err(e) => return Finished { account, result: Err(e) },
        }
        let mut result = result.and_then(|page| {
            account.upload_cursors.extend(upto);
            let more_down = page.more;
            let report = apply(store, &mut account, page, &types)?;
            Ok(Synced { again: more_up || more_down || report.merged > 0, report })
        });
        if let Err(e) = account.save(store) {
            log::error!("saving the sync account: {e}");
            if result.is_ok() {
                result = Err(e);
            }
        }
        Finished { account, result }
    }
}

/// Applies the page's records of the `types` this device syncs. The cursor passes the others; turning
/// one of them on later starts the download over (see [`Round::gather`]).
fn apply(store: &mut SyncStore<'_>, account: &mut Account, page: Page, types: &BTreeSet<DataType>) -> Result<ApplyReport, Error> {
    let records = page
        .records
        .into_iter()
        .filter_map(|r| Some(WireRecord { kind: Kind::from_code(r.kind)?, id: r.id, body: r.body }))
        .filter(|r| types.contains(&DataType::of(r.kind)))
        .collect();
    let report = store.apply(records)?;
    for rejected in &report.rejected {
        log::warn!("sync rejected {:?} {}: {}", rejected.kind, rejected.id, rejected.reason);
    }
    account.download_cursor = page.cursor;
    account.last_synced = Some(now_secs());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(bytes: usize) -> Record {
        Record { kind: 1, id: "x".to_owned(), body: vec![0; bytes] }
    }

    #[test]
    fn uploads_split_by_count_and_by_bytes() {
        let limits = Limits { max_batch: 2, max_record_bytes: 1000, max_request_bytes: 10_000 };
        let sizes: Vec<usize> = chunks((0..5).map(|_| record(10)).collect(), limits).iter().map(Vec::len).collect();
        assert_eq!(sizes, [2, 2, 1]);
        let limits = Limits { max_batch: 100, max_record_bytes: 3000, max_request_bytes: 5000 };
        let sizes: Vec<usize> = chunks((0..4).map(|_| record(1500)).collect(), limits).iter().map(Vec::len).collect();
        assert_eq!(sizes, [2, 2]);
        assert!(chunks(Vec::new(), limits).is_empty());
    }
}
