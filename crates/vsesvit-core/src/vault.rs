//! The profile vault: one random 256-bit key per profile, protected by the OS, seals the
//! profile's secrets (sync tokens), as Chrome's os_crypt does.
//!
//! | where                                   | the key                                                  |
//! |-----------------------------------------|----------------------------------------------------------|
//! | Windows                                 | `vault_key`, wrapped by DPAPI for the current user        |
//! | Linux                                   | a Secret Service item, "Vsesvit Safe Storage"             |
//! | Linux with no Secret Service, elsewhere | `vault_key` in the clear, with a warning (Chrome's "basic") |
//!
//! `vault_key.protection` records where the key lives. A key in a Secret Service that
//! cannot be reached is an error, never a fresh key: a fresh key could open nothing sealed
//! before. A key made in the clear because no Secret Service was reachable moves into one
//! once it is reachable and unlocked; it stays the same key, so nothing is sealed again.
//!
//! Sealed form: `[version 1][12-byte random nonce][ChaCha20-Poly1305 ciphertext + tag]`.
//! Callers pass associated data naming where the value is stored, so a sealed value copied
//! into another row does not open.

use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use rusqlite::{Connection, OptionalExtension, params};

use crate::Error;
use crate::crdt::DeviceId;

pub(crate) const SCHEMA: &str = "
CREATE TABLE vault_key (                 -- LOCAL, at most one row
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  protection  TEXT NOT NULL CHECK (protection IN ('dpapi', 'secret_service', 'plain')),
  key         BLOB                       -- DPAPI blob | NULL (the key is in the Secret Service) | the key
);
";

const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Where a profile that has no key yet keeps the one it makes. An existing key stays where
/// it is.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum KeyStore {
    /// DPAPI on Windows. The Secret Service on Linux, or [`KeyStore::Basic`] when none is
    /// reachable.
    #[default]
    System,
    /// In the profile database, unprotected, like Chrome's `--password-store=basic`. Tests use
    /// it so they never reach the user's keyring.
    Basic,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("the system keyring is locked, so this profile's protected key could not be read; unlock the keyring and try again")]
    KeyringLocked,
    #[error("this profile's key is in the system keyring, which is not reachable: {0}")]
    KeyringUnreachable(String),
    #[error("the system keyring failed: {0}")]
    Keyring(String),
    #[error("the protected key could not be read: {0}")]
    KeyUnreadable(String),
    #[error("a protected value could not be decrypted: it was sealed with another key, or it was altered")]
    Damaged,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct Key([u8; KEY_LEN]);

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(..)")
    }
}

impl Key {
    fn random() -> Key {
        let mut k = [0; KEY_LEN];
        getrandom::fill(&mut k).expect("the OS random source works");
        Key(k)
    }

    fn from_slice(bytes: &[u8]) -> Result<Key, VaultError> {
        <[u8; KEY_LEN]>::try_from(bytes)
            .map(Key)
            .map_err(|_| VaultError::KeyUnreadable(format!("it is {} bytes, not {KEY_LEN}", bytes.len())))
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(&self.0.into())
    }
}

pub(crate) fn seal(key: &Key, aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let mut nonce = [0; NONCE_LEN];
    getrandom::fill(&mut nonce).expect("the OS random source works");
    let sealed = key.cipher().encrypt(&nonce.into(), Payload { msg: plaintext, aad }).expect("sealing in memory cannot fail");
    [&[VERSION][..], &nonce, &sealed].concat()
}

pub(crate) fn open(key: &Key, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, VaultError> {
    let [VERSION, rest @ ..] = sealed else {
        return Err(VaultError::Damaged);
    };
    let Some((nonce, ciphertext)) = rest.split_first_chunk::<NONCE_LEN>() else {
        return Err(VaultError::Damaged);
    };
    key.cipher().decrypt(&(*nonce).into(), Payload { msg: ciphertext, aad }).map_err(|_| VaultError::Damaged)
}

/// The profile's key: read from where `vault_key` says it lives, or made and recorded there.
pub(crate) fn load_or_create(conn: &Connection, device: DeviceId, store: KeyStore) -> Result<Key, Error> {
    #[cfg(target_os = "linux")]
    let mut keyring = secret_service_keyring::SecretServiceKeyring;
    #[cfg(not(target_os = "linux"))]
    let mut keyring = NoKeyring;
    resolve(conn, device, store, &mut keyring)
}

/// The Secret Service as the vault uses it. A trait so the choice between it and the
/// fallback is tested without a running service. Windows only ever reads through it: its
/// keys are made with DPAPI.
#[cfg_attr(windows, allow(dead_code))]
pub(crate) trait Keyring {
    fn find(&mut self, device: DeviceId, unlock: Unlock) -> Result<Option<Vec<u8>>, KeyringError>;
    fn store(&mut self, device: DeviceId, key: &Key, unlock: Unlock) -> Result<(), KeyringError>;
}

/// What a locked item or collection does.
#[cfg_attr(windows, allow(dead_code))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Unlock {
    /// Ask the person, through the keyring's own prompt.
    Prompt,
    /// Fail with [`VaultError::KeyringLocked`] instead.
    Never,
}

#[cfg_attr(windows, allow(dead_code))]
pub(crate) enum KeyringError {
    /// No service to ask: no session bus, or no provider on it.
    Unreachable(String),
    /// The service answered and refused, or failed.
    Failed(VaultError),
}

#[cfg(not(target_os = "linux"))]
struct NoKeyring;

#[cfg(not(target_os = "linux"))]
impl Keyring for NoKeyring {
    fn find(&mut self, _: DeviceId, _: Unlock) -> Result<Option<Vec<u8>>, KeyringError> {
        Err(KeyringError::Unreachable("this system has no Secret Service".to_owned()))
    }

    fn store(&mut self, _: DeviceId, _: &Key, _: Unlock) -> Result<(), KeyringError> {
        Err(KeyringError::Unreachable("this system has no Secret Service".to_owned()))
    }
}

/// A `vault_key` row.
enum Stored {
    Dpapi(Vec<u8>),
    SecretService,
    Plain(Key),
}

fn read(conn: &Connection) -> Result<Option<Stored>, Error> {
    let row: Option<(String, Option<Vec<u8>>)> =
        conn.query_row("SELECT protection, key FROM vault_key WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((protection, blob)) = row else {
        return Ok(None);
    };
    Ok(Some(match (protection.as_str(), blob) {
        ("dpapi", Some(b)) => Stored::Dpapi(b),
        ("secret_service", _) => Stored::SecretService,
        ("plain", Some(b)) => Stored::Plain(Key::from_slice(&b)?),
        (other, _) => return Err(VaultError::KeyUnreadable(format!("its record ({other}) is incomplete")).into()),
    }))
}

fn write(conn: &Connection, stored: &Stored) -> Result<(), Error> {
    let (protection, blob) = match stored {
        Stored::Dpapi(b) => ("dpapi", Some(&b[..])),
        Stored::SecretService => ("secret_service", None),
        Stored::Plain(key) => ("plain", Some(&key.0[..])),
    };
    conn.execute("INSERT OR REPLACE INTO vault_key (id, protection, key) VALUES (1, ?1, ?2)", params![protection, blob])?;
    Ok(())
}

fn resolve(conn: &Connection, device: DeviceId, store: KeyStore, keyring: &mut impl Keyring) -> Result<Key, Error> {
    let Some(stored) = read(conn)? else {
        let (key, stored) = match store {
            KeyStore::Basic => {
                let key = Key::random();
                (key, Stored::Plain(key))
            }
            KeyStore::System => create_protected(device, keyring)?,
        };
        write(conn, &stored)?;
        return Ok(key);
    };
    Ok(match stored {
        Stored::Plain(key) => {
            #[cfg(not(windows))]
            if store == KeyStore::System {
                promote(conn, device, key, keyring)?;
            }
            key
        }
        Stored::Dpapi(blob) => unprotect_dpapi(&blob)?,
        Stored::SecretService => match keyring.find(device, Unlock::Prompt) {
            Ok(Some(b)) => Key::from_slice(&b)?,
            Ok(None) => {
                return Err(VaultError::KeyUnreadable(
                    "the system keyring has no \"Vsesvit Safe Storage\" item for this profile".to_owned(),
                )
                .into());
            }
            Err(KeyringError::Unreachable(why)) => return Err(VaultError::KeyringUnreachable(why).into()),
            Err(KeyringError::Failed(e)) => return Err(e.into()),
        },
    })
}

#[cfg(windows)]
fn create_protected(_: DeviceId, _: &mut impl Keyring) -> Result<(Key, Stored), VaultError> {
    let key = Key::random();
    let blob = dpapi::protect(&key.0).map_err(|e| VaultError::Keyring(format!("Windows could not protect the key: {e}")))?;
    Ok((key, Stored::Dpapi(blob)))
}

/// An item already in the keyring is this profile's, left by a run that stopped before
/// recording it.
#[cfg(not(windows))]
fn create_protected(device: DeviceId, keyring: &mut impl Keyring) -> Result<(Key, Stored), VaultError> {
    match keyring.find(device, Unlock::Prompt) {
        Ok(Some(b)) => Ok((Key::from_slice(&b)?, Stored::SecretService)),
        Ok(None) => {
            let key = Key::random();
            keyring.store(device, &key, Unlock::Prompt).map_err(|e| match e {
                KeyringError::Unreachable(why) => VaultError::KeyringUnreachable(why),
                KeyringError::Failed(e) => e,
            })?;
            Ok((key, Stored::SecretService))
        }
        Err(KeyringError::Unreachable(why)) => {
            log::warn!("no system keyring is reachable ({why}), so this profile's key is kept unprotected in its database");
            let key = Key::random();
            Ok((key, Stored::Plain(key)))
        }
        Err(KeyringError::Failed(e)) => Err(e),
    }
}

/// Moves a key kept in the clear into the Secret Service, without prompting. The item is
/// stored before the row changes, so a crash between the two leaves the key readable from
/// the row, and the next load finds the item and finishes. A keyring that is unreachable,
/// locked or failing leaves the key where it is; only an item holding another key is an error.
#[cfg(not(windows))]
fn promote(conn: &Connection, device: DeviceId, key: Key, keyring: &mut impl Keyring) -> Result<(), Error> {
    let stored = match keyring.find(device, Unlock::Never) {
        Ok(Some(b)) if b == key.0 => Ok(()),
        // Moving the key is a nicety; a stray item must not stop the profile opening.
        Ok(Some(_)) => {
            log::warn!(
                "this profile's key stays in its database: the keyring already holds a different \"Vsesvit Safe Storage\" key for it; remove that item to let the key move there"
            );
            return Ok(());
        }
        Ok(None) => keyring.store(device, &key, Unlock::Never),
        Err(e) => Err(e),
    };
    match stored {
        Ok(()) => write(conn, &Stored::SecretService),
        Err(KeyringError::Unreachable(why)) => {
            log::debug!("this profile's key stays in its database: no system keyring is reachable ({why})");
            Ok(())
        }
        Err(KeyringError::Failed(e)) => {
            log::debug!("this profile's key stays in its database: {e}");
            Ok(())
        }
    }
}

#[cfg(windows)]
fn unprotect_dpapi(blob: &[u8]) -> Result<Key, VaultError> {
    let key = dpapi::unprotect(blob).map_err(|e| VaultError::KeyUnreadable(format!("Windows could not unprotect it: {e}")))?;
    Key::from_slice(&key)
}

#[cfg(not(windows))]
fn unprotect_dpapi(_: &[u8]) -> Result<Key, VaultError> {
    Err(VaultError::KeyUnreadable("Windows protects it, so only Windows can read it".to_owned()))
}

#[cfg(windows)]
mod dpapi {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    pub(super) fn protect(data: &[u8]) -> std::io::Result<Vec<u8>> {
        let input = blob(data)?;
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        // SAFETY: `input` points at `data`, which outlives the call; DPAPI only reads it.
        let ok = unsafe {
            CryptProtectData(&input, null(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(take(out))
    }

    pub(super) fn unprotect(data: &[u8]) -> std::io::Result<Vec<u8>> {
        let input = blob(data)?;
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        // SAFETY: as in `protect`.
        let ok = unsafe {
            CryptUnprotectData(&input, null_mut(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(take(out))
    }

    fn blob(data: &[u8]) -> std::io::Result<CRYPT_INTEGER_BLOB> {
        let len = u32::try_from(data.len()).map_err(|_| std::io::Error::other("too large for DPAPI"))?;
        Ok(CRYPT_INTEGER_BLOB { cbData: len, pbData: data.as_ptr().cast_mut() })
    }

    /// Copies a blob DPAPI allocated, then frees it.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: on success DPAPI returns `cbData` bytes at `pbData`, allocated with
        // LocalAlloc and owned by the caller.
        unsafe {
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData.cast());
            bytes
        }
    }
}

#[cfg(target_os = "linux")]
mod secret_service_keyring {
    use std::collections::HashMap;

    use secret_service::blocking::SecretService;
    use secret_service::{EncryptionType, Error as SsError};

    use super::{Key, Keyring, KeyringError, Unlock, VaultError};
    use crate::crdt::DeviceId;

    const LABEL: &str = "Vsesvit Safe Storage";

    pub(super) struct SecretServiceKeyring;

    impl Keyring for SecretServiceKeyring {
        fn find(&mut self, device: DeviceId, unlock: Unlock) -> Result<Option<Vec<u8>>, KeyringError> {
            let ss = connect()?;
            let device = device.to_hex();
            let found = ss.search_items(attributes(&device)).map_err(failed)?;
            let Some(item) = found.unlocked.into_iter().chain(found.locked).next() else {
                return Ok(None);
            };
            if item.is_locked().map_err(failed)? {
                refuse_unless(unlock)?;
                item.unlock().map_err(failed)?;
            }
            item.get_secret().map(Some).map_err(failed)
        }

        fn store(&mut self, device: DeviceId, key: &Key, unlock: Unlock) -> Result<(), KeyringError> {
            let ss = connect()?;
            let collection = ss.get_default_collection().map_err(failed)?;
            if collection.is_locked().map_err(failed)? {
                refuse_unless(unlock)?;
                collection.unlock().map_err(failed)?;
            }
            let device = device.to_hex();
            collection.create_item(LABEL, attributes(&device), &key.0, true, "application/octet-stream").map_err(failed)?;
            Ok(())
        }
    }

    /// Opening a session never prompts, so any failure here means there is no service to ask:
    /// no session bus, no provider on it, or one that could not start.
    fn connect() -> Result<SecretService<'static>, KeyringError> {
        SecretService::connect(EncryptionType::Dh).map_err(|e| KeyringError::Unreachable(e.to_string()))
    }

    fn refuse_unless(unlock: Unlock) -> Result<(), KeyringError> {
        match unlock {
            Unlock::Prompt => Ok(()),
            Unlock::Never => Err(KeyringError::Failed(VaultError::KeyringLocked)),
        }
    }

    fn attributes(device: &str) -> HashMap<&str, &str> {
        HashMap::from([("application", "vsesvit"), ("device_id", device)])
    }

    fn failed(e: SsError) -> KeyringError {
        match e {
            SsError::Locked | SsError::Prompt | SsError::PromptDisconnected => KeyringError::Failed(VaultError::KeyringLocked),
            SsError::Unavailable => KeyringError::Unreachable(e.to_string()),
            e => KeyringError::Failed(VaultError::Keyring(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AAD: &[u8] = b"sync_secrets:token";

    #[test]
    fn seal_then_open_round_trips() {
        let key = Key::random();
        for plaintext in [&b""[..], b"x", &[7; 1000]] {
            let sealed = seal(&key, AAD, plaintext);
            assert_eq!(sealed.len(), 1 + NONCE_LEN + plaintext.len() + 16);
            assert_eq!(open(&key, AAD, &sealed).unwrap(), plaintext);
        }
    }

    #[test]
    fn each_seal_uses_a_fresh_nonce() {
        let key = Key::random();
        assert_ne!(seal(&key, AAD, b"same"), seal(&key, AAD, b"same"));
    }

    #[test]
    fn any_altered_byte_fails_to_open() {
        let key = Key::random();
        let sealed = seal(&key, AAD, b"refresh token");
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 0x01;
            assert!(matches!(open(&key, AAD, &bad), Err(VaultError::Damaged)), "byte {i}");
        }
        for len in 0..sealed.len() {
            assert!(matches!(open(&key, AAD, &sealed[..len]), Err(VaultError::Damaged)), "truncated to {len}");
        }
    }

    #[test]
    fn another_key_or_another_row_fails_to_open() {
        let key = Key::random();
        let sealed = seal(&key, AAD, b"refresh token");
        assert!(matches!(open(&Key::random(), AAD, &sealed), Err(VaultError::Damaged)));
        assert!(matches!(open(&key, b"sync_secrets:other", &sealed), Err(VaultError::Damaged)));
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn
    }

    fn row(conn: &Connection) -> Option<(String, Option<Vec<u8>>)> {
        conn.query_row("SELECT protection, key FROM vault_key", [], |r| Ok((r.get(0)?, r.get(1)?))).optional().unwrap()
    }

    /// Never reachable: [`KeyStore::Basic`] must not ask the keyring at all.
    struct Untouchable;

    impl Keyring for Untouchable {
        fn find(&mut self, _: DeviceId, _: Unlock) -> Result<Option<Vec<u8>>, KeyringError> {
            panic!("the keyring was asked")
        }
        fn store(&mut self, _: DeviceId, _: &Key, _: Unlock) -> Result<(), KeyringError> {
            panic!("the keyring was asked")
        }
    }

    #[test]
    fn basic_keeps_the_key_in_the_database_and_reads_it_back() {
        let conn = db();
        let key = resolve(&conn, DeviceId(1), KeyStore::Basic, &mut Untouchable).unwrap();
        assert_eq!(row(&conn), Some(("plain".to_owned(), Some(key.0.to_vec()))));
        assert_eq!(resolve(&conn, DeviceId(1), KeyStore::Basic, &mut Untouchable).unwrap(), key);
        #[cfg(windows)]
        assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut Untouchable).unwrap(), key, "Windows leaves a plain key alone");
        assert_eq!(row(&conn), Some(("plain".to_owned(), Some(key.0.to_vec()))));
    }

    #[cfg(windows)]
    #[test]
    fn windows_wraps_the_key_with_dpapi() {
        let conn = db();
        let key = resolve(&conn, DeviceId(1), KeyStore::System, &mut Untouchable).unwrap();
        let (protection, blob) = row(&conn).unwrap();
        assert_eq!(protection, "dpapi");
        let blob = blob.unwrap();
        assert!(!blob.windows(KEY_LEN).any(|w| w == key.0), "the key is not stored in the clear");
        assert_eq!(dpapi::unprotect(&blob).unwrap(), key.0);
        assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut Untouchable).unwrap(), key);

        let mut bad = blob.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        conn.execute("UPDATE vault_key SET key = ?1", [bad]).unwrap();
        let err = resolve(&conn, DeviceId(1), KeyStore::System, &mut Untouchable).unwrap_err();
        assert!(matches!(err, Error::Vault(VaultError::KeyUnreadable(_))), "{err}");
    }

    #[cfg(not(windows))]
    mod keyring_choice {
        use std::collections::HashMap;

        use super::*;

        #[derive(Default)]
        struct Fake {
            items: HashMap<DeviceId, Vec<u8>>,
            unreachable: bool,
            locked: bool,
            stores: usize,
            prompted: bool,
        }

        impl Keyring for Fake {
            fn find(&mut self, device: DeviceId, unlock: Unlock) -> Result<Option<Vec<u8>>, KeyringError> {
                self.prompted |= unlock == Unlock::Prompt;
                if self.unreachable {
                    return Err(KeyringError::Unreachable("no session bus".to_owned()));
                }
                if self.locked && self.items.contains_key(&device) {
                    return Err(KeyringError::Failed(VaultError::KeyringLocked));
                }
                Ok(self.items.get(&device).cloned())
            }

            fn store(&mut self, device: DeviceId, key: &Key, unlock: Unlock) -> Result<(), KeyringError> {
                self.prompted |= unlock == Unlock::Prompt;
                if self.unreachable {
                    return Err(KeyringError::Unreachable("no session bus".to_owned()));
                }
                self.stores += 1;
                if self.locked {
                    return Err(KeyringError::Failed(VaultError::KeyringLocked));
                }
                self.items.insert(device, key.0.to_vec());
                Ok(())
            }
        }

        #[test]
        fn a_reachable_keyring_holds_the_key_and_the_database_only_says_so() {
            let conn = db();
            let mut keyring = Fake::default();
            let key = resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap();
            assert_eq!(keyring.items[&DeviceId(1)], key.0);
            assert_eq!(row(&conn), Some(("secret_service".to_owned(), None)));
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), key);
        }

        #[test]
        fn an_item_left_by_an_interrupted_run_is_reused() {
            let conn = db();
            let mut keyring = Fake::default();
            keyring.items.insert(DeviceId(1), vec![9; KEY_LEN]);
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), Key([9; KEY_LEN]));
            assert_eq!(row(&conn), Some(("secret_service".to_owned(), None)));
        }

        /// A profile whose key was made in the clear, as with no keyring reachable.
        fn plain_profile() -> (Connection, Key) {
            let conn = db();
            let key = resolve(&conn, DeviceId(1), KeyStore::System, &mut Fake { unreachable: true, ..Fake::default() }).unwrap();
            assert_eq!(row(&conn), Some(("plain".to_owned(), Some(key.0.to_vec()))));
            (conn, key)
        }

        #[test]
        fn a_plain_key_stays_while_no_keyring_is_reachable() {
            let (conn, key) = plain_profile();
            let mut keyring = Fake { unreachable: true, ..Fake::default() };
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), key);
            assert_eq!(row(&conn), Some(("plain".to_owned(), Some(key.0.to_vec()))));
        }

        #[test]
        fn a_plain_key_moves_into_a_keyring_that_appears() {
            let (conn, key) = plain_profile();
            let mut keyring = Fake::default();
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), key);
            assert_eq!(keyring.items[&DeviceId(1)], key.0, "the same key, so nothing is sealed again");
            assert_eq!(row(&conn), Some(("secret_service".to_owned(), None)));
            assert!(!keyring.prompted, "moving never prompts");
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), key);
        }

        #[test]
        fn a_plain_key_stays_while_the_keyring_is_locked() {
            let (conn, key) = plain_profile();
            let mut keyring = Fake { locked: true, ..Fake::default() };
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), key);
            assert_eq!(row(&conn), Some(("plain".to_owned(), Some(key.0.to_vec()))));
            assert!(!keyring.prompted, "moving never prompts");
            assert!(keyring.items.is_empty());
        }

        #[test]
        fn an_interrupted_move_still_opens_and_finishes_next_time() {
            let (conn, key) = plain_profile();
            let mut keyring = Fake::default();
            keyring.items.insert(DeviceId(1), key.0.to_vec());
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), key);
            assert_eq!(keyring.stores, 0, "the item already holds the key");
            assert_eq!(row(&conn), Some(("secret_service".to_owned(), None)));
        }

        #[test]
        fn an_item_holding_another_key_leaves_the_key_where_it_is() {
            let (conn, key) = plain_profile();
            let mut keyring = Fake::default();
            keyring.items.insert(DeviceId(1), vec![9; KEY_LEN]);
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap(), key, "the profile still opens");
            assert_eq!(row(&conn), Some(("plain".to_owned(), Some(key.0.to_vec()))));
            assert_eq!(keyring.items[&DeviceId(1)], vec![9; KEY_LEN], "the other item is left alone");
        }

        #[test]
        fn a_basic_profile_never_moves_its_key() {
            let conn = db();
            let key = resolve(&conn, DeviceId(1), KeyStore::Basic, &mut Untouchable).unwrap();
            assert_eq!(resolve(&conn, DeviceId(1), KeyStore::Basic, &mut Fake::default()).unwrap(), key);
            assert_eq!(row(&conn), Some(("plain".to_owned(), Some(key.0.to_vec()))));
        }

        #[test]
        fn a_locked_keyring_is_an_error_and_records_nothing() {
            let conn = db();
            let mut keyring = Fake { locked: true, ..Fake::default() };
            let err = resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap_err();
            assert!(matches!(err, Error::Vault(VaultError::KeyringLocked)), "{err}");
            assert_eq!(row(&conn), None);
        }

        #[test]
        fn a_key_in_an_unreachable_keyring_is_an_error_not_a_new_key() {
            let conn = db();
            let mut keyring = Fake::default();
            resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap();

            keyring.unreachable = true;
            let err = resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap_err();
            assert!(matches!(err, Error::Vault(VaultError::KeyringUnreachable(_))), "{err}");
            keyring.unreachable = false;
            keyring.locked = true;
            let err = resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap_err();
            assert!(matches!(err, Error::Vault(VaultError::KeyringLocked)), "{err}");
            keyring.locked = false;
            keyring.items.clear();
            let err = resolve(&conn, DeviceId(1), KeyStore::System, &mut keyring).unwrap_err();
            assert!(matches!(err, Error::Vault(VaultError::KeyUnreadable(_))), "{err}");
            assert_eq!(row(&conn), Some(("secret_service".to_owned(), None)));
        }

        #[test]
        fn a_dpapi_key_is_unreadable_off_windows() {
            let conn = db();
            conn.execute("INSERT INTO vault_key (id, protection, key) VALUES (1, 'dpapi', x'00')", []).unwrap();
            let err = resolve(&conn, DeviceId(1), KeyStore::System, &mut Fake::default()).unwrap_err();
            assert!(matches!(err, Error::Vault(VaultError::KeyUnreadable(_))), "{err}");
        }
    }
}
