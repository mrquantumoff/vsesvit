//! The account a profile is signed in with, and the round: gather local changes (UI thread),
//! upload them and download one page (worker), apply the page (UI thread).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vsesvit_core::crdt::Seq;
use vsesvit_core::sync::{ApplyReport, DataType, Kind, SyncStore, WireRecord};
use vsesvit_sync_proto::{Limits, Page, Record, Upload};

use crate::auth;
use crate::server::{self, Call};
use crate::{Error, Http, now_secs};

/// The key in core's `sync_state`. An empty value means signed out.
const STATE_KEY: &str = "account";
/// The key in core's sealed `sync_secrets`, which holds the session apart from the rest.
const SESSION_KEY: &str = "account.session";
/// Where protocol 1 kept the provider's tokens; cleared on sign-out.
const OLD_TOKENS_KEY: &str = "account.tokens";

/// A profile's sign-in to one sync server, with how far it has synced. Saved into the profile after
/// every round: the session sealed by the profile's vault, the rest as plain JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Random per sign-in, so a round started before a sign-out cannot save over what came after.
    sign_in: String,
    server: String,
    name: Option<String>,
    /// The server's session, sealed apart from the JSON.
    #[serde(skip)]
    session: String,
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
    pub(crate) fn new(server: String, name: Option<String>, session: String, limits: Limits) -> Account {
        Account {
            sign_in: auth::random_token(),
            server,
            name,
            session,
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

    /// `Err` when the session cannot be unsealed, such as with the system keyring locked: the
    /// profile is still signed in, and loading again once it is unlocked works. An account saved by
    /// a version that signed in with the provider directly loads as signed out.
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
        let Some(sealed) = store.secret_state(SESSION_KEY)? else {
            log::info!("the saved sync account has no session, so this device signs in again");
            return Ok(None);
        };
        match String::from_utf8(sealed) {
            Ok(session) if !session.is_empty() => account.session = session,
            _ => {
                log::warn!("the saved sync session is unreadable, so this device is signed out");
                return Ok(None);
            }
        }
        Ok(Some(account))
    }

    pub fn save(&self, store: &mut SyncStore<'_>) -> Result<(), Error> {
        store.set_secret_state(SESSION_KEY, self.session.as_bytes())?;
        let bytes = serde_json::to_vec(self).expect("the account serializes");
        Ok(store.set_engine_state(STATE_KEY, &bytes)?)
    }

    /// Signs the profile out. The records stay on this device and on the server. Works with the
    /// keyring locked: forgetting a sealed value needs no key.
    pub fn forget(store: &mut SyncStore<'_>) -> Result<(), Error> {
        store.set_engine_state(STATE_KEY, &[])?;
        store.set_secret_state(OLD_TOKENS_KEY, &[])?;
        Ok(store.set_secret_state(SESSION_KEY, &[])?)
    }

    /// Worker thread, after [`Account::forget`]: ends the session on the server, best effort.
    pub fn revoke(self, http: &Http) {
        auth::sign_out(http, &self.server, &self.session);
    }

    /// Worker thread: deletes everything the server holds for the account. Every device keeps its
    /// copy, but uploads only what changes from then on: a device's cursors say the server has
    /// the rest. Signing in again starts a device's cursors over, and uploads everything it holds.
    pub fn delete_server_data(self, http: &Http) -> Result<Account, Error> {
        let server = self.server.clone();
        authorized(&self, |token| server::delete_account(http, &server, token))?;
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

    /// Worker thread: uploads, then downloads one page. Nothing it does needs finishing, so the
    /// shell may stop waiting for it at any moment, as when the browser quits.
    pub fn run(self, http: &Http) -> Exchanged {
        let Round { account, records, upto, more_up, types } = self;
        let result = exchange(http, &account, records);
        Exchanged { account, upto, more_up, types, result }
    }
}

fn exchange(http: &Http, account: &Account, records: Vec<Record>) -> Result<Page, Error> {
    let server = &account.server;
    let limits = account.limits;
    for chunk in chunks(records, limits) {
        let upload = Upload { records: chunk };
        authorized(account, |token| server::upload(http, server, token, &upload))?;
    }
    let since = account.download_cursor;
    authorized(account, |token| server::download(http, server, token, since, limits.max_batch))
}

/// Calls with the session. A session the server no longer knows (signed out elsewhere, unused too
/// long) means signing in again.
fn authorized<T>(account: &Account, call: impl FnOnce(&str) -> Result<Call<T>, Error>) -> Result<T, Error> {
    match call(&account.session)? {
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

/// The account to keep and what the round did.
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
