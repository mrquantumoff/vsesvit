//! The account a profile is signed in with, and the round: gather local changes (UI thread),
//! upload them, sealed when the account has a passphrase, and download and open one page (worker),
//! apply the page (UI thread). The sync passphrase's steps are [`PassphraseJob`]. How records are
//! encrypted, and why, is docs/design/sync-encryption.md.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vsesvit_core::crdt::Seq;
use vsesvit_core::permissions::Permission;
use vsesvit_core::sync::{ApplyReport, DataType, Kind, SyncStore, WireRecord};
use vsesvit_sync_proto::{Limits, MAX_ID_BYTES, Page, Record, Upload};

use crate::auth;
use crate::crypto::{KEYS_ID, KEYS_KIND, KeyRecord, Keyring, Opened, Passphrase, SEALED_KIND, Unopened, Unwrapped};
use crate::server;
use crate::{Error, Http, now_secs};

/// The key in core's `sync_state`. An empty value means signed out.
const STATE_KEY: &str = "account";
/// The key in core's sealed `sync_secrets`, which holds the session apart from the rest.
const SESSION_KEY: &str = "account.session";
/// The key in core's sealed `sync_secrets` that holds the account's keyring.
const KEYRING_KEY: &str = "account.keyring";
/// Where protocol 1 kept the provider's tokens; cleared on sign-in and sign-out.
const OLD_TOKENS_KEY: &str = "account.tokens";
/// Rounds one sync runs at most; the next sync takes what is left.
pub const MAX_ROUNDS: usize = 50;

/// A profile's sign-in to one sync server, with how far it has synced. Saved into the profile after
/// every round: the session and the keyring sealed by the profile's vault, the rest as plain JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Random per sign-in, so a round started before a sign-out cannot save over what came after.
    sign_in: String,
    server: String,
    name: Option<String>,
    /// The server's session, sealed apart from the JSON.
    #[serde(skip)]
    session: String,
    /// The account's keys, once the passphrase was set or entered here; sealed apart from the JSON.
    #[serde(skip)]
    keyring: Option<Keyring>,
    /// `keyring` wrapped by the passphrase, as the key slot holds it.
    #[serde(default)]
    own_keys: Option<KeyRecord>,
    /// The newest key record this device saw in the account's key slot.
    #[serde(default)]
    server_keys: ServerKeys,
    /// This device set the passphrase, and has not yet seen its key record in the key slot. Any
    /// other key record there means another device set one first, and this one takes that.
    #[serde(default)]
    pending: bool,
    /// While `pending`, this device seals a copy of each plaintext record the account holds, until
    /// its download reaches the end; then it sends its key record.
    #[serde(default)]
    converting: bool,
    /// Send `own_keys` with the next round, as after the server lost it.
    #[serde(default)]
    upload_keys: bool,
    /// This device entered the passphrase of an account others sync, and uploads nothing until
    /// its download reaches the end: what it holds may be older than what they sealed, and
    /// uploading first would put the older copies over theirs.
    #[serde(default)]
    joining: bool,
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
    /// The site permissions, by wire name, of the build that last moved `download_cursor`.
    /// Records of a permission it did not know were rejected as it passed them, so a build that
    /// knows more starts the download over, as for a kind.
    #[serde(default = "permissions_before_tracking")]
    known_permissions: BTreeSet<String>,
    /// The [`Page::epoch`] the cursors belong to. `None` while they are at the start, until a page
    /// comes.
    #[serde(default = "epoch_before_tracking")]
    epoch: Option<u64>,
    last_synced: Option<u64>,
}

/// The account's key slot as this device last saw it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ServerKeys {
    /// Not looked through the whole account yet.
    #[default]
    Unknown,
    /// A look through the whole account found no key record.
    Missing,
    Found(KeyRecord),
}

/// A key record in a page, to a device that syncs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Verdict {
    /// This device's own.
    Ours,
    /// Another device set a passphrase before this one's key record reached the key slot.
    SetElsewhere,
    /// Another device changed the passphrase.
    Replaced,
    /// Older than this device's, or altered: a server that went back to a backup, or a malicious
    /// one, holds it.
    Stale,
}

/// Where end-to-end encryption stands on this device. [`Encryption::Off`] syncs unencrypted and
/// [`Encryption::Ready`] encrypted; the others download nothing but the account's key record, and
/// upload nothing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Encryption {
    /// Looking through the account for its key record.
    Checking,
    /// The account has no sync passphrase, so it syncs unencrypted, as before Vsesvit encrypted
    /// sync. Setting one encrypts it.
    Off,
    /// The account's passphrase has not been entered on this device.
    Enter,
    /// Another device changed the passphrase; the new one has not been entered here.
    Changed,
    Ready,
}

fn every_type() -> BTreeSet<DataType> {
    DataType::ALL.into_iter().collect()
}

/// Core's kinds, and the two this crate's records travel as.
fn every_kind() -> BTreeSet<u8> {
    Kind::ALL.iter().map(|k| k.code()).chain([KEYS_KIND, SEALED_KIND]).collect()
}

/// The kinds there were when the account began keeping `known_kinds`. Fixed, not `Kind::ALL`, so
/// a build with a new kind still sees that an older account's download skipped it.
fn kinds_before_tracking() -> BTreeSet<u8> {
    BTreeSet::from([1, 2, 3, 4, 5, 6, 7, 8, 12])
}

fn every_permission() -> BTreeSet<String> {
    Permission::ALL.iter().map(|p| p.key().to_owned()).collect()
}

/// The site permissions there were when the account began keeping `known_permissions`.
fn permissions_before_tracking() -> BTreeSet<String> {
    ["camera", "microphone", "location", "notifications", "screen_share", "clipboard_read", "midi"]
        .into_iter()
        .map(str::to_owned)
        .collect()
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
            keyring: None,
            own_keys: None,
            server_keys: ServerKeys::Unknown,
            pending: false,
            converting: false,
            upload_keys: false,
            joining: false,
            limits,
            download_cursor: 0,
            upload_cursors: BTreeMap::new(),
            downloading: every_type(),
            known_kinds: every_kind(),
            known_permissions: every_permission(),
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

    pub fn encryption(&self) -> Encryption {
        match (&self.keyring, &self.server_keys) {
            (None, ServerKeys::Unknown) => Encryption::Checking,
            (None, ServerKeys::Missing) => Encryption::Off,
            (None, ServerKeys::Found(_)) => Encryption::Enter,
            (Some(keyring), ServerKeys::Found(keys)) if !self.pending && replaces(keys, keyring) => Encryption::Changed,
            (Some(_), _) => Encryption::Ready,
        }
    }

    /// The keyring records are sealed and opened with, while the device syncs.
    fn ready_keyring(&self) -> Option<&Keyring> {
        self.keyring.as_ref().filter(|_| self.encryption() == Encryption::Ready)
    }

    /// What a key record in a downloaded page means for this device, which syncs.
    fn verdict(&self, keys: &KeyRecord) -> Verdict {
        let keyring = self.keyring.as_ref().expect("the device syncs");
        if self.own_keys.as_ref() == Some(keys) {
            Verdict::Ours
        } else if self.pending {
            Verdict::SetElsewhere
        } else if replaces(keys, keyring) {
            Verdict::Replaced
        } else {
            Verdict::Stale
        }
    }

    /// Takes `keyring`, wrapped as `keys`, and syncs everything again with it.
    fn adopt(&mut self, keyring: Keyring, keys: KeyRecord) {
        self.keyring = Some(keyring);
        self.server_keys = ServerKeys::Found(keys.clone());
        self.own_keys = Some(keys);
        self.start_over();
    }

    /// Drops a keyring another device's key record replaced before this device's reached the
    /// key slot.
    fn drop_keys(&mut self, keys: KeyRecord) {
        self.keyring = None;
        self.own_keys = None;
        self.server_keys = ServerKeys::Found(keys);
        self.pending = false;
        self.converting = false;
        self.upload_keys = false;
        self.joining = false;
    }

    /// Everything goes up and comes down again from the start.
    fn start_over(&mut self) {
        self.upload_cursors.clear();
        self.download_cursor = 0;
        self.epoch = None;
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
        if let Some(sealed) = store.secret_state(KEYRING_KEY)? {
            account.keyring = Keyring::from_secret(&sealed);
            if account.keyring.is_none() {
                log::warn!("the saved sync keyring is unreadable, so this device asks for the passphrase again");
            }
        }
        Ok(Some(account))
    }

    /// The JSON goes first: stopped after it, a device whose keys just changed lacks the keyring
    /// its JSON names, and asks for the passphrase again, rather than syncing with a keyring its
    /// cursors were not started over for.
    pub fn save(&self, store: &mut SyncStore<'_>) -> Result<(), Error> {
        let bytes = serde_json::to_vec(self).expect("the account serializes");
        store.set_engine_state(STATE_KEY, &bytes)?;
        store.set_secret_state(SESSION_KEY, self.session.as_bytes())?;
        Ok(store.set_secret_state(KEYRING_KEY, &self.keyring.as_ref().map(Keyring::to_secret).unwrap_or_default())?)
    }

    /// Saves the account a sign-in just made, in place of any the profile stored, which is kept
    /// until then so that a sign-in that fails leaves it asking for another. Drops the tokens
    /// protocol 1 kept too, which nothing reads.
    pub fn save_signed_in(&self, store: &mut SyncStore<'_>) -> Result<(), Error> {
        store.set_secret_state(OLD_TOKENS_KEY, &[])?;
        self.save(store)
    }

    /// Signs the profile out, forgetting the account's keys, so that signing in again asks for the
    /// passphrase. The records stay on this device and on the server. Works with the system
    /// keyring locked: forgetting a sealed value needs no key.
    pub fn forget(store: &mut SyncStore<'_>) -> Result<(), Error> {
        store.set_engine_state(STATE_KEY, &[])?;
        store.set_secret_state(OLD_TOKENS_KEY, &[])?;
        store.set_secret_state(KEYRING_KEY, &[])?;
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

/// Setting, entering or changing the sync passphrase, whichever [`Account::encryption`] asks
/// for: setting one when the account has none, entering the account's when this device does not
/// sync with it, changing it when the device syncs. Built on the UI thread, run on a worker (it
/// derives a key with Argon2id, and touches no network), finished on the UI thread.
pub struct PassphraseJob {
    account: Account,
    step: Step,
    passphrase: Passphrase,
}

enum Step {
    Set,
    Enter(KeyRecord),
    Change(Keyring),
}

impl PassphraseJob {
    /// `None` while the device is still looking for the account's key record.
    pub fn new(account: Account, passphrase: Passphrase) -> Option<PassphraseJob> {
        let step = match (account.encryption(), &account.server_keys, &account.keyring) {
            (Encryption::Off, _, _) => Step::Set,
            (Encryption::Enter | Encryption::Changed, ServerKeys::Found(keys), _) => Step::Enter(keys.clone()),
            (Encryption::Ready, _, Some(keyring)) => Step::Change(keyring.clone()),
            _ => return None,
        };
        Some(PassphraseJob { account, step, passphrase })
    }

    /// Worker thread. Entering takes only a keyring that descends from the one the device holds,
    /// if it holds one: else whoever learned an old passphrase could offer a key of their own.
    pub fn run(self) -> NewKeys {
        let PassphraseJob { account, step, passphrase } = self;
        let result = match step {
            Step::Set => {
                let keyring = Keyring::new();
                let keys = keyring.wrap(&passphrase);
                Ok((keyring, keys))
            }
            Step::Enter(keys) => match keys.unwrap(&passphrase) {
                Ok(keyring) if account.keyring.as_ref().is_some_and(|held| !keyring.descends_from(held)) => Err(Error::UnrelatedKey),
                Ok(keyring) => Ok((keyring, keys)),
                Err(Unwrapped::WrongPassphrase) => Err(Error::WrongPassphrase),
                Err(Unwrapped::Invalid) => Err(Error::InvalidKeyRecord),
            },
            Step::Change(held) => {
                let keyring = held.next();
                let keys = keyring.wrap(&passphrase);
                Ok((keyring, keys))
            }
        };
        NewKeys { account, result }
    }
}

/// A passphrase checked or wrapped, to store on the UI thread.
pub struct NewKeys {
    /// As the job found it.
    account: Account,
    result: Result<(Keyring, KeyRecord), Error>,
}

impl NewKeys {
    /// Stores the new keys, and starts the sync over with them: everything this device holds goes
    /// up sealed with them, and everything comes down again. Refused when the profile signed out
    /// (or in again), or its keys changed, meanwhile. Returns the account as stored.
    pub fn finish(self, store: &mut SyncStore<'_>) -> Result<Account, Error> {
        let NewKeys { account, result } = self;
        let mut current = Account::load(store)?.filter(|c| c.sign_in == account.sign_in).ok_or(Error::SignedOut)?;
        if current.keyring != account.keyring || current.server_keys != account.server_keys {
            return Err(Error::KeysChanged);
        }
        let (keyring, keys) = result?;
        current.adopt(keyring, keys);
        match account.encryption() {
            // The key record goes up once the account's plaintext is sealed, so that what is
            // sealed is only what the server held before the account was encrypted.
            Encryption::Off => {
                current.pending = true;
                current.converting = true;
            }
            Encryption::Ready => current.upload_keys = !current.converting,
            Encryption::Enter | Encryption::Changed | Encryption::Checking => {
                current.pending = false;
                current.converting = false;
                current.upload_keys = false;
                current.joining = true;
            }
        }
        current.save(store)?;
        Ok(current)
    }
}

/// Local changes on their way to the server. Built on the UI thread, run on a worker.
pub struct Round {
    account: Account,
    /// Sealed on the worker.
    records: Vec<WireRecord>,
    /// The account's key record, when it goes up with them.
    keys: Option<Record>,
    upto: BTreeMap<u8, u64>,
    more_up: bool,
    types: BTreeSet<DataType>,
}

impl Round {
    /// Collects up to one batch of local changes of the `types` this device syncs, oldest first per
    /// kind; none while the device waits for the account's passphrase. A record larger than the
    /// server takes (sealed, when the account has a passphrase), or with an id it refuses, is left
    /// out and logged; nothing else could send it. The kinds of
    /// other types keep their cursors, so turning a type on uploads what changed while it was off.
    pub fn gather(store: &mut SyncStore<'_>, mut account: Account, types: &[DataType]) -> Result<Round, Error> {
        let types: BTreeSet<DataType> = types.iter().copied().collect();
        let kinds = every_kind();
        let permissions = every_permission();
        if !types.is_subset(&account.downloading)
            || !kinds.is_subset(&account.known_kinds)
            || !permissions.is_subset(&account.known_permissions)
        {
            account.download_cursor = 0;
        }
        account.downloading = types.clone();
        account.known_kinds = kinds;
        account.known_permissions = permissions;
        let mut records = Vec::new();
        let mut upto = BTreeMap::new();
        let mut more_up = false;
        let keys = match (&account.own_keys, account.ready_keyring()) {
            (Some(own), Some(_)) if account.upload_keys && !account.converting => Some(own.to_record()),
            _ => None,
        };
        let plain = account.encryption() == Encryption::Off;
        if !plain && (account.ready_keyring().is_none() || account.joining) {
            return Ok(Round { account, records, keys, upto, more_up, types });
        }
        let budget = account.limits.max_batch as usize;
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
                let max = account.limits.max_record_bytes as usize;
                if plain && (wire.id.is_empty() || wire.id.len() > MAX_ID_BYTES || wire.body.len() > max) {
                    log::warn!("not syncing {kind:?}: its {}-byte id or {}-byte body is over the server's limits", wire.id.len(), wire.body.len());
                    continue;
                }
                let sealed = Keyring::sealed_len(wire.id.len(), wire.body.len());
                if !plain && sealed > max {
                    log::warn!("not syncing {kind:?}: sealed, it is {sealed} bytes, over the server's limit");
                    continue;
                }
                records.push(wire);
            }
        }
        Ok(Round { account, records, keys, upto, more_up, types })
    }

    /// Whether this round uploads nothing.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty() && self.keys.is_none()
    }

    /// Worker thread: seals and uploads, then downloads one page and opens it. An upload the server
    /// refuses, as over the account's quota, still lets the download run, so the device keeps
    /// receiving the others' changes. One refused as too large first asks the server for its
    /// limits again, since they may have changed after sign-in. Nothing it does needs finishing,
    /// so the shell may stop waiting for it at any moment, as when the browser quits.
    ///
    /// An account without a passphrase downloads first, and uploads only once its download has
    /// reached the end with no key record: a device that set a passphrase meanwhile has its key
    /// record show first, and then nothing goes up unencrypted.
    pub fn run(self, http: &Http) -> Exchanged {
        let Round { mut account, records, keys, mut upto, more_up, types } = self;
        let sends_keys = keys.is_some();
        let mut downloaded = None;
        let outgoing: Vec<Record> = match account.ready_keyring() {
            Some(keyring) => keys.into_iter().chain(records.iter().map(|r| keyring.seal(r.kind.code(), &r.id, &r.body))).collect(),
            None if account.encryption() == Encryption::Off => {
                let page = match server::download(http, &account.server, &account.session, account.download_cursor, account.limits)
                    .and_then(|page| open_page(http, &account, page))
                {
                    Ok(page) => page,
                    Err(e) => return Exchanged { account, upto, more_up, types, refused: None, sent_keys: false, result: Err(e) },
                };
                let clear = page.keys.is_none() && !page.more;
                downloaded = Some(page);
                if clear {
                    records.into_iter().map(|r| Record { kind: r.kind.code(), id: r.id, body: r.body }).collect()
                } else {
                    upto.clear();
                    Vec::new()
                }
            }
            None => Vec::new(),
        };
        // Downloaded first, the page lacks what goes up now, so another round brings it, as one
        // that uploads first does, and the cursor moves past it: else a server that went back to
        // an older copy of the account could go unseen.
        let mut more_up = more_up || (downloaded.is_some() && !outgoing.is_empty());
        let refused = match upload(http, &account, outgoing) {
            Ok(()) => None,
            // 409: the server went back to an older copy of the account, and `finish` starts over.
            Err(e @ Error::Server { status, .. }) if status != 409 => Some(e),
            Err(e) => return Exchanged { account, upto, more_up, types, refused: None, sent_keys: false, result: Err(e) },
        };
        if let Some(e) = &refused {
            log::warn!("the sync server refused this device's changes: {e}");
            // The same records would be refused again, unless the limits they were gathered under
            // have changed.
            more_up = matches!(e, Error::Server { status: 413, .. }) && relearn_limits(http, &mut account);
        }
        let result = match downloaded {
            Some(page) => Ok(page),
            None => server::download(http, &account.server, &account.session, account.download_cursor, account.limits)
                .and_then(|page| open_page(http, &account, page)),
        };
        Exchanged { account, upto, more_up, types, sent_keys: sends_keys && refused.is_none(), refused, result }
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

/// A downloaded page, opened on the worker.
struct Downloaded {
    /// The records that opened, oldest write first.
    records: Vec<Opened>,
    /// The page's key record.
    keys: Option<KeyRecord>,
    cursor: u64,
    more: bool,
    epoch: u64,
}

/// Worker thread: notes the page's key record and opens the records sealed with the account's
/// key, or takes the plaintext ones while the account has no passphrase. While converting it also
/// seals a copy of each plaintext record, and until the copies are stored the round fails, so the
/// page comes again and none is passed. A device waiting for the account's passphrase opens
/// nothing, nor does one the page's key record takes the keys from: nothing in the page counts
/// until the passphrase is entered.
fn open_page(http: &Http, account: &Account, page: Page) -> Result<Downloaded, Error> {
    let Page { records, cursor, more, epoch } = page;
    let mut downloaded = Downloaded { records: Vec::new(), keys: None, cursor, more, epoch };
    let mut sealed = Vec::new();
    let mut plain = Vec::new();
    for record in records {
        match record.kind {
            KEYS_KIND => match KeyRecord::parse(&record.body).filter(|_| record.id == KEYS_ID) {
                Some(keys) => downloaded.keys = Some(keys),
                None => log::warn!("the sync server sent a key record this version cannot read"),
            },
            SEALED_KIND => sealed.push(record),
            _ if record.body.is_empty() => {}
            _ => plain.push(record),
        }
    }
    if account.encryption() == Encryption::Off {
        downloaded.records = plain.into_iter().map(|r| Opened { kind: r.kind, id: r.id, body: r.body }).collect();
        return Ok(downloaded);
    }
    let Some(keyring) = account.ready_keyring() else {
        return Ok(downloaded);
    };
    if downloaded.keys.as_ref().is_some_and(|keys| matches!(account.verdict(keys), Verdict::SetElsewhere | Verdict::Replaced)) {
        return Ok(downloaded);
    }
    for record in &sealed {
        match keyring.open(record) {
            Ok(opened) => downloaded.records.push(opened),
            Err(Unopened::Stale) => log::info!("skipping a sync record sealed with a replaced key"),
            Err(Unopened::Foreign) => log::warn!("skipping a sync record sealed with a key this device does not have"),
            Err(Unopened::Damaged) => log::warn!("skipping a sync record that was altered or is damaged"),
        }
    }
    if !account.converting {
        if !plain.is_empty() {
            log::info!("skipping {} unencrypted sync records a device without the passphrase, or the server, wrote", plain.len());
        }
        return Ok(downloaded);
    }
    let max = account.limits.max_record_bytes as usize;
    let copies = plain
        .iter()
        .filter(|r| {
            let fits = Keyring::sealed_len(r.id.len(), r.body.len()) <= max;
            if !fits {
                log::warn!("not encrypting an unencrypted sync record of kind {}: sealed, it is over the server's limit", r.kind);
            }
            fits
        })
        .map(|r| keyring.seal(r.kind, &r.id, &r.body))
        .collect();
    upload(http, account, copies)?;
    Ok(downloaded)
}

/// Whether `keys` is the account's key record in place of `keyring`'s. A record older than
/// `keyring`'s is not: a server that went back to a backup, or a malicious one, holds it.
fn replaces(keys: &KeyRecord, keyring: &Keyring) -> bool {
    !keys.is_of(keyring) && keys.generation() >= keyring.generation()
}

/// A round back from the network, to apply on the UI thread.
pub struct Exchanged {
    account: Account,
    upto: BTreeMap<u8, u64>,
    more_up: bool,
    types: BTreeSet<DataType>,
    refused: Option<Error>,
    /// The key record went up.
    sent_keys: bool,
    result: Result<Downloaded, Error>,
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

impl Synced {
    /// What the sync comes to after this round, its `round`th (counting from 1): `None` while it
    /// goes on, as more waits on either side and fewer than [`MAX_ROUNDS`] ran; else the upload
    /// the server refused, which waits for a later sync, or `Ok`.
    pub fn outcome(self, round: usize) -> Option<Result<(), Error>> {
        if self.again {
            if round < MAX_ROUNDS {
                return None;
            }
            log::info!("sync: {MAX_ROUNDS} rounds; the next sync goes on");
        }
        Some(self.refused.map_or(Ok(()), Err))
    }
}

impl Exchanged {
    /// Applies the downloaded page, moves the cursors and saves the account. When the profile
    /// signed out (or in again), or took other keys, while the round ran, it changes nothing and
    /// says [`Error::SignedOut`].
    pub fn finish(self, store: &mut SyncStore<'_>) -> Finished {
        let Exchanged { mut account, upto, more_up, types, refused, sent_keys, result } = self;
        match Account::load(store) {
            Ok(Some(current)) if current.sign_in == account.sign_in && current.keyring == account.keyring => {}
            Ok(_) => return Finished { account, result: Err(Error::SignedOut) },
            Err(e) => return Finished { account, result: Err(e) },
        }
        let went_back = match &result {
            Ok(page) => account.epoch.is_some_and(|epoch| epoch != page.epoch),
            Err(e) => matches!(e, Error::Server { status: 409, .. }),
        };
        let mut result = if went_back {
            // The server went back to an older copy of the account, as to a backup: everything
            // goes up and comes down again, which changes nothing this device already has. The
            // copy may lack the key record too.
            log::warn!("the sync server lost some of what this device synced with it; syncing everything again");
            account.start_over();
            account.upload_keys = account.ready_keyring().is_some() && !account.converting;
            Ok(Synced { report: ApplyReport::default(), again: true, refused: None })
        } else {
            result.and_then(|page| match account.encryption() {
                Encryption::Ready => {
                    if refused.is_none() {
                        account.upload_cursors.extend(upto);
                        account.upload_keys &= !sent_keys;
                    }
                    sync(store, &mut account, page, &types, more_up, refused)
                }
                Encryption::Off => {
                    if refused.is_none() {
                        account.upload_cursors.extend(upto);
                    }
                    unencrypted(store, &mut account, page, &types, more_up, refused)
                }
                Encryption::Checking | Encryption::Enter | Encryption::Changed => Ok(look(&mut account, page)),
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

/// A page while the device does not sync: only the key record counts. A device whose key another
/// device replaced syncs again when its own record comes back, and takes any record that
/// replaces its key as the one to ask for, since entering it checks that it descends from its
/// own. A device without keys takes only a record at least as new as the last one it saw, so a
/// server cannot offer an older passphrase's record after a newer one.
fn look(account: &mut Account, page: Downloaded) -> Synced {
    let newest = match (page.keys, &account.server_keys, &account.keyring) {
        (Some(keys), _, _) if account.own_keys.as_ref() == Some(&keys) => Some(keys),
        (Some(keys), _, Some(keyring)) => Some(keys).filter(|keys| replaces(keys, keyring)),
        (Some(keys), ServerKeys::Found(seen), None) => Some(keys).filter(|keys| keys.generation() >= seen.generation()),
        (Some(keys), _, None) => Some(keys),
        (None, _, _) => None,
    };
    if let Some(keys) = newest {
        account.server_keys = ServerKeys::Found(keys);
    }
    account.download_cursor = page.cursor;
    account.epoch = Some(page.epoch);
    if account.server_keys == ServerKeys::Unknown && !page.more {
        // No key record anywhere: the account syncs unencrypted, from the start, since the look
        // applied nothing.
        account.server_keys = ServerKeys::Missing;
        account.download_cursor = 0;
        account.epoch = None;
        return Synced { report: ApplyReport::default(), again: true, refused: None };
    }
    Synced { report: ApplyReport::default(), again: page.more, refused: None }
}

/// A page while the account has no passphrase. A key record in it means another device set one:
/// nothing of the page counts, and nothing more goes up, until the passphrase is entered here.
fn unencrypted(
    store: &mut SyncStore<'_>,
    account: &mut Account,
    page: Downloaded,
    types: &BTreeSet<DataType>,
    more_up: bool,
    refused: Option<Error>,
) -> Result<Synced, Error> {
    if let Some(keys) = page.keys {
        log::warn!("the sync account was encrypted on another device; syncing stops until its passphrase is entered here");
        account.server_keys = ServerKeys::Found(keys);
        return Ok(Synced { report: ApplyReport::default(), again: false, refused });
    }
    account.epoch = Some(page.epoch);
    let more_down = page.more;
    let report = apply(store, account, page, types)?;
    let again = more_up || more_down || report.merged > 0;
    Ok(Synced { report, again, refused })
}

/// A page while the device syncs. One whose key record takes this device's keys from it stops the
/// sync where it is, until the passphrase is entered.
fn sync(
    store: &mut SyncStore<'_>,
    account: &mut Account,
    page: Downloaded,
    types: &BTreeSet<DataType>,
    more_up: bool,
    refused: Option<Error>,
) -> Result<Synced, Error> {
    let stopped = Synced { report: ApplyReport::default(), again: false, refused: None };
    if let Some(keys) = &page.keys {
        match account.verdict(keys) {
            Verdict::Ours => account.pending = false,
            Verdict::SetElsewhere => {
                log::warn!("another device set a sync passphrase first; syncing stops until that one is entered here");
                account.drop_keys(keys.clone());
                return Ok(Synced { refused, ..stopped });
            }
            Verdict::Replaced => {
                log::warn!("the sync passphrase was changed on another device; syncing stops until it is entered here");
                account.server_keys = ServerKeys::Found(keys.clone());
                return Ok(Synced { refused, ..stopped });
            }
            Verdict::Stale => {
                log::warn!("the sync server holds an older or altered key record; sending this device's again");
                account.upload_keys = true;
            }
        }
    }
    account.epoch = Some(page.epoch);
    let more_down = page.more;
    let report = apply(store, account, page, types)?;
    if account.converting && !more_down {
        account.converting = false;
        account.upload_keys = true;
    }
    let joined = account.joining && !more_down;
    account.joining &= more_down;
    let again = more_up || more_down || report.merged > 0 || account.upload_keys || joined;
    Ok(Synced { report, again, refused })
}

/// Applies the page's records of the `types` this device syncs. The cursor passes the others, and
/// records of kinds this build does not know, and core rejects records of site permissions it does
/// not know; turning one of those types on later, or updating to a build that knows the kind or
/// the permission, starts the download over (see [`Round::gather`]).
fn apply(store: &mut SyncStore<'_>, account: &mut Account, page: Downloaded, types: &BTreeSet<DataType>) -> Result<ApplyReport, Error> {
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

    /// `account` with keys of its own, as after setting a passphrase on a device that synced
    /// nothing before and seeing its key record come back.
    fn syncing(mut account: Account) -> Account {
        let keyring = Keyring::new();
        let keys = keyring.wrap(&Passphrase::new("correct horse".to_owned()).unwrap());
        account.adopt(keyring, keys);
        account
    }

    #[test]
    fn a_sign_in_drops_the_tokens_protocol_1_kept() {
        use vsesvit_core::vault::KeyStore;
        use vsesvit_core::{OpenOptions, Profile};

        let dir = std::env::temp_dir().join(format!("vsesvit-sync-old-tokens-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut profile = Profile::open(&dir, OpenOptions { key_store: KeyStore::Basic, ..OpenOptions::default() }).unwrap();
        let mut store = profile.sync();
        store.set_secret_state(OLD_TOKENS_KEY, b"protocol 1 tokens").unwrap();
        let limits = Limits { max_batch: 100, max_record_bytes: 1 << 20, max_request_bytes: 4 << 20 };
        let account = Account::new("https://sync.example.com".to_owned(), None, "session".to_owned(), limits);
        account.save_signed_in(&mut store).unwrap();
        let old = store.secret_state(OLD_TOKENS_KEY).unwrap().filter(|t| !t.is_empty());
        let loaded = Account::load(&mut store).unwrap();
        drop(profile);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(old, None, "the old tokens are gone");
        assert_eq!(loaded, Some(account));
    }

    #[test]
    fn a_sync_that_runs_out_of_rounds_comes_to_the_refusal_still_waiting() {
        let refused = || Some(Error::Server { status: 507, message: "full".to_owned() });
        let round = |again, refused| Synced { report: ApplyReport::default(), again, refused };
        assert!(round(true, refused()).outcome(1).is_none(), "more waits");
        assert!(matches!(round(false, None).outcome(1), Some(Ok(()))));
        assert!(matches!(round(false, refused()).outcome(1), Some(Err(Error::Server { status: 507, .. }))));
        assert!(matches!(round(true, None).outcome(MAX_ROUNDS), Some(Ok(()))));
        let last = round(true, refused()).outcome(MAX_ROUNDS);
        assert!(matches!(last, Some(Err(Error::Server { status: 507, .. }))), "{last:?}");
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
    fn a_record_the_server_would_refuse_once_sealed_is_left_out_and_passed() {
        use vsesvit_core::history::Transition;
        use vsesvit_core::vault::KeyStore;
        use vsesvit_core::{OpenOptions, Profile, Url};

        let dir = std::env::temp_dir().join(format!("vsesvit-sync-long-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut profile = Profile::open(&dir, OpenOptions { key_store: KeyStore::Basic, ..OpenOptions::default() }).unwrap();
        let long = Url::parse(&format!("https://example.com/?q={}", "a".repeat(9000))).unwrap();
        profile.history().record_visit(&Url::parse("https://example.com/short").unwrap(), Transition::Link).unwrap();
        profile.history().record_visit(&long, Transition::Link).unwrap();

        let limits = Limits { max_batch: 100, max_record_bytes: 8000, max_request_bytes: 1 << 20 };
        let account = syncing(Account::new("https://sync.example.com".to_owned(), None, "session".to_owned(), limits));
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
        let every_code: Vec<u8> = every_kind().into_iter().collect();
        let mut gather = |account: Account| {
            let round = Round::gather(&mut store, account, &DataType::ALL).unwrap();
            let known = serde_json::to_value(&round.account).unwrap()["known_kinds"].clone();
            (round.account.download_cursor, serde_json::from_value::<Option<Vec<u8>>>(known).unwrap())
        };

        // Saved by a build that knew every kind but SitePermissions: the records of it passed then
        // were skipped.
        assert_eq!(gather(saved_account(Some(&[1, 2, 3, 4, 5, 6, 7, 8]))), (0, Some(every_code.clone())));
        assert_eq!(gather(saved_account(Some(&every_code))), (100, Some(every_code.clone())), "no new kind, no new start");
        // Saved before the account kept the list, by a build that knew the kinds there were then,
        // and no key record.
        assert_eq!(gather(saved_account(None)), (0, Some(every_code.clone())));
        let before_encryption: Vec<u8> = Kind::ALL.iter().map(|k| k.code()).collect();
        assert_eq!(gather(saved_account(Some(&before_encryption))), (0, Some(every_code.clone())), "a look for the key record covers the whole account");
        drop(profile);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_build_that_knows_a_new_site_permission_downloads_everything_again() {
        use vsesvit_core::vault::KeyStore;
        use vsesvit_core::{OpenOptions, Profile};

        let dir = std::env::temp_dir().join(format!("vsesvit-sync-new-permission-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut profile = Profile::open(&dir, OpenOptions { key_store: KeyStore::Basic, ..OpenOptions::default() }).unwrap();
        let mut store = profile.sync();
        let every_name: Vec<&str> = Permission::ALL.iter().map(|p| p.key()).collect();
        let mut gather = |known: Option<&[&str]>| {
            let mut account = serde_json::to_value(saved_account(None)).unwrap();
            account["known_kinds"] = serde_json::to_value(every_kind()).unwrap();
            match known {
                Some(names) => account["known_permissions"] = names.into(),
                None => drop(account.as_object_mut().unwrap().remove("known_permissions")),
            }
            let round = Round::gather(&mut store, serde_json::from_value(account).unwrap(), &DataType::ALL).unwrap();
            let known = serde_json::to_value(&round.account).unwrap()["known_permissions"].clone();
            (round.account.download_cursor, serde_json::from_value::<Option<Vec<String>>>(known).unwrap())
        };
        let every = Some(every_permission().into_iter().collect::<Vec<_>>());

        // Saved before the account kept the list, by a build without picture-in-picture: the
        // device rejected the records of it that the cursor passed then.
        assert_eq!(gather(None), (0, every.clone()));
        let without_pip: Vec<&str> = every_name.iter().copied().filter(|n| *n != "picture_in_picture").collect();
        assert_eq!(gather(Some(&without_pip)), (0, every.clone()));
        assert_eq!(gather(Some(&every_name)), (100, every.clone()), "no new permission, no new start");
        drop(profile);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn pass(text: &str) -> Passphrase {
        Passphrase::new(text.to_owned()).unwrap()
    }

    fn new_account() -> Account {
        let limits = Limits { max_batch: 100, max_record_bytes: 1 << 20, max_request_bytes: 4 << 20 };
        Account::new("https://sync.example.com".to_owned(), None, "session".to_owned(), limits)
    }

    fn page(keys: Option<&KeyRecord>, more: bool) -> Downloaded {
        Downloaded { records: Vec::new(), keys: keys.cloned(), cursor: 7, more, epoch: 0 }
    }

    /// A profile that is removed when dropped.
    struct Scratch(std::path::PathBuf, Option<vsesvit_core::Profile>);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            use vsesvit_core::vault::KeyStore;
            use vsesvit_core::{OpenOptions, Profile};
            let dir = std::env::temp_dir().join(format!("vsesvit-sync-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let profile = Profile::open(&dir, OpenOptions { key_store: KeyStore::Basic, ..OpenOptions::default() }).unwrap();
            Scratch(dir, Some(profile))
        }

        fn store(&mut self) -> SyncStore<'_> {
            self.1.as_mut().unwrap().sync()
        }

        /// Saves `account`, runs a passphrase job on it as the shells do, and returns what it
        /// stored.
        fn passphrase(&mut self, account: &Account, text: &str) -> Result<Account, Error> {
            account.save(&mut self.store()).unwrap();
            let job = PassphraseJob::new(Account::load(&mut self.store()).unwrap().unwrap(), pass(text)).expect("a step to take");
            job.run().finish(&mut self.store())
        }

        fn sync(&mut self, account: &mut Account, page: Downloaded) -> Synced {
            sync(&mut self.store(), account, page, &every_type(), false, None).unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(self.1.take());
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An account that looked through the server and found no key record.
    fn looked(mut account: Account) -> Account {
        look(&mut account, page(None, false));
        account
    }

    #[test]
    fn a_device_looks_for_the_key_record_then_syncs_unencrypted_or_asks_for_the_passphrase() {
        let mut account = new_account();
        assert_eq!(account.encryption(), Encryption::Checking);
        assert!(PassphraseJob::new(account.clone(), pass("correct horse")).is_none(), "nothing to set or enter yet");
        look(&mut account, page(None, true));
        assert_eq!(account.encryption(), Encryption::Checking, "the look goes on");
        assert_eq!(account.download_cursor, 7);
        assert!(look(&mut account, page(None, false)).again, "the unencrypted sync starts at once");
        assert_eq!(account.encryption(), Encryption::Off);
        assert_eq!(account.download_cursor, 0, "from the start, since the look applied nothing");

        let mut account = new_account();
        let keys = Keyring::new().wrap(&pass("correct horse"));
        look(&mut account, page(Some(&keys), true));
        assert_eq!(account.encryption(), Encryption::Enter);
        look(&mut account, page(None, false));
        assert_eq!(account.encryption(), Encryption::Enter, "found once, it stays found");
        assert_eq!(account.download_cursor, 7);
    }

    #[test]
    fn an_unencrypted_account_stops_at_a_key_record_and_applies_nothing_of_its_page() {
        let mut scratch = Scratch::new("unencrypted-stops");
        let mut account = looked(new_account());
        let keys = Keyring::new().wrap(&pass("correct horse"));
        let mut with_keys = page(Some(&keys), false);
        with_keys.records.push(Opened { kind: Kind::Prefs.code(), id: "x".to_owned(), body: b"{}".to_vec() });
        let stopped = unencrypted(&mut scratch.store(), &mut account, with_keys, &every_type(), true, None).unwrap();
        assert!(!stopped.again && stopped.report.merged == 0 && stopped.report.rejected.is_empty());
        assert_eq!(account.encryption(), Encryption::Enter);
        assert_eq!(account.download_cursor, 0, "the page is not passed");
        let round = Round::gather(&mut scratch.store(), account, &DataType::ALL).unwrap();
        assert!(round.is_empty(), "nothing goes up");
    }

    #[test]
    fn a_look_ignores_a_key_record_older_than_one_it_saw() {
        let keyring = Keyring::new();
        let (older, newer) = (keyring.wrap(&pass("correct horse")), keyring.next().wrap(&pass("battery staple")));
        let mut account = new_account();
        look(&mut account, page(Some(&newer), true));
        look(&mut account, page(Some(&older), false));
        assert_eq!(account.server_keys, ServerKeys::Found(newer));
    }

    #[test]
    fn setting_a_passphrase_starts_over_and_sends_the_key_record_once_it_has_sealed_the_plaintext() {
        let mut scratch = Scratch::new("set");
        let mut account = looked(new_account());
        account.upload_cursors.insert(1, 40);
        let mut set = scratch.passphrase(&account, "correct horse").unwrap();
        assert_eq!(set.encryption(), Encryption::Ready);
        assert!(set.pending && set.converting && !set.upload_keys, "it seals the account's plaintext first");
        assert_eq!((set.download_cursor, set.upload_cursors.len()), (0, 0), "everything goes up and comes down again");
        assert_eq!(Account::load(&mut scratch.store()).unwrap(), Some(set.clone()), "the keyring is stored");
        assert!(scratch.sync(&mut set, page(None, true)).again);
        assert!(set.converting && !set.upload_keys);
        assert!(scratch.sync(&mut set, page(None, false)).again, "the key record goes up next");
        assert!(!set.converting && set.upload_keys);
        let own = set.own_keys.clone().unwrap();
        scratch.sync(&mut set, page(Some(&own), false));
        assert!(!set.pending, "its key record reached the key slot");
    }

    #[test]
    fn a_device_takes_another_devices_passphrase_if_that_reached_the_key_slot_first() {
        let mut scratch = Scratch::new("set-elsewhere");
        let mut account = scratch.passphrase(&looked(new_account()), "correct horse").unwrap();
        let theirs = Keyring::new().wrap(&pass("battery staple"));
        let stopped = scratch.sync(&mut account, page(Some(&theirs), true));
        assert!(!stopped.again);
        assert_eq!(account.encryption(), Encryption::Enter);
        assert_eq!(account.download_cursor, 0, "nothing of the page counts");
        let entered = scratch.passphrase(&account, "battery staple").unwrap();
        assert_eq!(entered.encryption(), Encryption::Ready);
        assert!(!entered.pending && !entered.upload_keys);
        assert!(entered.joining, "it downloads everything before it uploads");
    }

    #[test]
    fn a_passphrase_changed_elsewhere_stops_the_sync_until_entered_and_only_a_descendant_is_taken() {
        let mut scratch = Scratch::new("changed");
        let mut account = syncing(looked(new_account()));
        let changed = account.keyring.as_ref().unwrap().next().wrap(&pass("battery staple"));
        assert!(!scratch.sync(&mut account, page(Some(&changed), true)).again);
        assert_eq!(account.encryption(), Encryption::Changed);
        assert_eq!(account.download_cursor, 0, "nothing of the page counts");
        let round = Round::gather(&mut scratch.store(), account.clone(), &DataType::ALL).unwrap();
        assert!(round.is_empty(), "nothing goes up with the old key");

        assert!(matches!(scratch.passphrase(&account, "correct horse"), Err(Error::WrongPassphrase)));
        let forged = Keyring::new().next().next().wrap(&pass("evil horse"));
        look(&mut account, page(Some(&forged), false));
        assert_eq!(account.encryption(), Encryption::Changed);
        assert!(matches!(scratch.passphrase(&account, "evil horse"), Err(Error::UnrelatedKey)), "a key that does not descend from this device's");

        look(&mut account, page(Some(&changed), false));
        let entered = scratch.passphrase(&account, "battery staple").unwrap();
        assert_eq!(entered.encryption(), Encryption::Ready);
        assert_eq!(entered.keyring.as_ref().unwrap().generation(), 1);
    }

    #[test]
    fn a_device_whose_own_key_record_comes_back_syncs_again() {
        let mut scratch = Scratch::new("comes-back");
        let mut account = syncing(looked(new_account()));
        let own = account.own_keys.clone().unwrap();
        let other = Keyring::new().wrap(&pass("battery staple"));
        scratch.sync(&mut account, page(Some(&other), false));
        assert_eq!(account.encryption(), Encryption::Changed);
        look(&mut account, page(Some(&own), false));
        assert_eq!(account.encryption(), Encryption::Ready);
    }

    #[test]
    fn an_older_or_altered_key_record_is_put_back() {
        let mut scratch = Scratch::new("put-back");
        let account = syncing(looked(new_account()));
        let first = account.own_keys.clone().unwrap();
        let mut changed = scratch.passphrase(&account, "battery staple").unwrap();
        assert!(changed.upload_keys, "a new key record goes up");
        changed.upload_keys = false;
        scratch.sync(&mut changed, page(Some(&first), false));
        assert_eq!(changed.encryption(), Encryption::Ready);
        assert!(changed.upload_keys, "the older record is replaced again");

        changed.upload_keys = false;
        let mut altered = serde_json::to_value(changed.own_keys.as_ref().unwrap()).unwrap();
        altered["nonce"] = serde_json::to_value(&first).unwrap()["nonce"].clone();
        let altered: KeyRecord = serde_json::from_value(altered).unwrap();
        scratch.sync(&mut changed, page(Some(&altered), false));
        assert_eq!(changed.encryption(), Encryption::Ready);
        assert!(changed.upload_keys);
    }

    #[test]
    fn a_round_that_outlives_a_change_of_keys_saves_nothing() {
        let mut scratch = Scratch::new("outlived");
        let account = syncing(looked(new_account()));
        account.save(&mut scratch.store()).unwrap();
        let round = Round::gather(&mut scratch.store(), account.clone(), &DataType::ALL).unwrap();
        let changed = scratch.passphrase(&account, "battery staple").unwrap();
        let exchanged = Exchanged {
            account: round.account,
            upto: round.upto,
            more_up: false,
            types: round.types,
            refused: None,
            sent_keys: false,
            result: Ok(page(None, false)),
        };
        assert!(matches!(exchanged.finish(&mut scratch.store()).result, Err(Error::SignedOut)));
        assert_eq!(Account::load(&mut scratch.store()).unwrap(), Some(changed));
    }

    #[test]
    fn a_passphrase_job_that_outlives_a_change_of_keys_stores_nothing() {
        let mut scratch = Scratch::new("job-outlived");
        let account = looked(new_account());
        account.save(&mut scratch.store()).unwrap();
        let job = PassphraseJob::new(account.clone(), pass("correct horse")).unwrap().run();
        let theirs = Keyring::new().wrap(&pass("battery staple"));
        let mut moved = account;
        look(&mut moved, page(Some(&theirs), false));
        moved.save(&mut scratch.store()).unwrap();
        assert!(matches!(job.finish(&mut scratch.store()), Err(Error::KeysChanged)));
        assert_eq!(Account::load(&mut scratch.store()).unwrap(), Some(moved));
    }
}
