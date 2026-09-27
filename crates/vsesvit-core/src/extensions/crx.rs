//! CRX3 parsing and verification. Pure functions over bytes, no I/O.
//!
//! ```text
//! "Cr24" | u32le version = 3 | u32le header_len | CrxFileHeader (protobuf, header_len bytes) | zip
//!
//! message CrxFileHeader {
//!   repeated AsymmetricKeyProof sha256_with_rsa   = 2;
//!   repeated AsymmetricKeyProof sha256_with_ecdsa = 3;
//!   optional bytes signed_header_data             = 10000;   // SignedData { optional bytes crx_id = 1; }
//! }
//! message AsymmetricKeyProof { optional bytes public_key = 1; optional bytes signature = 2; }
//!
//! signed message = "CRX3 SignedData\x00" ++ u32le(len(signed_header_data)) ++ signed_header_data ++ zip
//! ```
//!
//! The rules are Chromium's (`components/crx_file/crx_verifier.cc`): every proof must
//! verify, one proof key must hash to `crx_id`, and store installs also need a proof by
//! the Web Store publisher key. The header itself is not signed, so a header that
//! contains zip end-of-central-directory magic is rejected: a zip reader that scans
//! backwards could otherwise be pointed at attacker-chosen bytes.
//!
//! No protobuf crate: the header uses varints and length-delimited fields only, which
//! `ProtoReader` covers in a few lines. Unknown fields are skipped, as protobuf requires.

use rsa::pkcs8::DecodePublicKey;
use rsa::signature::hazmat::PrehashVerifier;
use sha2::{Digest, Sha256};

use super::ExtensionId;

pub const CRX_MAGIC: &[u8; 4] = b"Cr24";

/// SHA-256 of the Chrome Web Store publisher key's SPKI DER
/// (`61f7f2a6bfcf74cd0bc1fe2497cc9b04254c658f79f2145392867ea8366367cf`, Chromium's
/// `kPublisherKeyHash`).
pub const CWS_PUBLISHER_KEY_SHA256: [u8; 32] = [
    0x61, 0xf7, 0xf2, 0xa6, 0xbf, 0xcf, 0x74, 0xcd, 0x0b, 0xc1, 0xfe, 0x24, 0x97, 0xcc, 0x9b, 0x04, 0x25, 0x4c, 0x65, 0x8f, 0x79, 0xf2,
    0x14, 0x53, 0x92, 0x86, 0x7e, 0xa8, 0x36, 0x63, 0x67, 0xcf,
];

const SIGNED_DATA_CONTEXT: &[u8] = b"CRX3 SignedData\x00";
const ZIP_MAGICS: [&[u8; 4]; 3] = [b"PK\x05\x06", b"PK\x06\x06", b"PK\x06\x07"];

const FIELD_RSA_PROOF: u32 = 2;
const FIELD_ECDSA_PROOF: u32 = 3;
const FIELD_SIGNED_HEADER_DATA: u32 = 10000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyProof {
    /// SubjectPublicKeyInfo DER.
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProofAlgorithm {
    /// RSASSA-PKCS1-v1_5 with SHA-256.
    RsaSha256,
    /// ECDSA on P-256 with SHA-256, DER-encoded signature.
    EcdsaP256Sha256,
}

#[derive(Clone, Debug)]
pub struct Crx3<'a> {
    pub rsa_proofs: Vec<KeyProof>,
    pub ecdsa_proofs: Vec<KeyProof>,
    pub signed_header_data: &'a [u8],
    /// `SignedData.crx_id`: 16 bytes.
    pub crx_id: [u8; 16],
    pub zip: &'a [u8],
}

impl Crx3<'_> {
    pub fn proofs(&self) -> impl Iterator<Item = (ProofAlgorithm, &KeyProof)> {
        let rsa = self.rsa_proofs.iter().map(|p| (ProofAlgorithm::RsaSha256, p));
        let ecdsa = self.ecdsa_proofs.iter().map(|p| (ProofAlgorithm::EcdsaP256Sha256, p));
        rsa.chain(ecdsa)
    }

    /// SHA-256 of the signed message. Every proof signs this same message.
    pub fn signed_digest(&self) -> [u8; 32] {
        signed_digest(self.signed_header_data, self.zip)
    }
}

pub(crate) fn signed_digest(signed_header_data: &[u8], zip: &[u8]) -> [u8; 32] {
    let len = u32::try_from(signed_header_data.len()).expect("header length was read from a u32");
    let mut h = Sha256::new();
    h.update(SIGNED_DATA_CONTEXT);
    h.update(len.to_le_bytes());
    h.update(signed_header_data);
    h.update(zip);
    h.finalize().into()
}

pub fn parse(bytes: &[u8]) -> Result<Crx3<'_>, CrxError> {
    let rest = bytes.strip_prefix(CRX_MAGIC).ok_or(CrxError::BadMagic)?;
    let (version, rest) = take_u32le(rest)?;
    if version != 3 {
        return Err(CrxError::UnsupportedVersion(version));
    }
    let (header_len, rest) = take_u32le(rest)?;
    let header_len = usize::try_from(header_len).map_err(|_| CrxError::Malformed)?;
    if header_len > rest.len() {
        return Err(CrxError::Malformed);
    }
    let (header, zip) = rest.split_at(header_len);
    if header.windows(4).any(|w| ZIP_MAGICS.iter().any(|m| w == m.as_slice())) {
        return Err(CrxError::ZipMagicInHeader);
    }

    let mut rsa_proofs = Vec::new();
    let mut ecdsa_proofs = Vec::new();
    let mut signed_header_data = None;
    for field in ProtoReader::new(header) {
        match field? {
            (FIELD_RSA_PROOF, ProtoValue::Bytes(b)) => rsa_proofs.push(parse_proof(b)?),
            (FIELD_ECDSA_PROOF, ProtoValue::Bytes(b)) => ecdsa_proofs.push(parse_proof(b)?),
            // Protobuf semantics for a repeated singular field: the last one wins.
            (FIELD_SIGNED_HEADER_DATA, ProtoValue::Bytes(b)) => signed_header_data = Some(b),
            (FIELD_RSA_PROOF | FIELD_ECDSA_PROOF | FIELD_SIGNED_HEADER_DATA, ProtoValue::Varint(_)) => {
                return Err(CrxError::Malformed);
            }
            _ => {}
        }
    }
    let signed_header_data = signed_header_data.ok_or(CrxError::Malformed)?;
    let crx_id = parse_signed_data(signed_header_data)?;
    Ok(Crx3 { rsa_proofs, ecdsa_proofs, signed_header_data, crx_id, zip })
}

fn take_u32le(b: &[u8]) -> Result<(u32, &[u8]), CrxError> {
    let (head, rest) = b.split_first_chunk::<4>().ok_or(CrxError::Malformed)?;
    Ok((u32::from_le_bytes(*head), rest))
}

fn parse_proof(b: &[u8]) -> Result<KeyProof, CrxError> {
    let mut proof = KeyProof { public_key: Vec::new(), signature: Vec::new() };
    for field in ProtoReader::new(b) {
        match field? {
            (1, ProtoValue::Bytes(k)) => proof.public_key = k.to_vec(),
            (2, ProtoValue::Bytes(s)) => proof.signature = s.to_vec(),
            (1 | 2, ProtoValue::Varint(_)) => return Err(CrxError::Malformed),
            _ => {}
        }
    }
    Ok(proof)
}

fn parse_signed_data(b: &[u8]) -> Result<[u8; 16], CrxError> {
    let mut crx_id = None;
    for field in ProtoReader::new(b) {
        match field? {
            (1, ProtoValue::Bytes(id)) => crx_id = Some(id),
            (1, ProtoValue::Varint(_)) => return Err(CrxError::Malformed),
            _ => {}
        }
    }
    crx_id.and_then(|id| <[u8; 16]>::try_from(id).ok()).ok_or(CrxError::Malformed)
}

#[derive(Clone, Debug)]
pub enum VerifyPolicy {
    /// Store download: the id must equal `expected`, and a valid proof by the Web Store
    /// publisher key must be present (as Chrome requires for store installs).
    WebStore { expected: ExtensionId },
    /// Local `.crx` file: a valid developer proof is enough.
    AnyDeveloperKey,
}

#[derive(Clone, Debug)]
pub struct VerifiedCrx {
    pub id: ExtensionId,
    /// Goes into `manifest.json` as `"key"`, base64.
    pub developer_key: Vec<u8>,
    /// A valid proof by the Web Store publisher key is present. Always true under
    /// [`VerifyPolicy::WebStore`], which requires it.
    pub publisher_verified: bool,
}

/// 1. Verify every proof, RSA and ECDSA, over the signed message. One bad proof fails
///    the whole file, as in Chromium.
/// 2. Developer proof: a proof whose `sha256(public_key)[..16] == crx_id`. Required.
/// 3. `WebStore`: `ExtensionId::from_crx_id(crx_id) == expected`, and a proof whose
///    `sha256(public_key) == CWS_PUBLISHER_KEY_SHA256`.
pub fn verify(crx: &Crx3<'_>, policy: &VerifyPolicy) -> Result<VerifiedCrx, CrxError> {
    verify_with_publisher(crx, policy, &CWS_PUBLISHER_KEY_SHA256)
}

/// [`verify`] with the publisher key hash as a parameter, so tests can stand in for
/// the Web Store with a key they hold.
#[doc(hidden)]
pub fn verify_with_publisher(crx: &Crx3<'_>, policy: &VerifyPolicy, publisher_key_sha256: &[u8; 32]) -> Result<VerifiedCrx, CrxError> {
    let digest = crx.signed_digest();
    let mut developer_key = None;
    let mut publisher_verified = false;
    for (algorithm, proof) in crx.proofs() {
        verify_proof(algorithm, proof, &digest)?;
        let key_hash: [u8; 32] = Sha256::digest(&proof.public_key).into();
        if developer_key.is_none() && key_hash[..16] == crx.crx_id {
            developer_key = Some(proof.public_key.clone());
        }
        publisher_verified |= key_hash == *publisher_key_sha256;
    }
    let developer_key = developer_key.ok_or(CrxError::NoDeveloperProof)?;
    let id = ExtensionId::from_crx_id(crx.crx_id);
    if let VerifyPolicy::WebStore { expected } = policy {
        if id != *expected {
            return Err(CrxError::WrongId { expected: expected.as_str().to_owned(), actual: id.as_str().to_owned() });
        }
        if !publisher_verified {
            return Err(CrxError::NoPublisherProof);
        }
    }
    Ok(VerifiedCrx { id, developer_key, publisher_verified })
}

fn verify_proof(algorithm: ProofAlgorithm, proof: &KeyProof, digest: &[u8; 32]) -> Result<(), CrxError> {
    let valid = match algorithm {
        ProofAlgorithm::RsaSha256 => {
            let Ok(key) = rsa::RsaPublicKey::from_public_key_der(&proof.public_key) else {
                return Err(CrxError::InvalidProof(algorithm));
            };
            let Ok(sig) = rsa::pkcs1v15::Signature::try_from(proof.signature.as_slice()) else {
                return Err(CrxError::InvalidProof(algorithm));
            };
            rsa::pkcs1v15::VerifyingKey::<Sha256>::new(key).verify_prehash(digest, &sig).is_ok()
        }
        ProofAlgorithm::EcdsaP256Sha256 => {
            let Ok(key) = p256::ecdsa::VerifyingKey::from_public_key_der(&proof.public_key) else {
                return Err(CrxError::InvalidProof(algorithm));
            };
            let Ok(sig) = p256::ecdsa::Signature::from_der(&proof.signature) else {
                return Err(CrxError::InvalidProof(algorithm));
            };
            key.verify_prehash(digest, &sig).is_ok()
        }
    };
    if valid { Ok(()) } else { Err(CrxError::InvalidProof(algorithm)) }
}

#[derive(Debug, thiserror::Error)]
pub enum CrxError {
    #[error("not a CRX file")]
    BadMagic,
    #[error("CRX version {0} is not supported (only CRX3)")]
    UnsupportedVersion(u32),
    #[error("truncated or malformed CRX header")]
    Malformed,
    #[error("the CRX header contains zip end-of-central-directory magic")]
    ZipMagicInHeader,
    #[error("a {0:?} signature in the CRX header does not verify")]
    InvalidProof(ProofAlgorithm),
    #[error("no valid signature by the key the extension id is derived from")]
    NoDeveloperProof,
    #[error("no valid Chrome Web Store publisher signature")]
    NoPublisherProof,
    #[error("CRX is for extension {actual}, expected {expected}")]
    WrongId { expected: String, actual: String },
}

/// Minimal protobuf wire reader: varint (type 0) and length-delimited (type 2) fields,
/// skipping 64-bit (1) and 32-bit (5). Groups (3, 4) are malformed: CRX never uses them.
pub(crate) struct ProtoReader<'a> {
    buf: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProtoValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

impl<'a> ProtoReader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        ProtoReader { buf }
    }

    fn varint(&mut self) -> Result<u64, CrxError> {
        let mut value = 0u64;
        for (i, &byte) in self.buf.iter().enumerate().take(10) {
            let bits = u64::from(byte & 0x7f);
            if i == 9 && bits > 1 {
                return Err(CrxError::Malformed);
            }
            value |= bits << (7 * i);
            if byte & 0x80 == 0 {
                self.buf = &self.buf[i + 1..];
                return Ok(value);
            }
        }
        Err(CrxError::Malformed)
    }

    fn take(&mut self, n: u64) -> Result<&'a [u8], CrxError> {
        let n = usize::try_from(n).map_err(|_| CrxError::Malformed)?;
        if n > self.buf.len() {
            return Err(CrxError::Malformed);
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    fn field(&mut self) -> Result<Option<(u32, ProtoValue<'a>)>, CrxError> {
        while !self.buf.is_empty() {
            let key = self.varint()?;
            let number = u32::try_from(key >> 3).ok().filter(|&n| n != 0).ok_or(CrxError::Malformed)?;
            match key & 7 {
                0 => return Ok(Some((number, ProtoValue::Varint(self.varint()?)))),
                1 => {
                    self.take(8)?;
                }
                2 => {
                    let len = self.varint()?;
                    return Ok(Some((number, ProtoValue::Bytes(self.take(len)?))));
                }
                5 => {
                    self.take(4)?;
                }
                _ => return Err(CrxError::Malformed),
            }
        }
        Ok(None)
    }
}

impl<'a> Iterator for ProtoReader<'a> {
    type Item = Result<(u32, ProtoValue<'a>), CrxError>;
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.field().transpose();
        if matches!(item, Some(Err(_))) {
            self.buf = &[];
        }
        item
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(buf: &[u8]) -> Result<Vec<(u32, ProtoValue<'_>)>, CrxError> {
        ProtoReader::new(buf).collect()
    }

    #[test]
    fn proto_reader_reads_varints_and_bytes_and_skips_fixed_width() {
        // field 1 varint 300, field 2 fixed64, field 10000 bytes "ab", field 3 fixed32
        let buf = [0x08, 0xac, 0x02, 0x11, 1, 2, 3, 4, 5, 6, 7, 8, 0x82, 0xf1, 0x04, 2, b'a', b'b', 0x1d, 9, 9, 9, 9];
        let got = fields(&buf).unwrap();
        assert_eq!(got, vec![(1, ProtoValue::Varint(300)), (10000, ProtoValue::Bytes(b"ab"))]);
    }

    #[test]
    fn proto_reader_rejects_malformed_input() {
        for bad in [
            &[0x0a, 5, 1][..],                                                   // length past the end
            &[0x08][..],                                                         // truncated varint
            &[0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f], // varint overflow
            &[0x0b][..],                                                         // group start
            &[0x02, 0][..],                                                      // field number 0
            &[0x09, 1, 2, 3][..],                                                // truncated fixed64
        ] {
            assert!(fields(bad).is_err(), "{bad:?} should be malformed");
        }
    }

    #[test]
    fn parse_rejects_bad_framing() {
        assert!(matches!(parse(b"PK\x03\x04"), Err(CrxError::BadMagic)));
        assert!(matches!(parse(b"Cr24\x02\0\0\0\0\0\0\0"), Err(CrxError::UnsupportedVersion(2))));
        assert!(matches!(parse(b"Cr24\x03\0\0\0\xff\0\0\0"), Err(CrxError::Malformed)));
        assert!(matches!(parse(b"Cr24\x03\0\0"), Err(CrxError::Malformed)));
        // A header with no signed_header_data.
        assert!(matches!(parse(b"Cr24\x03\0\0\0\0\0\0\0PK"), Err(CrxError::Malformed)));
    }

    #[test]
    fn publisher_key_hash_matches_the_published_hex() {
        let hex: String = CWS_PUBLISHER_KEY_SHA256.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "61f7f2a6bfcf74cd0bc1fe2497cc9b04254c658f79f2145392867ea8366367cf");
    }
}
