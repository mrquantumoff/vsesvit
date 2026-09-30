//! The profile vault through `SyncStore::secret_state`: sealing, persistence across reopen,
//! and the failures that must surface as errors.

use std::path::{Path, PathBuf};

use vsesvit_core::vault::KeyStore;
use vsesvit_core::{Error, OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp() -> TempDir {
    TempDir(std::env::temp_dir().join(format!("vsesvit-vault-{}", uuid::Uuid::new_v4())))
}

/// DPAPI on Windows never prompts. On Linux the system store is the user's real keyring,
/// which a test must not touch; `vault.rs` tests the choice of store with a fake one.
#[cfg(windows)]
const STORE: KeyStore = KeyStore::System;
#[cfg(not(windows))]
const STORE: KeyStore = KeyStore::Basic;

fn open_with(dir: &Path, key_store: KeyStore) -> Profile {
    Profile::open(dir, OpenOptions { key_store, ..OpenOptions::default() }).unwrap()
}

fn open(dir: &Path) -> Profile {
    open_with(dir, STORE)
}

fn db(dir: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.join("vsesvit.db")).unwrap()
}

fn sealed(dir: &Path, key: &str) -> Vec<u8> {
    db(dir).query_row("SELECT value FROM sync_secrets WHERE key = ?1", [key], |r| r.get(0)).unwrap()
}

#[cfg(windows)]
fn protection(dir: &Path) -> String {
    db(dir).query_row("SELECT protection FROM vault_key", [], |r| r.get(0)).unwrap()
}

#[test]
fn a_secret_round_trips_and_is_not_stored_in_the_clear() {
    let dir = tmp();
    let mut p = open(&dir.0);
    assert_eq!(p.sync().secret_state("tokens").unwrap(), None);
    p.sync().set_secret_state("tokens", b"refresh-token-123").unwrap();
    assert_eq!(p.sync().secret_state("tokens").unwrap().as_deref(), Some(&b"refresh-token-123"[..]));
    drop(p);
    let stored = sealed(&dir.0, "tokens");
    assert!(!stored.windows(8).any(|w| w == b"refresh-"), "sealed, not plain");
}

#[test]
fn the_same_key_comes_back_after_reopen() {
    for store in [STORE, KeyStore::Basic] {
        let dir = tmp();
        let mut p = open_with(&dir.0, store);
        p.sync().set_secret_state("tokens", b"kept").unwrap();
        drop(p);
        let mut p = open_with(&dir.0, store);
        assert_eq!(p.sync().secret_state("tokens").unwrap().as_deref(), Some(&b"kept"[..]), "{store:?}");
    }
}

/// Windows only: elsewhere `System` moves a plain key into the user's real keyring.
#[cfg(windows)]
#[test]
fn the_store_option_only_places_a_new_key() {
    let dir = tmp();
    let mut p = open_with(&dir.0, KeyStore::Basic);
    p.sync().set_secret_state("tokens", b"kept").unwrap();
    drop(p);
    assert_eq!(protection(&dir.0), "plain");
    let mut p = open_with(&dir.0, KeyStore::System);
    assert_eq!(p.sync().secret_state("tokens").unwrap().as_deref(), Some(&b"kept"[..]));
    drop(p);
    assert_eq!(protection(&dir.0), "plain");
}

#[test]
fn an_empty_value_removes_the_secret() {
    let dir = tmp();
    let mut p = open(&dir.0);
    p.sync().set_secret_state("tokens", b"x").unwrap();
    p.sync().set_secret_state("tokens", b"").unwrap();
    assert_eq!(p.sync().secret_state("tokens").unwrap(), None);
    p.sync().set_secret_state("never-set", b"").unwrap();
}

#[test]
fn secrets_and_engine_state_are_separate() {
    let dir = tmp();
    let mut p = open(&dir.0);
    p.sync().set_engine_state("k", b"plain").unwrap();
    p.sync().set_secret_state("k", b"secret").unwrap();
    assert_eq!(p.sync().engine_state("k").unwrap().as_deref(), Some(&b"plain"[..]));
    assert_eq!(p.sync().secret_state("k").unwrap().as_deref(), Some(&b"secret"[..]));
}

fn assert_damaged(result: Result<Option<Vec<u8>>, Error>) {
    let err = result.unwrap_err();
    assert!(err.to_string().contains("could not be decrypted"), "{err}");
}

#[test]
fn an_altered_secret_is_an_error_not_none() {
    let dir = tmp();
    let mut p = open(&dir.0);
    p.sync().set_secret_state("tokens", b"refresh-token").unwrap();
    drop(p);
    let mut bad = sealed(&dir.0, "tokens");
    let last = bad.len() - 1;
    bad[last] ^= 0x01;
    db(&dir.0).execute("UPDATE sync_secrets SET value = ?1 WHERE key = 'tokens'", [bad]).unwrap();
    assert_damaged(open(&dir.0).sync().secret_state("tokens"));
}

#[test]
fn a_secret_from_another_profile_or_row_does_not_open() {
    let (a, b) = (tmp(), tmp());
    let mut p = open(&a.0);
    p.sync().set_secret_state("tokens", b"from a").unwrap();
    drop(p);
    let mut q = open(&b.0);
    q.sync().set_secret_state("tokens", b"from b").unwrap();
    drop(q);

    let from_a = sealed(&a.0, "tokens");
    let conn = db(&b.0);
    conn.execute("UPDATE sync_secrets SET value = ?1 WHERE key = 'tokens'", [&from_a]).unwrap();
    conn.execute("INSERT INTO sync_secrets (key, value) VALUES ('other', ?1)", [sealed(&b.0, "tokens")]).unwrap();
    drop(conn);
    let mut q = open(&b.0);
    assert_damaged(q.sync().secret_state("tokens"));
    assert_damaged(q.sync().secret_state("other"));
}

#[cfg(windows)]
#[test]
fn windows_keeps_the_key_under_dpapi_and_reports_a_damaged_one() {
    let dir = tmp();
    let mut p = open_with(&dir.0, KeyStore::System);
    p.sync().set_secret_state("tokens", b"x").unwrap();
    drop(p);
    assert_eq!(protection(&dir.0), "dpapi");

    db(&dir.0).execute("UPDATE vault_key SET key = x'00'", []).unwrap();
    let mut p = open_with(&dir.0, KeyStore::System);
    let err = p.sync().secret_state("tokens").unwrap_err();
    assert!(err.to_string().starts_with("the protected key could not be read"), "{err}");
    p.sync().set_secret_state("tokens", b"").unwrap();
    assert_eq!(p.sync().secret_state("tokens").unwrap(), None, "signing out needs no key");
}
