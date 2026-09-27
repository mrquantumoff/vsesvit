//! CRX3 writer: the inverse of `extensions::crx::parse`, with each layer exposed so
//! tests can build files that are wrong in exactly one way.

use std::io::{Cursor, Write};

use rsa::pkcs8::{DecodePrivateKey, EncodePublicKey};
use rsa::signature::SignatureEncoding;
use rsa::signature::hazmat::PrehashSigner;
use sha2::{Digest, Sha256};

use crate::extensions::ExtensionId;
use crate::extensions::crx::{self, KeyProof, ProofAlgorithm};

const PROBE_KEY_PEM: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/keys/test-only-probe-key.pem"));
const SECOND_KEY_PEM: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/keys/test-only-second-key.pem"));

/// A key that signs CRX3 proofs.
pub enum CrxKey {
    Rsa(Box<rsa::RsaPrivateKey>),
    Ecdsa(p256::ecdsa::SigningKey),
}

impl CrxKey {
    /// The committed RSA test key the probe is signed with. Its id is `testkit::PROBE_ID`.
    pub fn probe() -> CrxKey {
        CrxKey::rsa_pem(PROBE_KEY_PEM)
    }

    /// A second committed RSA test key.
    pub fn second() -> CrxKey {
        CrxKey::rsa_pem(SECOND_KEY_PEM)
    }

    /// PKCS#8 PEM. Text before the `-----BEGIN` line (the test-only label) is skipped.
    /// Panics on a malformed key: this is a test helper.
    pub fn rsa_pem(pem: &str) -> CrxKey {
        let start = pem.find("-----BEGIN").expect("a PEM block");
        CrxKey::Rsa(Box::new(rsa::RsaPrivateKey::from_pkcs8_pem(&pem[start..]).expect("a PKCS#8 RSA key")))
    }

    /// A deterministic P-256 key; different seeds give different keys.
    pub fn ecdsa(seed: u8) -> CrxKey {
        let mut scalar = [0u8; 32];
        scalar[0] = 1;
        scalar[31] = seed;
        CrxKey::Ecdsa(p256::ecdsa::SigningKey::from_slice(&scalar).expect("a scalar in 1..n"))
    }

    pub fn algorithm(&self) -> ProofAlgorithm {
        match self {
            CrxKey::Rsa(_) => ProofAlgorithm::RsaSha256,
            CrxKey::Ecdsa(_) => ProofAlgorithm::EcdsaP256Sha256,
        }
    }

    /// SubjectPublicKeyInfo DER, as CRX proofs and the manifest `key` carry it.
    pub fn public_key_der(&self) -> Vec<u8> {
        let doc = match self {
            CrxKey::Rsa(key) => key.to_public_key().to_public_key_der(),
            CrxKey::Ecdsa(key) => key.verifying_key().to_public_key_der(),
        };
        doc.expect("public keys always encode").as_bytes().to_vec()
    }

    pub fn crx_id(&self) -> [u8; 16] {
        let hash = Sha256::digest(self.public_key_der());
        let mut id = [0u8; 16];
        id.copy_from_slice(&hash[..16]);
        id
    }

    pub fn extension_id(&self) -> ExtensionId {
        ExtensionId::from_public_key(&self.public_key_der())
    }

    /// SHA-256 of the SPKI DER: what the Web Store publisher check compares.
    pub fn key_sha256(&self) -> [u8; 32] {
        Sha256::digest(self.public_key_der()).into()
    }

    /// A proof over the CRX3 signed-message digest.
    pub fn proof(&self, digest: &[u8; 32]) -> KeyProof {
        let signature = match self {
            CrxKey::Rsa(key) => {
                let signer = rsa::pkcs1v15::SigningKey::<Sha256>::new((**key).clone());
                signer.sign_prehash(digest).expect("RSA signing").to_vec()
            }
            CrxKey::Ecdsa(key) => {
                let sig: p256::ecdsa::Signature = key.sign_prehash(digest).expect("ECDSA signing");
                sig.to_der().as_bytes().to_vec()
            }
        };
        KeyProof { public_key: self.public_key_der(), signature }
    }
}

/// A deflated zip of `files` (`/`-separated names), byte-for-byte deterministic.
pub fn zip_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (name, data) in files {
        writer.start_file(*name, options).expect("zip entry");
        writer.write_all(data).expect("zip write to memory");
    }
    writer.finish().expect("zip finish").into_inner()
}

/// The CRX3 frame around `zip`, with arbitrary proofs and signed header data.
pub fn encode_crx3(rsa_proofs: &[KeyProof], ecdsa_proofs: &[KeyProof], signed_header_data: &[u8], zip: &[u8]) -> Vec<u8> {
    let mut header = Vec::new();
    for proof in rsa_proofs {
        put_bytes(&mut header, 2, &encode_proof(proof));
    }
    for proof in ecdsa_proofs {
        put_bytes(&mut header, 3, &encode_proof(proof));
    }
    put_bytes(&mut header, 10000, signed_header_data);

    let mut out = Vec::with_capacity(12 + header.len() + zip.len());
    out.extend_from_slice(crx::CRX_MAGIC);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(header.len()).expect("header fits in u32").to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(zip);
    out
}

/// A CRX3 of `zip` declaring `crx_id`, with one proof per signer.
pub fn sign_crx3(zip: &[u8], crx_id: [u8; 16], signers: &[&CrxKey]) -> Vec<u8> {
    let mut signed_header_data = Vec::new();
    put_bytes(&mut signed_header_data, 1, &crx_id);
    let digest = crx::signed_digest(&signed_header_data, zip);
    let proofs = |algorithm| signers.iter().filter(|k| k.algorithm() == algorithm).map(|k| k.proof(&digest)).collect::<Vec<_>>();
    encode_crx3(&proofs(ProofAlgorithm::RsaSha256), &proofs(ProofAlgorithm::EcdsaP256Sha256), &signed_header_data, zip)
}

/// A valid CRX3 of `files`, signed by `key`, whose id derives from `key`.
pub fn write_crx3(files: &[(&str, &[u8])], key: &CrxKey) -> Vec<u8> {
    sign_crx3(&zip_files(files), key.crx_id(), &[key])
}

fn encode_proof(proof: &KeyProof) -> Vec<u8> {
    let mut out = Vec::new();
    put_bytes(&mut out, 1, &proof.public_key);
    put_bytes(&mut out, 2, &proof.signature);
    out
}

fn put_bytes(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    put_varint(out, u64::from(field) << 3 | 2);
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}
