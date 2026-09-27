//! The protocol's pure logic, matched to `tauri-plugin-updater` 2.12 `updater.rs`: endpoint
//! templating, response parsing, platform-key lookup and signature checks.

use std::collections::HashMap;

use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::Deserialize;
use time::OffsetDateTime;
use url::Url;

use crate::{ARCH, DisabledReason, Error, TARGET};

/// Tauri substitutes `unknown` for `{{bundle_type}}` when the app is not a known bundle.
const UNKNOWN_BUNDLE_TYPE: &str = "unknown";

pub(crate) struct Release {
    pub version: Version,
    pub notes: Option<String>,
    pub pub_date: Option<OffsetDateTime>,
    artifacts: Artifacts,
}

enum Artifacts {
    Dynamic(Artifact),
    Static(HashMap<String, Artifact>),
}

#[derive(Deserialize)]
pub(crate) struct Artifact {
    pub url: Url,
    pub signature: String,
}

/// Fills in an endpoint. `Url` percent-encodes braces in the path but not in the query, so both
/// spellings of each placeholder are replaced.
pub(crate) fn endpoint_url(template: &Url, current: &Version, variant: Option<&str>) -> String {
    // Tauri percent-encodes the version with CONTROLS + '+'; semver has no control characters.
    let version = current.to_string().replace('+', "%2B");
    let bundle_type = variant.unwrap_or(UNKNOWN_BUNDLE_TYPE);
    let mut url = template.to_string();
    for (name, value) in
        [("current_version", version.as_str()), ("target", TARGET), ("arch", ARCH), ("bundle_type", bundle_type)]
    {
        url = url.replace(&format!("%7B%7B{name}%7D%7D"), value).replace(&format!("{{{{{name}}}}}"), value);
    }
    url
}

pub(crate) fn parse_release(body: &[u8]) -> Result<Release, Error> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(alias = "name", deserialize_with = "version_with_optional_v")]
        version: Version,
        notes: Option<String>,
        pub_date: Option<String>,
        platforms: Option<HashMap<String, Artifact>>,
        url: Option<Url>,
        signature: Option<String>,
    }

    let raw: Raw = serde_json::from_slice(body).map_err(|e| Error::BadResponse(e.to_string()))?;
    let pub_date = raw
        .pub_date
        .map(|date| OffsetDateTime::parse(&date, &time::format_description::well_known::Rfc3339))
        .transpose()
        .map_err(|e| Error::BadResponse(format!("invalid pub_date: {e}")))?;
    let artifacts = match (raw.platforms, raw.url, raw.signature) {
        (Some(platforms), _, _) => Artifacts::Static(platforms),
        (None, Some(url), Some(signature)) => Artifacts::Dynamic(Artifact { url, signature }),
        (None, None, _) => return Err(Error::BadResponse("neither `platforms` nor `url` is set".into())),
        (None, Some(_), None) => return Err(Error::BadResponse("`signature` is not set".into())),
    };
    Ok(Release { version: raw.version, notes: raw.notes, pub_date, artifacts })
}

fn version_with_optional_v<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Version, D::Error> {
    let version = String::deserialize(deserializer)?;
    Version::parse(version.trim_start_matches('v')).map_err(serde::de::Error::custom)
}

impl Release {
    /// A static manifest is searched for `<target>-<arch>-<variant>`, then `<target>-<arch>`.
    pub fn artifact(&self, variant: Option<&str>) -> Result<&Artifact, Error> {
        let platforms = match &self.artifacts {
            Artifacts::Dynamic(artifact) => return Ok(artifact),
            Artifacts::Static(platforms) => platforms,
        };
        let keys: Vec<String> = variant
            .map(|variant| format!("{TARGET}-{ARCH}-{variant}"))
            .into_iter()
            .chain([format!("{TARGET}-{ARCH}")])
            .collect();
        keys.iter().find_map(|key| platforms.get(key)).ok_or(Error::NoArtifactForTarget(keys))
    }
}

pub(crate) fn decode_public_key(pubkey: &str) -> Result<PublicKey, DisabledReason> {
    if pubkey.is_empty() {
        return Err(DisabledReason::NoPublicKey);
    }
    let text = base64_text(pubkey).map_err(DisabledReason::InvalidPublicKey)?;
    PublicKey::decode(&text).map_err(|e| DisabledReason::InvalidPublicKey(e.to_string()))
}

/// Checks `data` against `signature` (base64 of a minisign `.sig` file), then requires the
/// signed trusted comment to carry `version:<announced>`. Without that second check a tampered
/// response could pair a new version number with an older, genuinely signed release.
pub(crate) fn verify(data: &[u8], signature: &str, key: &PublicKey, announced: &Version) -> Result<(), Error> {
    let text = base64_text(signature).map_err(Error::Signature)?;
    let signature = Signature::decode(&text).map_err(|e| Error::Signature(e.to_string()))?;
    key.verify(data, &signature, true).map_err(|e| Error::Signature(e.to_string()))?;

    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .ok_or_else(|| Error::Signature("the signature does not name the version it was made for".into()))?;
    match Version::parse(signed.trim_start_matches('v')) {
        Ok(version) if version == *announced => Ok(()),
        _ => Err(Error::SignedVersionMismatch { signed: signed.to_owned(), announced: announced.clone() }),
    }
}

fn base64_text(encoded: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).map_err(|e| format!("base64: {e}"))?;
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templating_replaces_raw_and_percent_encoded_placeholders() {
        let template = Url::parse(
            "https://u.test/{{target}}/{{arch}}/{{current_version}}/{{bundle_type}}?v={{current_version}}&t={{target}}&a={{arch}}&b={{bundle_type}}",
        )
        .unwrap();
        assert!(template.as_str().contains("%7B%7Btarget%7D%7D"), "the path form is percent-encoded: {template}");
        let url = endpoint_url(&template, &Version::parse("1.2.3-beta.1+build.7").unwrap(), Some("deb"));
        assert_eq!(
            url,
            format!(
                "https://u.test/{TARGET}/{ARCH}/1.2.3-beta.1%2Bbuild.7/deb?v=1.2.3-beta.1%2Bbuild.7&t={TARGET}&a={ARCH}&b=deb"
            )
        );
    }

    #[test]
    fn unpackaged_bundle_type_is_unknown() {
        let template = Url::parse("https://u.test/x?variant={{bundle_type}}").unwrap();
        assert_eq!(endpoint_url(&template, &Version::new(1, 0, 0), None), "https://u.test/x?variant=unknown");
    }

    #[test]
    fn static_lookup_prefers_the_variant_key() {
        let body = format!(
            r#"{{"version": "1.0.0", "platforms": {{
                "{TARGET}-{ARCH}-deb": {{"url": "https://u.test/deb", "signature": "s1"}},
                "{TARGET}-{ARCH}": {{"url": "https://u.test/any", "signature": "s2"}}
            }}}}"#
        );
        let release = parse_release(body.as_bytes()).unwrap();
        assert_eq!(release.artifact(Some("deb")).unwrap().url.as_str(), "https://u.test/deb");
        assert_eq!(release.artifact(Some("rpm")).unwrap().url.as_str(), "https://u.test/any");
        assert_eq!(release.artifact(None).unwrap().url.as_str(), "https://u.test/any");
    }

    #[test]
    fn static_lookup_names_every_key_it_tried() {
        let release = parse_release(br#"{"version": "1.0.0", "platforms": {}}"#).unwrap();
        let Err(Error::NoArtifactForTarget(keys)) = release.artifact(Some("rpm")) else { panic!("no artifact expected") };
        assert_eq!(keys, [format!("{TARGET}-{ARCH}-rpm"), format!("{TARGET}-{ARCH}")]);
    }

    #[test]
    fn dynamic_needs_url_and_signature() {
        assert!(matches!(parse_release(br#"{"version": "1.0.0", "signature": "s"}"#), Err(Error::BadResponse(_))));
        assert!(matches!(
            parse_release(br#"{"version": "1.0.0", "url": "https://u.test/a"}"#),
            Err(Error::BadResponse(_))
        ));
    }

    #[test]
    fn version_takes_a_leading_v_and_the_name_alias() {
        let release = parse_release(br#"{"name": "v2.0.0", "url": "https://u.test/a", "signature": "s"}"#).unwrap();
        assert_eq!(release.version, Version::new(2, 0, 0));
    }

    #[test]
    fn empty_and_malformed_keys_are_typed() {
        assert_eq!(decode_public_key("").unwrap_err(), DisabledReason::NoPublicKey);
        assert!(matches!(decode_public_key("!!!").unwrap_err(), DisabledReason::InvalidPublicKey(_)));
        assert!(matches!(decode_public_key("aGVsbG8=").unwrap_err(), DisabledReason::InvalidPublicKey(_)));
    }
}
