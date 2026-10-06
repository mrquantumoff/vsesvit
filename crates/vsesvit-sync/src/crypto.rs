//! The formats of end-to-end encrypted sync (docs/design/sync-encryption.md): the keyring, the key
//! record that holds it wrapped by the sync passphrase, and sealed records.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use vsesvit_sync_proto::Record;
use zeroize::Zeroize;

/// The wire kind of the key record. Core's `Kind` codes stay below it.
pub(crate) const KEYS_KIND: u8 = 200;
/// The wire kind of every sealed record, whatever its own kind.
pub(crate) const SEALED_KIND: u8 = 201;
pub(crate) const KEYS_ID: &str = "keys";

const FORMAT: u8 = 1;
const KEY_LEN: usize = 32;
const KEY_ID_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const SALT_LEN: usize = 16;
const HEADER_LEN: usize = 1 + KEY_ID_LEN + NONCE_LEN;
/// Kind, then the lengths of the id and the body.
const FRAME_LEN: usize = 1 + 4 + 4;

/// What a key record derives its key with: RFC 9106's second recommendation, with one lane
/// because the derivation runs on one thread. Format 1 takes no others, so a server cannot make a
/// device spend more, or less.
const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const PARALLELISM: u32 = 1;

pub const MIN_PASSPHRASE_CHARS: usize = 8;

/// A sync passphrase as typed, in Unicode's NFC (RFC 8265), so it derives the same key whichever
/// way a keyboard composed it. Cleared from memory when dropped.
pub struct Passphrase(String);

impl Passphrase {
    /// `None` when shorter than [`MIN_PASSPHRASE_CHARS`].
    pub fn new(mut text: String) -> Option<Passphrase> {
        let passphrase = Passphrase(icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(&text).into_owned());
        text.zeroize();
        (passphrase.0.chars().count() >= MIN_PASSPHRASE_CHARS).then_some(passphrase)
    }
}

impl Drop for Passphrase {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Passphrase(..)")
    }
}

/// The key records are sealed with.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecordKey {
    #[serde(with = "bytes")]
    id: [u8; KEY_ID_LEN],
    #[serde(with = "bytes")]
    key: [u8; KEY_LEN],
}

/// A key a passphrase change replaced, as its id and a hash: enough to tell its records apart and
/// to show that a later keyring descends from the one it was in, and nothing that opens them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Replaced {
    #[serde(with = "bytes")]
    id: [u8; KEY_ID_LEN],
    #[serde(with = "bytes")]
    hash: [u8; 32],
}

/// The account's keys. Its generation is how many passphrase changes replaced a key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Keyring {
    #[serde(with = "bytes")]
    id_key: [u8; KEY_LEN],
    replaced: Vec<Replaced>,
    current: RecordKey,
}

impl Drop for Keyring {
    fn drop(&mut self) {
        self.id_key.zeroize();
        self.current.key.zeroize();
    }
}

impl std::fmt::Debug for Keyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keyring(generation {})", self.generation())
    }
}

/// Why a sealed record did not open.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unopened {
    /// Sealed with a key a passphrase change replaced.
    Stale,
    /// Sealed with a key this keyring never had.
    Foreign,
    /// Altered, truncated, or not a sealed record.
    Damaged,
}

/// A record as it was before sealing. `kind` may be one this build does not know.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Opened {
    pub kind: u8,
    pub id: String,
    pub body: Vec<u8>,
}

impl Keyring {
    pub(crate) fn new() -> Keyring {
        Keyring { id_key: random(), replaced: Vec::new(), current: RecordKey { id: random(), key: random() } }
    }

    /// This keyring with a new key, for a new passphrase.
    pub(crate) fn next(&self) -> Keyring {
        let mut next = self.clone();
        next.replaced.push(Replaced { id: self.current.id, hash: key_hash(&self.current) });
        next.current = RecordKey { id: random(), key: random() };
        next
    }

    pub(crate) fn generation(&self) -> u32 {
        u32::try_from(self.replaced.len()).expect("fewer than 2^32 passphrase changes")
    }

    /// Made from `older` by passphrase changes: the same id key, and `older`'s keys replaced in the
    /// same order, its current key among them.
    pub(crate) fn descends_from(&self, older: &Keyring) -> bool {
        self.id_key == older.id_key
            && self.replaced.starts_with(&older.replaced)
            && self
                .replaced
                .get(older.replaced.len())
                .is_some_and(|r| r.id == older.current.id && r.hash == key_hash(&older.current))
    }

    /// The id the server sees for a record.
    pub(crate) fn server_id(&self, kind: u8, id: &str) -> String {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.id_key).expect("HMAC takes any key length");
        mac.update(b"vsesvit-sync id\0");
        mac.update(&[kind]);
        mac.update(id.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    /// The body length [`Keyring::seal`] makes of a record with these lengths.
    pub(crate) fn sealed_len(id_len: usize, body_len: usize) -> usize {
        HEADER_LEN + padme(FRAME_LEN + id_len + body_len) + TAG_LEN
    }

    pub(crate) fn seal(&self, kind: u8, id: &str, body: &[u8]) -> Record {
        let server_id = self.server_id(kind, id);
        let mut plaintext = Vec::with_capacity(padme(FRAME_LEN + id.len() + body.len()));
        plaintext.push(kind);
        plaintext.extend_from_slice(&len32(id.len()).to_le_bytes());
        plaintext.extend_from_slice(id.as_bytes());
        plaintext.extend_from_slice(&len32(body.len()).to_le_bytes());
        plaintext.extend_from_slice(body);
        plaintext.resize(padme(plaintext.len()), 0);
        let nonce: [u8; NONCE_LEN] = random();
        let aad = record_aad(&self.current.id, &server_id);
        let sealed = cipher(&self.current.key)
            .encrypt(&nonce.into(), Payload { msg: &plaintext, aad: &aad })
            .expect("sealing in memory cannot fail");
        plaintext.zeroize();
        Record { kind: SEALED_KIND, id: server_id, body: [&[FORMAT][..], &self.current.id, &nonce, &sealed].concat() }
    }

    /// Opens a record of [`SEALED_KIND`].
    pub(crate) fn open(&self, record: &Record) -> Result<Opened, Unopened> {
        let [FORMAT, rest @ ..] = record.body.as_slice() else {
            return Err(Unopened::Damaged);
        };
        let Some((key_id, rest)) = rest.split_first_chunk::<KEY_ID_LEN>() else {
            return Err(Unopened::Damaged);
        };
        let Some((nonce, ciphertext)) = rest.split_first_chunk::<NONCE_LEN>() else {
            return Err(Unopened::Damaged);
        };
        if *key_id != self.current.id {
            return Err(if self.replaced.iter().any(|r| r.id == *key_id) { Unopened::Stale } else { Unopened::Foreign });
        }
        let aad = record_aad(key_id, &record.id);
        let mut plaintext = cipher(&self.current.key)
            .decrypt(&(*nonce).into(), Payload { msg: ciphertext, aad: &aad })
            .map_err(|_| Unopened::Damaged)?;
        let opened = unframe(&plaintext);
        plaintext.zeroize();
        let opened = opened.ok_or(Unopened::Damaged)?;
        // Only a holder of the keys could seal it, so this catches a device's bug, not the server.
        if self.server_id(opened.kind, &opened.id) != record.id {
            return Err(Unopened::Damaged);
        }
        Ok(opened)
    }

    /// The key record of this keyring, wrapped with a key derived from `passphrase` and a new salt.
    /// Slow (Argon2id); run it on a worker.
    pub(crate) fn wrap(&self, passphrase: &Passphrase) -> KeyRecord {
        let kdf = Kdf { memory_kib: MEMORY_KIB, iterations: ITERATIONS, parallelism: PARALLELISM, salt: random::<SALT_LEN>().to_vec() };
        let mut kek = kdf.derive(passphrase).expect("format 1's parameters derive");
        let mut record = KeyRecord {
            version: FORMAT,
            generation: self.generation(),
            key_id: self.current.id,
            kdf,
            nonce: random(),
            keyring: Vec::new(),
        };
        let mut plaintext = self.to_secret();
        record.keyring = cipher(&kek)
            .encrypt(&record.nonce.into(), Payload { msg: &plaintext, aad: &record.aad() })
            .expect("sealing in memory cannot fail");
        plaintext.zeroize();
        kek.zeroize();
        record
    }

    /// For the profile's sealed secrets.
    pub(crate) fn to_secret(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a keyring serializes")
    }

    pub(crate) fn from_secret(bytes: &[u8]) -> Option<Keyring> {
        serde_json::from_slice(bytes).ok()
    }
}

fn key_hash(key: &RecordKey) -> [u8; 32] {
    Sha256::new().chain_update(b"vsesvit-sync replaced key\0").chain_update(key.id).chain_update(key.key).finalize().into()
}

/// Why a key record did not open.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unwrapped {
    /// The passphrase does not open it: a wrong one, or an altered record.
    WrongPassphrase,
    /// Parameters other than format 1's, or a keyring that disagrees with the record.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Kdf {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    #[serde(with = "base64_vec")]
    salt: Vec<u8>,
}

impl Kdf {
    /// `None` for parameters other than format 1's, checked before any memory is taken.
    fn derive(&self, passphrase: &Passphrase) -> Option<[u8; KEY_LEN]> {
        if (self.memory_kib, self.iterations, self.parallelism, self.salt.len()) != (MEMORY_KIB, ITERATIONS, PARALLELISM, SALT_LEN) {
            return None;
        }
        let params = Params::new(self.memory_kib, self.iterations, self.parallelism, Some(KEY_LEN)).ok()?;
        let mut key = [0; KEY_LEN];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params).hash_password_into(passphrase.0.as_bytes(), &self.salt, &mut key).ok()?;
        Some(key)
    }
}

/// A keyring wrapped by a passphrase, as the server stores it in the key slot. Readable by anyone;
/// only the passphrase opens `keyring`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KeyRecord {
    version: u8,
    generation: u32,
    #[serde(with = "bytes")]
    key_id: [u8; KEY_ID_LEN],
    kdf: Kdf,
    #[serde(with = "bytes")]
    nonce: [u8; NONCE_LEN],
    #[serde(with = "base64_vec")]
    keyring: Vec<u8>,
}

impl KeyRecord {
    /// `None` for a body that is not a key record of a format this build reads.
    pub(crate) fn parse(body: &[u8]) -> Option<KeyRecord> {
        serde_json::from_slice::<KeyRecord>(body).ok().filter(|r| r.version == FORMAT)
    }

    pub(crate) fn to_record(&self) -> Record {
        Record { kind: KEYS_KIND, id: KEYS_ID.to_owned(), body: serde_json::to_vec(self).expect("a key record serializes") }
    }

    pub(crate) fn generation(&self) -> u32 {
        self.generation
    }

    /// Whether this is the record of `keyring`'s current key.
    pub(crate) fn is_of(&self, keyring: &Keyring) -> bool {
        self.generation == keyring.generation() && self.key_id == keyring.current.id
    }

    /// The key check: opens the keyring with `passphrase`. Slow (Argon2id); run it on a worker.
    pub(crate) fn unwrap(&self, passphrase: &Passphrase) -> Result<Keyring, Unwrapped> {
        let mut kek = self.kdf.derive(passphrase).ok_or(Unwrapped::Invalid)?;
        let opened = cipher(&kek).decrypt(&self.nonce.into(), Payload { msg: &self.keyring, aad: &self.aad() });
        kek.zeroize();
        let mut plaintext = opened.map_err(|_| Unwrapped::WrongPassphrase)?;
        let keyring = Keyring::from_secret(&plaintext);
        plaintext.zeroize();
        keyring.filter(|k| self.is_of(k)).ok_or(Unwrapped::Invalid)
    }

    fn aad(&self) -> Vec<u8> {
        let Kdf { memory_kib, iterations, parallelism, salt } = &self.kdf;
        [
            &b"vsesvit-sync keys\0"[..],
            &[self.version],
            &self.generation.to_le_bytes(),
            &self.key_id,
            &memory_kib.to_le_bytes(),
            &iterations.to_le_bytes(),
            &parallelism.to_le_bytes(),
            salt,
        ]
        .concat()
    }
}

fn record_aad(key_id: &[u8; KEY_ID_LEN], server_id: &str) -> Vec<u8> {
    [&b"vsesvit-sync record\0"[..], &[FORMAT], key_id, server_id.as_bytes()].concat()
}

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(&(*key).into())
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes
}

fn len32(len: usize) -> u32 {
    u32::try_from(len).expect("a record is under 4 GiB")
}

fn unframe(plaintext: &[u8]) -> Option<Opened> {
    let (&kind, rest) = plaintext.split_first()?;
    let (id_len, rest) = rest.split_first_chunk::<4>()?;
    let (id, rest) = rest.split_at_checked(u32::from_le_bytes(*id_len) as usize)?;
    let (body_len, rest) = rest.split_first_chunk::<4>()?;
    let (body, padding) = rest.split_at_checked(u32::from_le_bytes(*body_len) as usize)?;
    if padding.iter().any(|&b| b != 0) {
        return None;
    }
    Some(Opened { kind, id: String::from_utf8(id.to_vec()).ok()?, body: body.to_vec() })
}

/// Padmé (Nikitin et al., 2019): rounds `len` up so that it leaks O(log log len) bits, at most 12%
/// more bytes.
fn padme(len: usize) -> usize {
    if len < 2 {
        return len;
    }
    let e = len.ilog2();
    let s = e.ilog2() + 1;
    let mask = (1usize << (e - s)) - 1;
    (len + mask) & !mask
}

mod bytes {
    use super::*;

    pub(super) fn serialize<S: Serializer, const N: usize>(bytes: &[u8; N], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>, const N: usize>(d: D) -> Result<[u8; N], D::Error> {
        let bytes = super::base64_vec::deserialize(d)?;
        <[u8; N]>::try_from(bytes).map_err(|b| serde::de::Error::invalid_length(b.len(), &"a fixed number of bytes"))
    }
}

mod base64_vec {
    use super::*;

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        STANDARD.decode(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passphrase(text: &str) -> Passphrase {
        Passphrase::new(text.to_owned()).unwrap()
    }

    #[test]
    fn a_record_round_trips_and_hides_its_kind_id_and_size() {
        let keyring = Keyring::new();
        let sealed = keyring.seal(2, "https://example.com/secret", b"{\"title\":\"x\"}");
        assert_eq!(sealed.kind, SEALED_KIND);
        assert_eq!(sealed.id.len(), 43);
        assert!(!sealed.id.contains("example"));
        assert_eq!(sealed.body.len(), Keyring::sealed_len(26, 13));
        assert!(!sealed.body.windows(7).any(|w| w == b"example"));
        assert_eq!(keyring.open(&sealed).unwrap(), Opened { kind: 2, id: "https://example.com/secret".to_owned(), body: b"{\"title\":\"x\"}".to_vec() });
        assert_eq!(keyring.seal(2, "https://example.com/secret", b"").id, sealed.id, "the same record, the same slot");
        assert_ne!(keyring.seal(1, "https://example.com/secret", b"").id, sealed.id, "the kind is part of the slot");
        assert_ne!(Keyring::new().seal(2, "https://example.com/secret", b"").id, sealed.id, "another account, another slot");
    }

    #[test]
    fn each_seal_takes_a_fresh_nonce() {
        let keyring = Keyring::new();
        let (a, b) = (keyring.seal(1, "a", b"same"), keyring.seal(1, "a", b"same"));
        assert_eq!(a.id, b.id);
        assert_ne!(a.body[HEADER_LEN - NONCE_LEN..HEADER_LEN], b.body[HEADER_LEN - NONCE_LEN..HEADER_LEN]);
    }

    #[test]
    fn any_altered_or_truncated_byte_is_damage() {
        let keyring = Keyring::new();
        let sealed = keyring.seal(7, "theme", b"{\"v\":\"dark\"}");
        for i in 0..sealed.body.len() {
            let mut bad = sealed.clone();
            bad.body[i] ^= 0x01;
            let expected = if (1..1 + KEY_ID_LEN).contains(&i) { Unopened::Foreign } else { Unopened::Damaged };
            assert_eq!(keyring.open(&bad), Err(expected), "byte {i}");
        }
        for len in 0..sealed.body.len() {
            let mut bad = sealed.clone();
            bad.body.truncate(len);
            assert!(keyring.open(&bad).is_err(), "truncated to {len}");
        }
    }

    #[test]
    fn a_record_moved_to_another_slot_does_not_open() {
        let keyring = Keyring::new();
        let bookmark = keyring.seal(1, "a", b"{}");
        let other = keyring.seal(1, "b", b"{}");
        let moved = Record { id: other.id, ..bookmark };
        assert_eq!(keyring.open(&moved), Err(Unopened::Damaged));
    }

    #[test]
    fn another_accounts_key_and_a_replaced_key_do_not_open() {
        let keyring = Keyring::new();
        let sealed = keyring.seal(1, "a", b"{}");
        assert_eq!(Keyring::new().open(&sealed), Err(Unopened::Foreign));
        let next = keyring.next();
        assert_eq!(next.open(&sealed), Err(Unopened::Stale), "a replaced key's records are refused");
        assert_eq!(next.server_id(1, "a"), sealed.id, "a new key keeps the slots");
        assert_eq!(keyring.open(&next.seal(1, "a", b"{}")), Err(Unopened::Foreign));
    }

    #[test]
    fn a_keyring_keeps_no_key_it_replaced() {
        let first = Keyring::new();
        let second = first.next();
        assert!(!second.to_secret().windows(43).any(|w| w == STANDARD.encode(first.current.key).as_bytes()));
    }

    #[test]
    fn padding_must_be_zeros_and_lengths_must_fit() {
        assert_eq!(unframe(&[1, 1, 0, 0, 0, b'a', 2, 0, 0, 0, b'{', b'}', 0, 0]).unwrap().body, b"{}");
        assert!(unframe(&[1, 1, 0, 0, 0, b'a', 2, 0, 0, 0, b'{', b'}', 0, 1]).is_none());
        assert!(unframe(&[1, 9, 0, 0, 0, b'a']).is_none());
        assert!(unframe(&[1, 1, 0, 0, 0, b'a', 9, 0, 0, 0, b'{']).is_none());
        assert!(unframe(&[1, 1, 0, 0, 0, 0xff, 0, 0, 0, 0]).is_none(), "an id that is not UTF-8");
    }

    #[test]
    fn padme_leaks_little_and_wastes_little() {
        assert_eq!((0..=8).map(padme).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(padme(9), 10);
        assert_eq!(padme(100), 104);
        assert_eq!(padme(1000), 1024);
        for len in 2..20_000 {
            let padded = padme(len);
            assert!(padded >= len && padded - len <= len / 8 + 1, "{len} -> {padded}");
        }
    }

    #[test]
    fn the_key_record_opens_with_its_passphrase_only() {
        let keyring = Keyring::new();
        let record = keyring.wrap(&passphrase("correct horse"));
        let parsed = KeyRecord::parse(&record.to_record().body).unwrap();
        assert_eq!(parsed, record);
        assert!(parsed.is_of(&keyring));
        assert_eq!(parsed.unwrap(&passphrase("correct horse")).unwrap(), keyring);
        assert_eq!(parsed.unwrap(&passphrase("correct horsf")).unwrap_err(), Unwrapped::WrongPassphrase);
        assert!(!String::from_utf8(record.to_record().body).unwrap().contains(&STANDARD.encode(keyring.current.key)));
    }

    #[test]
    fn a_passphrase_opens_its_record_however_it_was_composed() {
        let keyring = Keyring::new();
        let record = keyring.wrap(&passphrase("caf\u{e9} au lait"));
        assert_eq!(record.unwrap(&passphrase("cafe\u{301} au lait")).unwrap(), keyring);
    }

    #[test]
    fn an_altered_key_record_fails_the_key_check() {
        let keyring = Keyring::new().next();
        let record = keyring.wrap(&passphrase("correct horse"));
        let open = |r: &KeyRecord| r.unwrap(&passphrase("correct horse"));
        let mut changed = record.clone();
        changed.generation = 0;
        assert_eq!(open(&changed).unwrap_err(), Unwrapped::WrongPassphrase, "the generation is bound");
        let mut changed = record.clone();
        changed.key_id[0] ^= 1;
        assert_eq!(open(&changed).unwrap_err(), Unwrapped::WrongPassphrase, "the key id is bound");
        let mut changed = record.clone();
        changed.kdf.salt[0] ^= 1;
        assert_eq!(open(&changed).unwrap_err(), Unwrapped::WrongPassphrase);
        let mut changed = record.clone();
        changed.keyring[5] ^= 1;
        assert_eq!(open(&changed).unwrap_err(), Unwrapped::WrongPassphrase);
    }

    #[test]
    fn parameters_other_than_format_1s_are_refused_before_deriving() {
        let record = Keyring::new().wrap(&passphrase("correct horse"));
        let with = |change: fn(&mut Kdf)| {
            let mut r = record.clone();
            change(&mut r.kdf);
            r.unwrap(&passphrase("correct horse")).unwrap_err()
        };
        assert_eq!(with(|k| k.memory_kib = 4 * 1024 * 1024), Unwrapped::Invalid, "4 GiB");
        assert_eq!(with(|k| k.memory_kib = 8), Unwrapped::Invalid, "too weak");
        assert_eq!(with(|k| k.iterations = 1000), Unwrapped::Invalid);
        assert_eq!(with(|k| k.parallelism = 64), Unwrapped::Invalid);
        assert_eq!(with(|k| k.salt = vec![0; 4]), Unwrapped::Invalid);
    }

    #[test]
    fn a_keyring_descends_only_from_its_own_past() {
        let first = Keyring::new();
        let second = first.next();
        let third = second.next();
        assert!(second.descends_from(&first));
        assert!(third.descends_from(&first));
        assert!(third.descends_from(&second));
        assert!(!first.descends_from(&first), "a descendant is newer");
        assert!(!first.descends_from(&second));
        let fork = first.next();
        assert!(!fork.descends_from(&second), "another device's change from the same generation");
        assert!(!second.descends_from(&fork));
        assert!(!Keyring::new().next().descends_from(&first), "another account");
        assert_eq!(Keyring::from_secret(&third.to_secret()).unwrap(), third);
    }

    #[test]
    fn a_passphrase_has_at_least_eight_characters() {
        assert!(Passphrase::new("1234567".to_owned()).is_none());
        assert!(Passphrase::new("ÿÿÿÿÿÿÿÿ".to_owned()).is_some(), "characters, not bytes");
    }
}
