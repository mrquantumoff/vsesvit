//! Tauri-compatible minisign signing (docs/design/packaging.md, "Building packages").
//!
//! Keys use the Tauri CLI's encoding: the private key is base64 of a minisign secret key box,
//! the public key base64 of a minisign public key box, and a `.sig` file is base64 of the
//! minisign signature box. Keys made here work with `tauri signer` and the reverse.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign::{KeyPair, SecretKey, SecretKeyBox};

use crate::Result;

const KEY_ENV: &str = "TAURI_SIGNING_PRIVATE_KEY";
const PASSWORD_ENV: &str = "TAURI_SIGNING_PRIVATE_KEY_PASSWORD";

pub const GENERATE_USAGE: &str = "cargo xtask signer generate -w PRIVATE_KEY_FILE [-p PASSWORD] [--force]";

/// Signs `file` with the key in `TAURI_SIGNING_PRIVATE_KEY` and writes `<file>.sig`, whose
/// content is base64 of the minisign signature file, with the trusted comment
/// `timestamp:<unix>\tfile:<name>\tversion:<version>`.
pub fn sign_file(file: &Path, version: &str) -> Result {
    let key = std::env::var(KEY_ENV).map_err(|_| format!("{KEY_ENV} is not set"))?;
    // Like `tauri build`, the variable holds the key itself or a path to a file holding it.
    let key = match std::fs::read_to_string(&key) {
        Ok(text) => text,
        Err(_) => key,
    };
    let password = std::env::var(PASSWORD_ENV).unwrap_or_default();
    let sig = sign_with(&secret_key(&key, &password)?, file, version)?;
    let sig_path = sig_path(file);
    std::fs::write(&sig_path, sig).map_err(|e| format!("{}: {e}", sig_path.display()))?;
    println!("{}", sig_path.display());
    Ok(())
}

/// `<file>.sig`, the name the Tauri bundler gives it.
pub fn sig_path(file: &Path) -> PathBuf {
    let mut path = OsString::from(file);
    path.push(".sig");
    path.into()
}

/// Decodes a Tauri private key. Tauri keys are always encrypted, an empty password included;
/// a plain unencrypted minisign key is accepted too.
pub(crate) fn secret_key(key: &str, password: &str) -> Result<SecretKey> {
    let text = STANDARD
        .decode(key.trim())
        .map_err(|e| format!("{KEY_ENV} is not base64: {e}"))?;
    let text = String::from_utf8(text).map_err(|_| format!("{KEY_ENV} is not a minisign key"))?;
    let parse = || SecretKeyBox::from_string(&text).map_err(|e| format!("{KEY_ENV}: {e}"));
    if let Ok(key) = parse()?.into_unencrypted_secret_key() {
        return Ok(key);
    }
    parse()?
        .into_secret_key(Some(password.to_owned()))
        .map_err(|e| format!("{KEY_ENV}: {e} (check {PASSWORD_ENV})"))
}

/// The base64 `.sig` content for `file`.
pub(crate) fn sign_with(key: &SecretKey, file: &Path, version: &str) -> Result<String> {
    let name = file
        .file_name()
        .ok_or_else(|| format!("{} is not a file", file.display()))?
        .to_string_lossy();
    // The trusted comment is one line of tab separated fields.
    if name.contains(['\t', '\r', '\n']) || version.contains(['\t', '\r', '\n']) {
        return Err(format!("cannot sign {name:?} {version:?}: tab or newline in the name or version"));
    }
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let trusted = format!("timestamp:{timestamp}\tfile:{name}\tversion:{version}");
    let data = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let signature = minisign::sign(
        None,
        key,
        std::io::BufReader::new(data),
        Some(&trusted),
        Some("signature from tauri secret key"),
    )
    .map_err(|e| format!("signing {}: {e}", file.display()))?;
    Ok(STANDARD.encode(signature.to_string()))
}

/// A new key pair in Tauri's encoding: (private key, public key).
pub(crate) fn generate(password: &str) -> Result<(String, String)> {
    let KeyPair { pk, sk } =
        KeyPair::generate_encrypted_keypair(Some(password.to_owned())).map_err(|e| e.to_string())?;
    let pk = pk.to_box().map_err(|e| e.to_string())?.to_string();
    let sk = sk.to_box(None).map_err(|e| e.to_string())?.to_string();
    Ok((STANDARD.encode(sk), STANDARD.encode(pk)))
}

/// `signer generate -w PATH [-p PASSWORD] [--force]`: writes the private key to PATH and the
/// public key to `PATH.pub`, as `tauri signer generate -w` does.
pub fn generate_command(args: &[String]) -> Result {
    let mut path = None;
    let mut password = String::new();
    let mut force = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-w" | "--write-keys" => path = args.next().map(PathBuf::from),
            "-p" | "--password" => password = args.next().cloned().ok_or(GENERATE_USAGE)?,
            "-f" | "--force" => force = true,
            _ => return Err(GENERATE_USAGE.to_owned()),
        }
    }
    let sk_path = path.ok_or(GENERATE_USAGE)?;
    let pk_path = PathBuf::from(format!("{}.pub", sk_path.display()));
    if !force && let Some(existing) = [&sk_path, &pk_path].into_iter().find(|p| p.exists()) {
        return Err(format!("{} exists; pass --force to replace the key pair", existing.display()));
    }
    let (sk, pk) = generate(&password)?;
    if let Some(dir) = sk_path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write_private(&sk_path, &sk)?;
    std::fs::write(&pk_path, &pk).map_err(|e| format!("{}: {e}", pk_path.display()))?;
    println!("Private key: {} (keep it secret)", sk_path.display());
    println!("Public key:  {}", pk_path.display());
    println!("\nPut this in packaging/updater.json \"pubkey\":\n{pk}");
    Ok(())
}

fn write_private(path: &Path, key: &str) -> Result {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::io::Write::write_all(&mut file, key.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Tauri CLI's own test key (crates/tauri-cli/src/helpers/updater_signature.rs),
    /// encrypted with an empty password.
    const TAURI_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5ClJXUlRZMEl5dkpDN09RZm5GeVAzc2RuYlNzWVVJelJRQnNIV2JUcGVXZUplWXZXYXpqUUFBQkFBQUFBQUFBQUFBQUlBQUFBQTZrN2RnWGh5dURxSzZiL1ZQSDdNcktiaHRxczQwMXdQelRHbjRNcGVlY1BLMTBxR2dpa3I3dDE1UTVDRDE4MXR4WlQwa1BQaXdxKy9UU2J2QmVSNXhOQWFDeG1GSVllbUNpTGJQRkhhTnROR3I5RmdUZi90OGtvaGhJS1ZTcjdZU0NyYzhQWlQ5cGM9Cg==";

    /// What `tauri-plugin-updater` 2.12 does in `verify_signature`, returning the signed version.
    fn verify_like_tauri(data: &[u8], sig_b64: &str, pubkey_b64: &str) -> String {
        let decode = |s: &str| String::from_utf8(STANDARD.decode(s).unwrap()).unwrap();
        let public_key = minisign_verify::PublicKey::decode(&decode(pubkey_b64)).unwrap();
        let signature = minisign_verify::Signature::decode(&decode(sig_b64)).unwrap();
        public_key.verify(data, &signature, true).unwrap();
        signature
            .trusted_comment()
            .split('\t')
            .find_map(|field| field.strip_prefix("version:"))
            .expect("the trusted comment carries version:")
            .to_owned()
    }

    fn temp_file(name: &str, data: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vsesvit-xtask-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn generated_keys_round_trip_through_the_tauri_verifier() {
        for password in ["", "hunter2"] {
            let (sk, pk) = generate(password).unwrap();
            let file = temp_file("Vsesvit_1.2.3_x64-setup.exe", b"MZ installer bytes");
            let sig = sign_with(&secret_key(&sk, password).unwrap(), &file, "1.2.3").unwrap();
            assert_eq!(verify_like_tauri(b"MZ installer bytes", &sig, &pk), "1.2.3");

            let comment = String::from_utf8(STANDARD.decode(&sig).unwrap()).unwrap();
            assert!(comment.contains("\tfile:Vsesvit_1.2.3_x64-setup.exe\tversion:1.2.3\n"), "{comment}");
        }
    }

    #[test]
    fn a_wrong_password_is_rejected() {
        let (sk, _) = generate("right").unwrap();
        assert!(secret_key(&sk, "wrong").is_err());
    }

    #[test]
    fn tampered_data_fails_verification() {
        let (sk, pk) = generate("").unwrap();
        let file = temp_file("a.AppImage", b"original");
        let sig = sign_with(&secret_key(&sk, "").unwrap(), &file, "1.0.0").unwrap();
        let decode = |s: &str| String::from_utf8(STANDARD.decode(s).unwrap()).unwrap();
        let public_key = minisign_verify::PublicKey::decode(&decode(&pk)).unwrap();
        let signature = minisign_verify::Signature::decode(&decode(&sig)).unwrap();
        assert!(public_key.verify(b"tampered", &signature, true).is_err());
    }

    #[test]
    fn signs_with_a_key_made_by_the_tauri_cli() {
        let key = secret_key(TAURI_KEY, "").unwrap();
        let file = temp_file("vsesvit-2.0.0-1-x86_64.pkg.tar.zst", b"zstd");
        assert!(sign_with(&key, &file, "2.0.0").is_ok());
    }

    #[test]
    fn accepts_an_unencrypted_minisign_key() {
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().unwrap();
        let sk = STANDARD.encode(sk.to_box(None).unwrap().to_string());
        let pk = STANDARD.encode(pk.to_box().unwrap().to_string());
        let file = temp_file("plain.deb", b"deb");
        let sig = sign_with(&secret_key(&sk, "").unwrap(), &file, "0.1.0").unwrap();
        assert_eq!(verify_like_tauri(b"deb", &sig, &pk), "0.1.0");
    }

    #[test]
    fn rejects_a_version_that_would_break_the_trusted_comment() {
        let (sk, _) = generate("").unwrap();
        let file = temp_file("b.rpm", b"rpm");
        assert!(sign_with(&secret_key(&sk, "").unwrap(), &file, "1.0.0\tversion:9.9.9").is_err());
    }

    #[test]
    fn sig_path_appends_to_the_full_name() {
        assert_eq!(sig_path(Path::new("dist/V_1_x64-setup.exe")), Path::new("dist/V_1_x64-setup.exe.sig"));
    }
}
