//! The network half: asking the endpoints for a release and downloading its artifact.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use minisign_verify::PublicKey;
use semver::Version;
use time::OffsetDateTime;
use url::Url;

use crate::release::{self, Release};
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

/// A release newer than the running version, with this installation's artifact picked out.
#[derive(Debug, Clone)]
pub struct Update {
    pub version: Version,
    pub notes: Option<String>,
    pub pub_date: Option<OffsetDateTime>,
    pub url: Url,
    /// Base64 of the artifact's minisign signature file.
    pub signature: String,
    /// `None` when this installation does not update itself: the update can be shown but not
    /// downloaded.
    pub format: Option<Format>,
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
    pub fn check(&self) -> Result<Option<Update>, Error> {
        let mut last_error = None;
        for template in &self.endpoints {
            let url = release::endpoint_url(template, &self.current_version, self.installation.variant());
            match self.fetch(&url) {
                Ok(None) => return Ok(None),
                Ok(Some(release)) => return self.offer(release),
                Err(e) => {
                    log::warn!("update endpoint {url} failed: {e}");
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap_or(Error::Disabled(DisabledReason::NoEndpoints)))
    }

    /// `Ok(None)` for `204` and `404`, the two ways a server says it has no release.
    fn fetch(&self, url: &str) -> Result<Option<Release>, Error> {
        let mut response = self.agent.get(url).header("Accept", "application/json").call().map_err(network_error)?;
        match response.status().as_u16() {
            204 | 404 => Ok(None),
            200..=299 => {
                let body = response.body_mut().read_to_vec().map_err(network_error)?;
                release::parse_release(&body).map(Some)
            }
            status => Err(Error::Http(status)),
        }
    }

    fn offer(&self, release: Release) -> Result<Option<Update>, Error> {
        if release.version <= self.current_version {
            return Ok(None);
        }
        let artifact = release.artifact(self.installation.variant())?;
        Ok(Some(Update {
            url: artifact.url.clone(),
            signature: artifact.signature.clone(),
            version: release.version,
            notes: release.notes,
            pub_date: release.pub_date,
            format: self.installation.format(),
            agent: self.agent.clone(),
            key: self.key.clone(),
            install_mode: self.install_mode,
        }))
    }
}

impl Update {
    /// Streams the artifact into `dir`, calling `progress(received, content_length)` as bytes
    /// arrive, then verifies it. A file that fails verification is deleted.
    pub fn download(&self, dir: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Downloaded, Error> {
        let format = self.format.ok_or(Error::Disabled(DisabledReason::NotSelfUpdating))?;
        let name = format!("vsesvit-{}{}", self.version, format.file_suffix());
        let path = dir.join(&name);
        let partial = dir.join(format!("{name}.part"));
        if let Err(e) = self.fetch_to(&partial, &mut progress).and_then(|()| self.verify(&partial, format)) {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        fs::rename(&partial, &path)?;
        Ok(Downloaded { path, format, version: self.version.clone(), install_mode: self.install_mode })
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

    fn verify(&self, partial: &Path, format: Format) -> Result<(), Error> {
        let data = fs::read(partial)?;
        release::verify(&data, &self.signature, &self.key, &self.version)?;
        if !format.matches_magic(&data) {
            return Err(Error::WrongArtifactType(format));
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
