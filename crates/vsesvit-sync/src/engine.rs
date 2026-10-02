//! The account a profile is signed in with, and the round: gather local changes (UI thread),
//! upload them and download one page (worker), apply the page (UI thread).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vsesvit_core::crdt::Seq;
use vsesvit_core::sync::{ApplyReport, DataType, Kind, SyncStore, WireRecord};
use vsesvit_sync_proto::{Limits, MAX_ID_BYTES, Page, Record, Upload};

use crate::auth;
use crate::server;
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
    /// The `Kind` codes of the build that last moved `download_cursor`. Records of kinds it did not
    /// know were skipped as it passed them, so a build that knows more starts the download over.
    #[serde(default = "kinds_before_tracking")]
    known_kinds: BTreeSet<u8>,
    /// The [`Page::epoch`] the cursors belong to. `None` while they are at the start, until a page
    /// comes.
    #[serde(default = "epoch_before_tracking")]
    epoch: Option<u64>,
    last_synced: Option<u64>,
}

fn every_type() -> BTreeSet<DataType> {
    DataType::ALL.into_iter().collect()
}

fn every_kind() -> BTreeSet<u8> {
    Kind::ALL.iter().map(|k| k.code()).collect()
}

/// The kinds there were when the account began keeping `known_kinds`. Fixed, not `Kind::ALL`, so
/// a build with a new kind still sees that an older account's download skipped it.
fn kinds_before_tracking() -> BTreeSet<u8> {
    BTreeSet::from([1, 2, 3, 4, 5, 6, 7, 8, 12])
}

/// The epoch of every page before servers had them, so an account saved then still sees a new one.
fn epoch_before_tracking() -> Option<u64> {
    Some(0)
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
            known_kinds: every_kind(),
            epoch: None,
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

    /// Worker thread: deletes everything the server holds for the account, and signs every device
    /// out there. Each keeps its copy; signing in again starts a device's cursors over, and uploads
    /// everything it holds.
    pub fn delete_server_data(self, http: &Http) -> Result<Account, Error> {
        server::delete_account(http, &self.server, &self.session)?;
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
    /// kind. A record larger than the server takes, or with an id it refuses, is left out and
    /// logged; nothing else could send it. The kinds of other types keep their cursors, so turning
    /// a type on uploads what changed while it was off.
    pub fn gather(store: &mut SyncStore<'_>, mut account: Account, types: &[DataType]) -> Result<Round, Error> {
        let types: BTreeSet<DataType> = types.iter().copied().collect();
        let kinds = every_kind();
        if !types.is_subset(&account.downloading) || !kinds.is_subset(&account.known_kinds) {
            account.download_cursor = 0;
        }
        account.downloading = types.clone();
        account.known_kinds = kinds;
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
                if wire.id.is_empty() || wire.id.len() > MAX_ID_BYTES || wire.body.len() > account.limits.max_record_bytes as usize {
                    log::warn!("not syncing {kind:?}: its {}-byte id or {}-byte body is over the server's limits", wire.id.len(), wire.body.len());
                    continue;
                }
                records.push(Record { kind: kind.code(), id: wire.id, body: wire.body });
            }
        }
        Ok(Round { account, records, upto, more_up, types })
    }

    /// Whether this round uploads nothing.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Worker thread: uploads, then downloads one page. An upload the server refuses, as over the
    /// account's quota, still lets the download run, so the device keeps receiving the others'
    /// changes. One refused as too large first asks the server for its limits again, since they may
    /// have changed after sign-in. Nothing it does needs finishing, so the shell may stop waiting
    /// for it at any moment, as when the browser quits.
    pub fn run(self, http: &Http) -> Exchanged {
        let Round { mut account, records, upto, mut more_up, types } = self;
        let refused = match upload(http, &account, records) {
            Ok(()) => None,
            // 409: the server went back to an older copy of the account, and `finish` starts over.
            Err(e @ Error::Server { status, .. }) if status != 409 => Some(e),
            Err(e) => return Exchanged { account, upto, more_up, types, refused: None, result: Err(e) },
        };
        if let Some(e) = &refused {
            log::warn!("the sync server refused this device's changes: {e}");
            // The same records would be refused again, unless the limits they were gathered under
            // have changed.
            more_up = matches!(e, Error::Server { status: 413, .. }) && relearn_limits(http, &mut account);
        }
        let result = server::download(http, &account.server, &account.session, account.download_cursor, account.limits);
        Exchanged { account, upto, more_up, types, refused, result }
    }
}

fn upload(http: &Http, account: &Account, records: Vec<Record>) -> Result<(), Error> {
    for chunk in chunks(records, account.limits) {
        let upload = Upload { records: chunk, download_cursor: account.download_cursor };
        server::upload(http, &account.server, &account.session, &upload)?;
    }
    Ok(())
}

/// Asks the server for its limits again. `true` when they changed.
fn relearn_limits(http: &Http, account: &mut Account) -> bool {
    match server::info(http, &account.server) {
        Ok(info) if info.limits != account.limits => {
            log::info!("the sync server's limits are now {:?}", info.limits);
            account.limits = info.limits;
            true
        }
        Ok(_) => false,
        Err(e) => {
            log::warn!("asking the sync server for its limits: {e}");
            false
        }
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
    refused: Option<Error>,
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
    /// An upload the server refused this round, which waits for a later sync. The page still
    /// came down and is applied, so `report` stands.
    pub refused: Option<Error>,
}

impl Exchanged {
    /// Applies the downloaded page, moves the cursors and saves the account. When the profile
    /// signed out (or in again) while the round ran, it changes nothing and says
    /// [`Error::SignedOut`].
    pub fn finish(self, store: &mut SyncStore<'_>) -> Finished {
        let Exchanged { mut account, upto, more_up, types, refused, result } = self;
        match Account::load(store) {
            Ok(Some(current)) if current.sign_in == account.sign_in => {}
            Ok(_) => return Finished { account, result: Err(Error::SignedOut) },
            Err(e) => return Finished { account, result: Err(e) },
        }
        let went_back = match &result {
            Ok(page) => account.epoch.is_some_and(|epoch| epoch != page.epoch),
            Err(e) => matches!(e, Error::Server { status: 409, .. }),
        };
        let mut result = if went_back {
            // The server went back to an older copy of the account, as to a backup: everything
            // goes up and comes down again, which changes nothing this device already has.
            log::warn!("the sync server lost some of what this device synced with it; syncing everything again");
            account.upload_cursors.clear();
            account.download_cursor = 0;
            account.epoch = None;
            Ok(Synced { report: ApplyReport::default(), again: true, refused: None })
        } else {
            result.and_then(|page| {
                if refused.is_none() {
                    account.upload_cursors.extend(upto);
                }
                account.epoch = Some(page.epoch);
                let more_down = page.more;
                let report = apply(store, &mut account, page, &types)?;
                let again = more_up || more_down || report.merged > 0;
                Ok(Synced { report, again, refused })
            })
        };
        if let Err(e) = account.save(store) {
            log::error!("saving the sync account: {e}");
            if result.is_ok() {
                result = Err(e);
            }
        }
        Finished { account, result }
    }
}

/// Applies the page's records of the `types` this device syncs. The cursor passes the others, and
/// records of kinds this build does not know; turning one of those types on later, or updating to a
/// build that knows the kind, starts the download over (see [`Round::gather`]).
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

    #[test]
    fn a_record_whose_id_the_server_refuses_is_left_out_and_passed() {
        use vsesvit_core::history::Transition;
        use vsesvit_core::vault::KeyStore;
        use vsesvit_core::{OpenOptions, Profile, Url};

        let dir = std::env::temp_dir().join(format!("vsesvit-sync-long-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut profile = Profile::open(&dir, OpenOptions { key_store: KeyStore::Basic, ..OpenOptions::default() }).unwrap();
        let long = Url::parse(&format!("https://example.com/?q={}", "a".repeat(9000))).unwrap();
        profile.history().record_visit(&Url::parse("https://example.com/short").unwrap(), Transition::Link).unwrap();
        profile.history().record_visit(&long, Transition::Link).unwrap();

        let limits = Limits { max_batch: 100, max_record_bytes: 1 << 20, max_request_bytes: 4 << 20 };
        let account = Account::new("https://sync.example.com".to_owned(), None, "session".to_owned(), limits);
        let mut store = profile.sync();
        let round = Round::gather(&mut store, account, &[DataType::History]).unwrap();
        let ids: Vec<usize> = round.records.iter().map(|r| r.id.len()).collect();
        assert_eq!(ids, ["https://example.com/short".len()], "the long URL is left out");
        let all = store.changes_since(Kind::HistoryPages, Seq(0), 100).unwrap();
        assert_eq!(round.upto[&Kind::HistoryPages.code()], all.upto.0, "the cursor passes the long one too");
        drop(profile);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An account as some build saved it, with its download cursor at 100 and these kind codes
    /// known, or no list at all.
    fn saved_account(known_kinds: Option<&[u8]>) -> Account {
        let limits = Limits { max_batch: 100, max_record_bytes: 1 << 20, max_request_bytes: 4 << 20 };
        let mut json = serde_json::to_value(Account::new("https://sync.example.com".to_owned(), None, "session".to_owned(), limits)).unwrap();
        json["download_cursor"] = 100.into();
        match known_kinds {
            Some(codes) => json["known_kinds"] = codes.into(),
            None => drop(json.as_object_mut().unwrap().remove("known_kinds")),
        }
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_build_that_knows_a_new_kind_downloads_everything_again() {
        use vsesvit_core::vault::KeyStore;
        use vsesvit_core::{OpenOptions, Profile};

        let dir = std::env::temp_dir().join(format!("vsesvit-sync-new-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut profile = Profile::open(&dir, OpenOptions { key_store: KeyStore::Basic, ..OpenOptions::default() }).unwrap();
        let mut store = profile.sync();
        let every_code: Vec<u8> = Kind::ALL.iter().map(|k| k.code()).collect();
        let mut gather = |account: Account| {
            let round = Round::gather(&mut store, account, &DataType::ALL).unwrap();
            let known = serde_json::to_value(&round.account).unwrap()["known_kinds"].clone();
            (round.account.download_cursor, serde_json::from_value::<Option<Vec<u8>>>(known).unwrap())
        };

        // Saved by a build that knew every kind but SitePermissions: the records of it passed then
        // were skipped.
        assert_eq!(gather(saved_account(Some(&[1, 2, 3, 4, 5, 6, 7, 8]))), (0, Some(every_code.clone())));
        assert_eq!(gather(saved_account(Some(&every_code))), (100, Some(every_code.clone())), "no new kind, no new start");
        // Saved before the account kept the list, by a build that knew the kinds there were then.
        assert_eq!(gather(saved_account(None)), (100, Some(every_code.clone())));
        drop(profile);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
