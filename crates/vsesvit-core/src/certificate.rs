//! X.509 certificates as the connection popup shows them: who the certificate is issued
//! to and by, when it is valid, the names it covers and its SHA-256 fingerprint.
//!
//! The shells get DER bytes from the engine (WebView2's `Network.getCertificate`, WebKit's
//! `GTlsCertificate`); [`parse_der`] is the boundary that turns them into plain values.

use sha2::{Digest, Sha256};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate, X509Name};
use x509_parser::public_key::PublicKey;
use x509_parser::x509::AttributeTypeAndValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    pub subject: Name,
    pub issuer: Name,
    pub not_before_unix: i64,
    pub not_after_unix: i64,
    /// The DNS names of the subject alternative name extension, in certificate order.
    pub dns_names: Vec<String>,
    /// Colon-separated uppercase hex of the serial number's bytes: `01:23:AB`.
    pub serial_hex: String,
    /// Colon-separated uppercase hex of the SHA-256 of the DER, as browsers show it.
    pub sha256_hex: String,
    /// `RSA 2048 bits`, `ECDSA P-256`, `Ed25519`; the algorithm's OID when unknown.
    pub public_key: String,
    /// `SHA-256 with RSA`, `ECDSA with SHA-384`; the algorithm's OID when unknown.
    pub signature_algorithm: String,
}

/// The parts of a distinguished name the popup shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Name {
    pub common_name: Option<String>,
    pub organization: Option<String>,
    pub organizational_unit: Option<String>,
    pub country: Option<String>,
}

impl Name {
    /// The common name, else the organization, else "Unknown".
    pub fn display(&self) -> String {
        self.common_name.as_ref().or(self.organization.as_ref()).cloned().unwrap_or_else(|| "Unknown".to_owned())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CertificateError {
    #[error("not a DER X.509 certificate: {0}")]
    Malformed(String),
    #[error("{0} bytes follow the certificate")]
    TrailingBytes(usize),
}

const SIGNATURE_ALGORITHMS: &[(&str, &str)] = &[
    ("1.2.840.113549.1.1.5", "SHA-1 with RSA"),
    ("1.2.840.113549.1.1.10", "RSA-PSS"),
    ("1.2.840.113549.1.1.11", "SHA-256 with RSA"),
    ("1.2.840.113549.1.1.12", "SHA-384 with RSA"),
    ("1.2.840.113549.1.1.13", "SHA-512 with RSA"),
    ("1.2.840.10045.4.1", "ECDSA with SHA-1"),
    ("1.2.840.10045.4.3.2", "ECDSA with SHA-256"),
    ("1.2.840.10045.4.3.3", "ECDSA with SHA-384"),
    ("1.2.840.10045.4.3.4", "ECDSA with SHA-512"),
    ("1.3.101.112", "Ed25519"),
    ("1.3.101.113", "Ed448"),
];

const CURVES: &[(&str, &str)] = &[("1.2.840.10045.3.1.7", "P-256"), ("1.3.132.0.34", "P-384"), ("1.3.132.0.35", "P-521")];

const RSA: &str = "1.2.840.113549.1.1.1";
const EC: &str = "1.2.840.10045.2.1";

fn lookup(table: &[(&str, &'static str)], oid: &str) -> Option<&'static str> {
    table.iter().find(|(o, _)| *o == oid).map(|(_, name)| *name)
}

/// Parses one DER certificate. Bytes after it are an error: the caller passed the wrong slice.
pub fn parse_der(der: &[u8]) -> Result<Certificate, CertificateError> {
    let (rest, cert) = X509Certificate::from_der(der).map_err(|e| CertificateError::Malformed(e.to_string()))?;
    if !rest.is_empty() {
        return Err(CertificateError::TrailingBytes(rest.len()));
    }
    let dns_names = match cert.subject_alternative_name() {
        Ok(Some(san)) => san
            .value
            .general_names
            .iter()
            .filter_map(|n| match n {
                GeneralName::DNSName(dns) => Some((*dns).to_owned()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let signature = cert.signature_algorithm.algorithm.to_id_string();
    Ok(Certificate {
        subject: name(cert.subject()),
        issuer: name(cert.issuer()),
        not_before_unix: cert.validity().not_before.timestamp(),
        not_after_unix: cert.validity().not_after.timestamp(),
        dns_names,
        serial_hex: colon_hex(cert.raw_serial()),
        sha256_hex: colon_hex(&Sha256::digest(der)),
        public_key: public_key(&cert),
        signature_algorithm: lookup(SIGNATURE_ALGORITHMS, &signature).map_or(signature, str::to_owned),
    })
}

fn name(n: &X509Name<'_>) -> Name {
    Name {
        common_name: first(n.iter_common_name()),
        organization: first(n.iter_organization()),
        organizational_unit: first(n.iter_organizational_unit()),
        country: first(n.iter_country()),
    }
}

fn first<'x, 'a: 'x>(mut attrs: impl Iterator<Item = &'x AttributeTypeAndValue<'a>>) -> Option<String> {
    attrs.find_map(|a| a.as_str().ok().map(str::to_owned))
}

fn public_key(cert: &X509Certificate<'_>) -> String {
    let spki = cert.public_key();
    let oid = spki.algorithm.algorithm.to_id_string();
    match oid.as_str() {
        RSA => match spki.parsed() {
            Ok(PublicKey::RSA(key)) => format!("RSA {} bits", key.key_size()),
            _ => "RSA".to_owned(),
        },
        EC => {
            let curve = spki.algorithm.parameters.as_ref().and_then(|p| p.as_oid().ok()).map(|o| o.to_id_string());
            match curve {
                Some(curve) => format!("ECDSA {}", lookup(CURVES, &curve).map_or(curve, str::to_owned)),
                None => "ECDSA".to_owned(),
            }
        }
        _ => lookup(SIGNATURE_ALGORITHMS, &oid).map_or(oid, str::to_owned),
    }
}

fn colon_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &[u8] = include_bytes!("../tests/fixtures/certificates/root.der");
    const LEAF: &[u8] = include_bytes!("../tests/fixtures/certificates/leaf.der");

    // Expected values are `openssl x509 -inform DER -noout -text -fingerprint -sha256` of the fixtures.
    #[test]
    fn parses_an_ecdsa_leaf_signed_by_an_rsa_root() {
        let root_name = Name {
            common_name: Some("Vsesvit Test Root".into()),
            organization: Some("Vsesvit Test".into()),
            organizational_unit: Some("Fixtures".into()),
            country: Some("UA".into()),
        };
        assert_eq!(
            parse_der(LEAF).unwrap(),
            Certificate {
                subject: Name {
                    common_name: Some("www.example.test".into()),
                    organization: Some("Example Org".into()),
                    ..Name::default()
                },
                issuer: root_name.clone(),
                not_before_unix: 1_767_225_600,
                not_after_unix: 1_798_761_600,
                dns_names: vec!["www.example.test".into(), "example.test".into()],
                serial_hex: "01:23:45:67:89:AB:CD:EF".into(),
                sha256_hex: "92:EE:97:79:67:B7:15:1A:A7:4D:97:95:24:B7:3E:13:EC:C1:06:D1:52:0B:CC:68:6E:19:9F:8C:FE:4B:5B:EF"
                    .into(),
                public_key: "ECDSA P-256".into(),
                signature_algorithm: "SHA-256 with RSA".into(),
            }
        );

        let root = parse_der(ROOT).unwrap();
        assert_eq!((&root.subject, &root.issuer), (&root_name, &root_name));
        assert_eq!((root.not_before_unix, root.not_after_unix), (1_735_689_600, 2_366_841_600));
        assert_eq!(root.dns_names, Vec::<String>::new());
        assert_eq!(root.serial_hex, "01");
        assert_eq!(
            root.sha256_hex,
            "E2:CC:AA:B1:05:F8:7C:33:FA:FA:07:37:5D:54:43:C2:04:88:C7:0F:B7:E2:4D:03:D0:7A:9D:2D:B6:D2:23:FC"
        );
        assert_eq!(root.public_key, "RSA 2048 bits");
    }

    #[test]
    fn names_display_the_common_name_else_the_organization() {
        let org = Name { organization: Some("Org".into()), ..Name::default() };
        assert_eq!(Name { common_name: Some("CN".into()), ..org.clone() }.display(), "CN");
        assert_eq!(org.display(), "Org");
        assert_eq!(Name::default().display(), "Unknown");
    }

    #[test]
    fn rejects_what_is_not_exactly_one_certificate() {
        assert!(matches!(parse_der(b"not a certificate"), Err(CertificateError::Malformed(_))));
        assert!(matches!(parse_der(&LEAF[..LEAF.len() - 1]), Err(CertificateError::Malformed(_))));
        let with_trailing = [LEAF, b"\0\0"].concat();
        assert!(matches!(parse_der(&with_trailing), Err(CertificateError::TrailingBytes(2))));
    }
}
