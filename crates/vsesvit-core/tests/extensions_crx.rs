//! CRX3 parse + verify against files written by `testkit`: a round trip, then files that
//! are wrong in exactly one way. Run with `--features testkit`.
#![cfg(feature = "testkit")]

use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::extensions::crx::{self, CrxError, CrxStore, ProofAlgorithm, VerifyPolicy};
use vsesvit_core::testkit::{self, CrxKey, encode_crx3, sign_crx3, write_crx3, zip_files};

const FILES: &[(&str, &[u8])] =
    &[("manifest.json", br#"{"manifest_version": 3, "name": "T", "version": "1.0"}"#), ("js/a.js", b"console.log('a');")];

fn header_len(crx: &[u8]) -> usize {
    u32::from_le_bytes(crx[8..12].try_into().unwrap()) as usize
}

/// Re-frames `crx` with `extra` protobuf bytes appended to its (unsigned) header.
fn with_header_suffix(crx: &[u8], extra: &[u8]) -> Vec<u8> {
    let len = header_len(crx);
    let mut out = crx[..8].to_vec();
    out.extend_from_slice(&u32::try_from(len + extra.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&crx[12..12 + len]);
    out.extend_from_slice(extra);
    out.extend_from_slice(&crx[12 + len..]);
    out
}

fn verify(bytes: &[u8], policy: &VerifyPolicy) -> Result<crx::VerifiedCrx, CrxError> {
    crx::verify(&crx::parse(bytes)?, policy)
}

#[test]
fn probe_key_derives_to_probe_id() {
    assert_eq!(CrxKey::probe().extension_id().as_str(), testkit::PROBE_ID);
    let verified = verify(&testkit::probe_crx(), &VerifyPolicy::AnyDeveloperKey).unwrap();
    assert_eq!(verified.id.as_str(), testkit::PROBE_ID);
    assert_eq!(testkit::probe_crx(), testkit::probe_crx(), "probe_crx() is deterministic");
}

#[test]
fn round_trip_rsa() {
    let key = CrxKey::probe();
    let bytes = write_crx3(FILES, &key);
    let parsed = crx::parse(&bytes).unwrap();
    assert_eq!(parsed.zip, zip_files(FILES).as_slice());
    assert_eq!(parsed.crx_id, key.crx_id());
    assert_eq!((parsed.rsa_proofs.len(), parsed.ecdsa_proofs.len()), (1, 0));

    let verified = crx::verify(&parsed, &VerifyPolicy::AnyDeveloperKey).unwrap();
    assert_eq!(verified.id, key.extension_id());
    assert_eq!(verified.developer_key, key.public_key_der());
    assert!(!verified.publisher_verified);
}

#[test]
fn round_trip_ecdsa_developer_key_and_mixed_proofs() {
    let dev = CrxKey::ecdsa(7);
    let verified = verify(&write_crx3(FILES, &dev), &VerifyPolicy::AnyDeveloperKey).unwrap();
    assert_eq!(verified.id, dev.extension_id());

    let rsa_dev = CrxKey::probe();
    let (second, ecdsa) = (CrxKey::second(), CrxKey::ecdsa(3));
    let bytes = sign_crx3(&zip_files(FILES), rsa_dev.crx_id(), &[&rsa_dev, &second, &ecdsa]);
    let parsed = crx::parse(&bytes).unwrap();
    assert_eq!((parsed.rsa_proofs.len(), parsed.ecdsa_proofs.len()), (2, 1));
    assert_eq!(crx::verify(&parsed, &VerifyPolicy::AnyDeveloperKey).unwrap().developer_key, rsa_dev.public_key_der());
}

#[test]
fn web_store_policy_needs_the_expected_id_and_a_publisher_proof() {
    let dev = CrxKey::probe();
    let publisher = CrxKey::ecdsa(42);
    let store = VerifyPolicy::WebStore { store: CrxStore::ChromeWebStore, expected: dev.extension_id() };

    let unpublished = write_crx3(FILES, &dev);
    assert!(matches!(verify(&unpublished, &store), Err(CrxError::NoPublisherProof(CrxStore::ChromeWebStore))));

    // The real publisher key is Google's; a key we hold stands in for it here.
    let published = sign_crx3(&zip_files(FILES), dev.crx_id(), &[&dev, &publisher]);
    let parsed = crx::parse(&published).unwrap();
    let verified = crx::verify_with_publisher(&parsed, &store, &publisher.key_sha256()).unwrap();
    assert!(verified.publisher_verified);
    assert_eq!(verified.id, dev.extension_id());
    assert!(matches!(crx::verify(&parsed, &store), Err(CrxError::NoPublisherProof(_))), "only the pinned key counts");
}

#[test]
fn edge_add_ons_policy_needs_the_edge_publisher_proof() {
    let dev = CrxKey::probe();
    let publisher = CrxKey::ecdsa(43);
    let edge = VerifyPolicy::WebStore { store: CrxStore::EdgeAddons, expected: dev.extension_id() };
    let chrome = VerifyPolicy::WebStore { store: CrxStore::ChromeWebStore, expected: dev.extension_id() };

    let unpublished = write_crx3(FILES, &dev);
    let refused = verify(&unpublished, &edge).unwrap_err();
    assert!(matches!(refused, CrxError::NoPublisherProof(CrxStore::EdgeAddons)));
    assert_eq!(refused.to_string(), "no valid Edge Add-ons publisher signature");

    // The real publisher key is Microsoft's; a key we hold stands in for it here.
    let published = sign_crx3(&zip_files(FILES), dev.crx_id(), &[&dev, &publisher]);
    let parsed = crx::parse(&published).unwrap();
    let verified = crx::verify_with_publisher(&parsed, &edge, &publisher.key_sha256()).unwrap();
    assert!(verified.publisher_verified);
    assert_eq!(verified.id, dev.extension_id());
    assert!(matches!(crx::verify(&parsed, &edge), Err(CrxError::NoPublisherProof(CrxStore::EdgeAddons))), "only the pinned key counts");
    assert_eq!(CrxStore::EdgeAddons.publisher_key_sha256(), &crx::EDGE_PUBLISHER_KEY_SHA256);
    assert_ne!(CrxStore::EdgeAddons.publisher_key_sha256(), CrxStore::ChromeWebStore.publisher_key_sha256());
    assert!(matches!(crx::verify(&parsed, &chrome), Err(CrxError::NoPublisherProof(CrxStore::ChromeWebStore))));
}

#[test]
fn flipped_zip_byte_fails_every_proof() {
    let mut bytes = write_crx3(FILES, &CrxKey::probe());
    let zip_start = 12 + header_len(&bytes);
    bytes[zip_start + 40] ^= 0x01;
    assert!(matches!(verify(&bytes, &VerifyPolicy::AnyDeveloperKey), Err(CrxError::InvalidProof(ProofAlgorithm::RsaSha256))));
}

#[test]
fn wrong_expected_id_is_rejected() {
    let bytes = write_crx3(FILES, &CrxKey::probe());
    let policy = VerifyPolicy::WebStore { store: CrxStore::ChromeWebStore, expected: CrxKey::second().extension_id() };
    match verify(&bytes, &policy) {
        Err(CrxError::WrongId { expected, actual }) => {
            assert_eq!(expected, CrxKey::second().extension_id().as_str());
            assert_eq!(actual, testkit::PROBE_ID);
        }
        other => panic!("expected WrongId, got {other:?}"),
    }
}

#[test]
fn zip_end_of_central_directory_magic_in_the_header_is_rejected() {
    let bytes = write_crx3(FILES, &CrxKey::probe());
    // Field 50, length-delimited: unknown to the parser, and outside the signed data.
    for magic in [b"PK\x05\x06", b"PK\x06\x06", b"PK\x06\x07"] {
        let mut field = vec![0x92, 0x03, 4];
        field.extend_from_slice(magic);
        let tampered = with_header_suffix(&bytes, &field);
        assert!(matches!(crx::parse(&tampered), Err(CrxError::ZipMagicInHeader)), "{magic:?}");
    }
    // The same unknown field without the magic is skipped and the signature still holds.
    let harmless = with_header_suffix(&bytes, &[0x92, 0x03, 4, b'a', b'b', b'c', b'd']);
    assert!(verify(&harmless, &VerifyPolicy::AnyDeveloperKey).is_ok());
}

#[test]
fn missing_developer_proof_is_rejected() {
    let (dev, other) = (CrxKey::probe(), CrxKey::second());
    let zip = zip_files(FILES);
    // Validly signed, but by a key the declared id does not derive from.
    let bytes = sign_crx3(&zip, dev.crx_id(), &[&other]);
    assert!(matches!(verify(&bytes, &VerifyPolicy::AnyDeveloperKey), Err(CrxError::NoDeveloperProof)));
    let unsigned = sign_crx3(&zip, dev.crx_id(), &[]);
    assert!(matches!(verify(&unsigned, &VerifyPolicy::AnyDeveloperKey), Err(CrxError::NoDeveloperProof)));
}

#[test]
fn an_invalid_second_proof_fails_the_file() {
    let (dev, second, ecdsa) = (CrxKey::probe(), CrxKey::second(), CrxKey::ecdsa(5));
    let bytes = sign_crx3(&zip_files(FILES), dev.crx_id(), &[&dev, &second, &ecdsa]);
    let parsed = crx::parse(&bytes).unwrap();

    let mut rsa = parsed.rsa_proofs.clone();
    let last = rsa[1].signature.len() - 1;
    rsa[1].signature[last] ^= 0x01;
    let tampered = encode_crx3(&rsa, &parsed.ecdsa_proofs, parsed.signed_header_data, parsed.zip);
    assert!(matches!(verify(&tampered, &VerifyPolicy::AnyDeveloperKey), Err(CrxError::InvalidProof(ProofAlgorithm::RsaSha256))));

    let mut ecdsa_proofs = parsed.ecdsa_proofs.clone();
    ecdsa_proofs[0].signature = CrxKey::ecdsa(6).proof(&[0; 32]).signature;
    let tampered = encode_crx3(&parsed.rsa_proofs, &ecdsa_proofs, parsed.signed_header_data, parsed.zip);
    assert!(matches!(verify(&tampered, &VerifyPolicy::AnyDeveloperKey), Err(CrxError::InvalidProof(ProofAlgorithm::EcdsaP256Sha256))));

    let mut garbage_key = parsed.rsa_proofs.clone();
    garbage_key[1].public_key = b"not a key".to_vec();
    let tampered = encode_crx3(&garbage_key, &parsed.ecdsa_proofs, parsed.signed_header_data, parsed.zip);
    assert!(matches!(verify(&tampered, &VerifyPolicy::AnyDeveloperKey), Err(CrxError::InvalidProof(ProofAlgorithm::RsaSha256))));
}

#[test]
fn a_substituted_crx_id_breaks_the_signature() {
    let dev = CrxKey::probe();
    let parsed_bytes = write_crx3(FILES, &dev);
    let parsed = crx::parse(&parsed_bytes).unwrap();
    let mut forged = parsed.signed_header_data.to_vec();
    let n = forged.len();
    forged[n - 1] ^= 0x01;
    let tampered = encode_crx3(&parsed.rsa_proofs, &[], &forged, parsed.zip);
    assert!(matches!(verify(&tampered, &VerifyPolicy::AnyDeveloperKey), Err(CrxError::InvalidProof(_))));
}

#[test]
fn other_formats_are_rejected() {
    let mut crx2 = write_crx3(FILES, &CrxKey::probe());
    crx2[4] = 2;
    assert!(matches!(crx::parse(&crx2), Err(CrxError::UnsupportedVersion(2))));
    assert!(matches!(crx::parse(&zip_files(FILES)), Err(CrxError::BadMagic)));
    let truncated = write_crx3(FILES, &CrxKey::probe());
    assert!(matches!(crx::parse(&truncated[..20]), Err(CrxError::Malformed)));
    assert!(ExtensionId::parse(testkit::PROBE_ID).unwrap().is_chrome_style());
}
