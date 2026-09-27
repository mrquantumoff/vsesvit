//! Writes `target/dist/latest.json` in Tauri's static format from the signed artifacts present.

use std::collections::BTreeMap;

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Result;
use crate::ctx::{Ctx, Format};
use crate::sign::sig_path;

pub const USAGE: &str = "cargo xtask manifest --base-url URL [--notes FILE] [--pub-date RFC3339] [--allow-unverified]";

/// Tauri's static update format. The Quadrant server's ingest requires every field, so
/// `notes` is always present and `pub_date` is always an RFC 3339 date.
#[derive(Serialize, Debug)]
struct LatestJson {
    version: String,
    notes: String,
    pub_date: String,
    platforms: BTreeMap<&'static str, Platform>,
}

#[derive(Serialize, Debug, PartialEq)]
struct Platform {
    signature: String,
    url: String,
}

/// A signed artifact in `target/dist`.
struct Signed {
    format: Format,
    file_name: String,
    signature: String,
}

pub fn run(ctx: &Ctx, args: &[String]) -> Result {
    let mut base_url = None;
    let mut notes = String::new();
    let mut pub_date = OffsetDateTime::now_utc();
    let mut allow_unverified = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(USAGE);
        match arg.as_str() {
            "--base-url" => base_url = Some(value()?.clone()),
            "--notes" => {
                let path = value()?;
                notes = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            }
            "--pub-date" => {
                let date = value()?;
                pub_date = OffsetDateTime::parse(date, &Rfc3339).map_err(|e| format!("--pub-date {date}: {e}"))?;
            }
            "--allow-unverified" => allow_unverified = true,
            _ => return Err(USAGE.to_owned()),
        }
    }
    let base_url = base_url.ok_or(USAGE)?;
    let pubkey = builtin_pubkey(ctx)?;
    if pubkey.is_empty() && !allow_unverified {
        return Err("packaging/updater.json has no pubkey, so installed copies could not verify this release; \
                    set it, or pass --allow-unverified for a local test"
            .to_owned());
    }

    let mut signed = Vec::new();
    for format in Format::ALL {
        let artifact = ctx.artifact(format);
        let Ok(signature) = std::fs::read_to_string(sig_path(&artifact)) else { continue };
        if !artifact.is_file() {
            return Err(format!("{} has a .sig but the file is missing", artifact.display()));
        }
        if !pubkey.is_empty() {
            let data = std::fs::read(&artifact).map_err(|e| format!("{}: {e}", artifact.display()))?;
            verify(&pubkey, &data, signature.trim(), &ctx.version).map_err(|e| format!("{}: {e}", artifact.display()))?;
        }
        signed.push(Signed {
            format,
            file_name: format.artifact_name(&ctx.version),
            signature: signature.trim().to_owned(),
        });
    }
    let manifest = build(&ctx.version, &base_url, notes, pub_date, &signed)?;
    let path = ctx.dist.join("latest.json");
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&path, json + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}

fn builtin_pubkey(ctx: &Ctx) -> Result<String> {
    let path = ctx.packaging().join("updater.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(config["pubkey"].as_str().unwrap_or_default().trim().to_owned())
}

/// Checks a signature the way installed copies will (vsesvit-update, after tauri-plugin-updater):
/// against the compiled-in key, with the signed `version:` equal to the release version. A
/// release that fails here would be downloaded and then rejected by every client.
fn verify(pubkey_b64: &str, data: &[u8], signature_b64: &str, version: &str) -> Result {
    use base64::Engine;
    let decode = |b64: &str, what: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| format!("{what} is not base64 text"))
    };
    let key = minisign_verify::PublicKey::decode(&decode(pubkey_b64, "the updater.json pubkey")?)
        .map_err(|e| format!("updater.json pubkey: {e}"))?;
    let signature =
        minisign_verify::Signature::decode(&decode(signature_b64, "the signature")?).map_err(|e| format!("signature: {e}"))?;
    key.verify(data, &signature, true)
        .map_err(|e| format!("signature does not verify with the updater.json pubkey ({e}); was it signed with another key?"))?;
    let signed = signature.trusted_comment().split('\t').find_map(|field| field.strip_prefix("version:"));
    if signed != Some(version) {
        return Err(format!("signed for version {signed:?}, releasing {version}"));
    }
    Ok(())
}

fn build(version: &str, base_url: &str, notes: String, pub_date: OffsetDateTime, signed: &[Signed]) -> Result<LatestJson> {
    let base_url = base_url.trim_end_matches('/');
    let mut platforms = BTreeMap::new();
    for artifact in signed {
        for key in artifact.format.manifest_keys() {
            let entry = Platform {
                signature: artifact.signature.clone(),
                url: format!("{base_url}/{}", percent_encode(&artifact.file_name)),
            };
            platforms.insert(*key, entry);
        }
    }
    if platforms.is_empty() {
        return Err(format!("no signed updater artifacts for {version} in target/dist (run `cargo xtask sign` first)"));
    }
    let pub_date = pub_date
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .map_err(|e| e.to_string())?
        .format(&Rfc3339)
        .map_err(|e| e.to_string())?;
    Ok(LatestJson { version: version.to_owned(), notes, pub_date, platforms })
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn percent_encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{DateTime, Utc};
    use serde::Deserialize;
    use time::macros::datetime;

    use super::*;

    #[test]
    fn only_signatures_installed_copies_accept_are_published() {
        let (sk, pk) = crate::sign::generate("").unwrap();
        let (_, other_pk) = crate::sign::generate("").unwrap();
        let key = crate::sign::secret_key(&sk, "").unwrap();
        let file = std::env::temp_dir().join(format!("xtask-manifest-verify-{}.bin", std::process::id()));
        std::fs::write(&file, b"artifact").unwrap();
        let sig = crate::sign::sign_with(&key, &file, "1.2.0-nightly.20260927.4").unwrap();
        std::fs::remove_file(&file).unwrap();

        verify(&pk, b"artifact", &sig, "1.2.0-nightly.20260927.4").expect("same key, same version");
        assert!(verify(&pk, b"tampered", &sig, "1.2.0-nightly.20260927.4").is_err(), "other bytes");
        assert!(verify(&other_pk, b"artifact", &sig, "1.2.0-nightly.20260927.4").is_err(), "other key");
        assert!(verify(&pk, b"artifact", &sig, "1.2.0").is_err(), "signed for another version");
    }

    /// The shape `quadrant_api/src/tauri_updates/submit_versions.rs` deserializes.
    #[derive(Deserialize)]
    struct LatestRelease {
        pub_date: DateTime<Utc>,
        notes: String,
        version: String,
        platforms: HashMap<String, PlatformInfo>,
    }

    #[derive(Deserialize)]
    struct PlatformInfo {
        signature: String,
        url: String,
    }

    fn signed(format: Format, version: &str) -> Signed {
        Signed { format, file_name: format.artifact_name(version), signature: format!("sig-{}", format.variant()) }
    }

    #[test]
    fn the_quadrant_server_ingests_the_manifest() {
        let artifacts = [signed(Format::Nsis, "1.2.3"), signed(Format::AppImage, "1.2.3"), signed(Format::Rpm, "1.2.3")];
        let manifest = build("1.2.3", "https://example.com/r/v1.2.3/", String::new(), datetime!(2026-09-27 12:30:15.5 +02:00), &artifacts).unwrap();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let release: LatestRelease = serde_json::from_str(&json).unwrap();

        assert_eq!(release.version, "1.2.3");
        assert_eq!(release.notes, "");
        assert_eq!(release.pub_date.to_rfc3339(), "2026-09-27T10:30:15+00:00");
        let mut keys: Vec<_> = release.platforms.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["linux-x86_64", "linux-x86_64-appimage", "linux-x86_64-rpm", "windows-x86_64", "windows-x86_64-nsis"]);
        for key in &keys {
            let parts: Vec<&str> = key.split('-').collect();
            assert!(matches!(parts.as_slice(), [_, "x86_64"] | [_, "x86_64", _]), "{key}");
        }
        let nsis = &release.platforms["windows-x86_64-nsis"];
        assert_eq!(nsis.url, "https://example.com/r/v1.2.3/Vsesvit_1.2.3_x64-setup.exe");
        assert_eq!(nsis.signature, "sig-nsis");
        assert_eq!(release.platforms["windows-x86_64"].url, nsis.url);
    }

    #[test]
    fn flatpak_is_never_published() {
        let err = build("1.0.0", "https://x", String::new(), OffsetDateTime::UNIX_EPOCH, &[signed(Format::Flatpak, "1.0.0")]);
        assert!(err.is_err());
    }

    #[test]
    fn file_names_are_percent_encoded() {
        assert_eq!(percent_encode("Vsesvit_1.0.0-rc+1 x.exe"), "Vsesvit_1.0.0-rc%2B1%20x.exe");
    }
}
