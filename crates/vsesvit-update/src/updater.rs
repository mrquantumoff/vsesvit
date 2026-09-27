//! The network half: asking the endpoints for a release and downloading its artifact.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use minisign_verify::PublicKey;
use semver::Version;
use time::OffsetDateTime;
use url::Url;

use crate::release::{self, Artifact, Manifest};
use crate::{Config, DisabledReason, Error, Format, Installation, WindowsInstallMode};

/// Checks the configured endpoints for a newer release.
#[derive(Debug)]
pub struct Updater {
    agent: ureq::Agent,
    key: PublicKey,
    endpoints: Vec<Url>,
    current_version: Version,
    installation: Installation,
    install_mode: WindowsInstallMode,
}

/// What a release says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub notes: Option<String>,
    pub pub_date: Option<OffsetDateTime>,
}

/// A release newer than the running version.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant, reason = "one per check")]
pub enum Available {
    /// This installation updates itself and the release has an artifact for it.
    Update(Update),
    /// This installation does not update itself (unpackaged, Flatpak), so no artifact is looked
    /// up: the release can be shown, not installed.
    NotInstallable(Release),
}

/// A release newer than the running version, with this installation's artifact picked out.
#[derive(Debug, Clone)]
pub struct Update {
    pub release: Release,
    pub url: Url,
    /// Base64 of the artifact's minisign signature file.
    pub signature: String,
    pub format: Format,
    agent: ureq::Agent,
    key: PublicKey,
    install_mode: WindowsInstallMode,
}

/// An artifact on disk whose signature, signed version and magic bytes have been checked. Only
/// [`Update::download`] makes one, so nothing unverified reaches an installer.
#[derive(Debug)]
pub struct Downloaded {
    pub(crate) path: PathBuf,
    pub(crate) format: Format,
    pub(crate) version: Version,
    #[cfg_attr(not(windows), expect(dead_code, reason = "only the Windows installer has modes"))]
    pub(crate) install_mode: WindowsInstallMode,
}

impl Updater {
    /// Fails with [`Error::Disabled`] when the config has no usable public key or no endpoints.
    /// An installation that does not update itself still checks.
    pub fn new(config: Config, current_version: Version, installation: Installation) -> Result<Updater, Error> {
        let key = release::decode_public_key(&config.pubkey).map_err(Error::Disabled)?;
        if config.endpoints.is_empty() {
            return Err(Error::Disabled(DisabledReason::NoEndpoints));
        }
        Ok(Updater {
            agent: agent(config.https_only, &current_version),
            key,
            endpoints: config.endpoints,
            current_version,
            installation,
            install_mode: config.windows_install_mode,
        })
    }

    /// Asks each endpoint in turn until one answers. `Ok(None)` when the answering endpoint has
    /// nothing newer. When every endpoint fails, returns the last failure.
    pub fn check(&self) -> Result<Option<Available>, Error> {
        let mut last_error = None;
        for template in &self.endpoints {
            let url = release::endpoint_url(template, &self.current_version, self.installation.variant());
            match self.fetch(&url) {
                Ok(None) => return Ok(None),
                Ok(Some(manifest)) => return self.offer(manifest),
                Err(e) => {
                    log::warn!("update endpoint {url} failed: {e}");
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap_or(Error::Disabled(DisabledReason::NoEndpoints)))
    }

    /// `Ok(None)` for `204` and `404`, the two ways a server says it has no release.
    fn fetch(&self, url: &str) -> Result<Option<Manifest>, Error> {
        let mut response = self.agent.get(url).header("Accept", "application/json").call().map_err(network_error)?;
        match response.status().as_u16() {
            204 | 404 => Ok(None),
            200..=299 => {
                let body = response.body_mut().read_to_vec().map_err(network_error)?;
                release::parse_manifest(&body).map(Some)
            }
            status => Err(Error::Http(status)),
        }
    }

    fn offer(&self, manifest: Manifest) -> Result<Option<Available>, Error> {
        if manifest.release.version <= self.current_version {
            return Ok(None);
        }
        let Some(format) = self.installation.format() else {
            return Ok(Some(Available::NotInstallable(manifest.release)));
        };
        let Artifact { url, signature } = manifest.artifact(format.variant())?.clone();
        Ok(Some(Available::Update(Update {
            release: manifest.release,
            url,
            signature,
            format,
            agent: self.agent.clone(),
            key: self.key.clone(),
            install_mode: self.install_mode,
        })))
    }
}

impl Available {
    pub fn release(&self) -> &Release {
        match self {
            Available::Update(update) => &update.release,
            Available::NotInstallable(release) => release,
        }
    }

    /// Fails with [`DisabledReason::NotSelfUpdating`] when this installation does not update
    /// itself.
    pub fn into_update(self) -> Result<Update, Error> {
        match self {
            Available::Update(update) => Ok(update),
            Available::NotInstallable(_) => Err(Error::Disabled(DisabledReason::NotSelfUpdating)),
        }
    }
}

impl Update {
    /// Streams the artifact into `dir`, calling `progress(received, content_length)` as bytes
    /// arrive, then verifies it. A file that fails verification is deleted. When `dir` already
    /// holds this artifact from an earlier download and it verifies, nothing is fetched.
    pub fn download(&self, dir: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Downloaded, Error> {
        let name = DownloadName { version: self.release.version.clone(), format: self.format, partial: false };
        let path = dir.join(name.to_string());
        if path.exists() {
            match self.verify(&path) {
                Ok(()) => return Ok(self.downloaded(path)),
                Err(e) => log::info!("downloading {} again: {e}", path.display()),
            }
        }
        let partial = dir.join(DownloadName { partial: true, ..name }.to_string());
        if let Err(e) = self.fetch_to(&partial, &mut progress).and_then(|()| self.verify(&partial)) {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        fs::rename(&partial, &path)?;
        Ok(self.downloaded(path))
    }

    fn downloaded(&self, path: PathBuf) -> Downloaded {
        Downloaded { path, format: self.format, version: self.release.version.clone(), install_mode: self.install_mode }
    }

    fn fetch_to(&self, partial: &Path, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<(), Error> {
        let response =
            self.agent.get(self.url.as_str()).header("Accept", "application/octet-stream").call().map_err(network_error)?;
        let status = response.status().as_u16();
        if !(200..=299).contains(&status) {
            return Err(Error::Http(status));
        }
        let total = response.body().content_length();
        let mut reader = response.into_body().into_reader();
        let mut file = File::create(partial)?;
        let mut chunk = vec![0u8; 64 * 1024];
        let mut received = 0u64;
        progress(received, total);
        loop {
            let n = reader.read(&mut chunk).map_err(|e| Error::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&chunk[..n])?;
            received += n as u64;
            progress(received, total);
        }
        file.sync_all()?;
        Ok(())
    }

    fn verify(&self, file: &Path) -> Result<(), Error> {
        let data = fs::read(file)?;
        release::verify(&data, &self.signature, &self.key, &self.release.version)?;
        if !self.format.matches_magic(&data) {
            return Err(Error::WrongArtifactType(self.format));
        }
        Ok(())
    }
}

/// Deletes what [`Update::download`] left in `dir`: partial downloads, and the artifacts of
/// every version but `keep`. A file that cannot be deleted, such as an installer that is still
/// running, stays until the next call. Other files are left alone, and a missing `dir` is not
/// an error.
pub fn remove_stale_downloads(dir: &Path, keep: Option<&Version>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().and_then(DownloadName::parse) else {
            continue;
        };
        if !name.partial && Some(&name.version) == keep {
            continue;
        }
        if let Err(e) = fs::remove_file(entry.path()) {
            log::debug!("keeping {}: {e}", entry.path().display());
        }
    }
    Ok(())
}

/// `vsesvit-<version><format suffix>`, plus `.part` while it downloads.
#[derive(Debug, PartialEq, Eq)]
struct DownloadName {
    version: Version,
    format: Format,
    partial: bool,
}

impl DownloadName {
    fn parse(name: &str) -> Option<DownloadName> {
        let (name, partial) = match name.strip_suffix(".part") {
            Some(name) => (name, true),
            None => (name, false),
        };
        let rest = name.strip_prefix("vsesvit-")?;
        Format::ALL.into_iter().find_map(|format| {
            let version = Version::parse(rest.strip_suffix(format.file_suffix())?).ok()?;
            Some(DownloadName { version, format, partial })
        })
    }
}

impl fmt::Display for DownloadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vsesvit-{}{}", self.version, self.format.file_suffix())?;
        if self.partial {
            f.write_str(".part")?;
        }
        Ok(())
    }
}

impl Downloaded {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn format(&self) -> Format {
        self.format
    }
}

fn agent(https_only: bool, current_version: &Version) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(https_only)
        .http_status_as_error(false)
        .user_agent(format!("vsesvit/{current_version}"))
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(Duration::from_secs(30 * 60)))
        .build()
        .into()
}

fn network_error(e: ureq::Error) -> Error {
    match e {
        ureq::Error::StatusCode(code) => Error::Http(code),
        other => Error::Network(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_names_round_trip() {
        for format in Format::ALL {
            for partial in [false, true] {
                let name = DownloadName { version: Version::parse("2.0.0-rc.1").unwrap(), format, partial };
                assert_eq!(DownloadName::parse(&name.to_string()), Some(name));
            }
        }
    }

    #[test]
    fn other_names_are_not_downloads() {
        for name in ["vsesvit-1.2.3.tar.gz", "other-1.2.3.deb", "vsesvit-latest.rpm", "vsesvit-1.2.3.deb.part.part", "notes"] {
            assert_eq!(DownloadName::parse(name), None, "{name}");
        }
    }
}
