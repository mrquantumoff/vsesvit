//! The network half: asking the endpoints for a release and downloading its artifact.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use minisign_verify::PublicKey;
use semver::Version;
use time::OffsetDateTime;
use url::Url;

use crate::release::{self, Artifact, Manifest};
use crate::{Config, DisabledReason, Error, Format, Installation, WindowsInstallMode};

/// The largest artifact a download takes, many times a release's size. The signature is checked
/// only once the whole file is here, so without a cap an endpoint or artifact host could fill
/// the disk, and then the memory [`Update::download`] reads the file into to verify it.
const MAX_ARTIFACT_SIZE: u64 = 512 * 1024 * 1024;

/// The channels, from the steadiest to the newest, as `UpdateChannel` in vsesvit-core names them.
/// A release is on the channel its prerelease starts with, or on stable without one.
const CHANNELS: [&str; 4] = ["stable", "beta", "weekly", "nightly"];

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
/// [`Update::download`] makes one, and installing checks the file again first, since anything
/// running as the user can change it in between. The installer still reads it from a path the
/// user can write to, so it is only as safe as the user's account.
#[derive(Debug)]
pub struct Downloaded {
    pub(crate) path: PathBuf,
    pub(crate) format: Format,
    pub(crate) version: Version,
    #[cfg_attr(not(windows), expect(dead_code, reason = "only the Windows installer has modes"))]
    pub(crate) install_mode: WindowsInstallMode,
    signature: String,
    key: PublicKey,
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

    /// Asks each endpoint in turn until one answers, filling `{{channel}}` with `channel`.
    /// `Ok(None)` when the answering endpoint has nothing newer, or only a release on a channel
    /// less steady than `channel`. When every endpoint fails, returns the last failure.
    pub fn check(&self, channel: &str) -> Result<Option<Available>, Error> {
        let mut last_error = None;
        for template in &self.endpoints {
            let url = release::endpoint_url(template, channel, &self.current_version, self.installation.variant());
            match self.fetch(&url) {
                Ok(None) => return Ok(None),
                Ok(Some(manifest)) => return self.offer(manifest, channel),
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

    fn offer(&self, manifest: Manifest, channel: &str) -> Result<Option<Available>, Error> {
        if manifest.release.version <= self.current_version {
            return Ok(None);
        }
        // The signature binds the version, not the channel, so an endpoint answering for stable
        // could otherwise hand out a nightly build, which sorts above the last stable release.
        if steadiness(release_channel(&manifest.release.version)) > steadiness(channel) {
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
    /// arrive, then verifies it. A file that grows past 512 MiB or fails verification is
    /// deleted. When `dir` already holds this artifact from an earlier download and it verifies,
    /// nothing is fetched. Other processes, such as other profiles, can download into the same
    /// `dir` at the same time: each writes a partial file of its own.
    ///
    /// The connection can drop, or the 30 minutes a response gets run out on a slow link. The
    /// download then asks for the rest while each attempt gets further, and fails with
    /// [`Error::Network`] once one gets nowhere. The partial file stays, and the next call
    /// resumes it.
    pub fn download(&self, dir: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Downloaded, Error> {
        let name = DownloadName { version: self.release.version.clone(), format: self.format };
        let path = dir.join(name.to_string());
        if path.exists() {
            match fs::read(&path).map_err(Error::from).and_then(|data| self.verify(&data)) {
                Ok(()) => return Ok(self.downloaded(path)),
                Err(e) => log::info!("downloading {} again: {e}", path.display()),
            }
        }
        let (mut file, partial) = match resume_partial(dir, &name) {
            Some(found) => found,
            None => create_partial(dir, &name)?,
        };
        let fetched = loop {
            let before = file.metadata()?.len();
            match self.fetch_to(&mut file, &mut progress) {
                Err(Error::Network(e)) if file.metadata()?.len() > before => log::info!("resuming the download: {e}"),
                // Unlocked when `file` closes, for the next download to resume.
                Err(e @ Error::Network(_)) => return Err(e),
                result => break result,
            }
        };
        if let Err(e) = fetched.and_then(|()| self.verify(&read_back(&mut file)?)) {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        if let Err(e) = fs::rename(&partial, &path) {
            // Another download of this version can have put its copy there first, and an
            // installer can be running from it.
            let _ = fs::remove_file(&partial);
            return match fs::read(&path).map_err(Error::from).and_then(|data| self.verify(&data)) {
                Ok(()) => Ok(self.downloaded(path)),
                Err(_) => Err(e.into()),
            };
        }
        Ok(self.downloaded(path))
    }

    fn downloaded(&self, path: PathBuf) -> Downloaded {
        Downloaded {
            path,
            format: self.format,
            version: self.release.version.clone(),
            install_mode: self.install_mode,
            signature: self.signature.clone(),
            key: self.key.clone(),
        }
    }

    /// Appends the rest of the artifact to what `file` holds, or replaces it when the server
    /// does not resume.
    fn fetch_to(&self, file: &mut File, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<(), Error> {
        let start = file.seek(SeekFrom::End(0))?;
        let mut request = self.agent.get(self.url.as_str()).header("Accept", "application/octet-stream");
        if start > 0 {
            request = request.header("Range", format!("bytes={start}-"));
        }
        let response = request.call().map_err(network_error)?;
        let status = response.status().as_u16();
        let start = match status {
            206 if content_range_start(&response) == Some(start) => start,
            206 => return Err(Error::BadResponse("the server sent another part of the update".into())),
            // The partial file is as long as the artifact or longer: it is not a part of it.
            416 if start > 0 => {
                file.set_len(0)?;
                return self.fetch_to(file, progress);
            }
            200..=299 => {
                file.set_len(0)?;
                file.rewind()?;
                0
            }
            status => return Err(Error::Http(status)),
        };
        let total = response.body().content_length().map(|rest| start + rest);
        let reader = response.into_body().into_reader();
        save(reader, start, total, file, MAX_ARTIFACT_SIZE, progress)?;
        file.sync_all()?;
        Ok(())
    }

    fn verify(&self, data: &[u8]) -> Result<(), Error> {
        verify(data, &self.signature, &self.key, &self.release.version, self.format)
    }
}

/// Creates and locks `<name>.<tag>.part`, a name no other download uses. The lock keeps
/// [`remove_stale_downloads`] in other processes away from it while it is written.
fn create_partial(dir: &Path, name: &DownloadName) -> io::Result<(File, PathBuf)> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    loop {
        let path = dir.join(format!("{name}.{}-{}.part", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let file = match File::options().read(true).write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        // A cleanup that locked the file before this did has deleted it by the time it lets go.
        file.lock()?;
        if path.exists() {
            return Ok((file, path));
        }
    }
}

/// Locks a partial download of `name` an earlier download left, to resume it.
fn resume_partial(dir: &Path, name: &DownloadName) -> Option<(File, PathBuf)> {
    fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        let (found, true) = entry.file_name().to_str().and_then(DownloadName::parse)? else {
            return None;
        };
        let file = (found == *name).then(|| lock_partial(&path).ok()).flatten()?;
        // A cleanup that locked the file before this did has deleted it by the time it lets go.
        path.exists().then_some((file, path))
    })
}

/// Reads through the locking handle: on Windows the lock keeps every other handle out.
fn read_back(file: &mut File) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    file.rewind()?;
    file.read_to_end(&mut data)?;
    Ok(data)
}

fn verify(data: &[u8], signature: &str, key: &PublicKey, version: &Version, format: Format) -> Result<(), Error> {
    release::verify(data, signature, key, version)?;
    if !format.matches_magic(data) {
        return Err(Error::WrongArtifactType(format));
    }
    Ok(())
}

/// Copies `reader` into `file`, which holds the first `start` bytes of the `total` the server
/// announced. Fails with [`Error::TooLarge`] without reading when `total` is over `max`, and
/// before writing past `max` whatever the server announced.
fn save(
    mut reader: impl Read,
    start: u64,
    total: Option<u64>,
    file: &mut impl Write,
    max: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Error> {
    if total.is_some_and(|total| total > max) {
        return Err(Error::TooLarge(max));
    }
    let mut chunk = vec![0u8; 64 * 1024];
    let mut received = start;
    progress(received, total);
    loop {
        let n = reader.read(&mut chunk).map_err(|e| Error::Network(e.to_string()))?;
        if n == 0 {
            break;
        }
        if received + n as u64 > max {
            return Err(Error::TooLarge(max));
        }
        file.write_all(&chunk[..n])?;
        received += n as u64;
        progress(received, total);
    }
    Ok(())
}

/// Deletes what [`Update::download`] left in `dir` for every version but `keep`: artifacts and
/// partial downloads. `keep`'s partial downloads stay for a download to resume. A file that
/// cannot be deleted, such as an installer that is still running, stays until the next call,
/// and so does a partial download another process is still writing. Other files are left
/// alone, and a missing `dir` is not an error.
pub fn remove_stale_downloads(dir: &Path, keep: Option<&Version>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let Some((name, partial)) = entry.file_name().to_str().and_then(DownloadName::parse) else {
            continue;
        };
        if Some(&name.version) == keep {
            continue;
        }
        // Held until the file is gone, so a download cannot take the file up in between.
        let _lock = match partial.then(|| lock_partial(&entry.path())).transpose() {
            Ok(lock) => lock,
            Err(e) => {
                log::debug!("keeping {}: {e}", entry.path().display());
                continue;
            }
        };
        if let Err(e) = fs::remove_file(entry.path()) {
            log::debug!("keeping {}: {e}", entry.path().display());
        }
    }
    Ok(())
}

/// Locks a partial download, failing when another process holds it.
fn lock_partial(path: &Path) -> io::Result<File> {
    let file = File::options().read(true).write(true).open(path)?;
    file.try_lock()?;
    Ok(file)
}

/// `vsesvit-<version><format suffix>`, plus `.<tag>.part` while it downloads, where the tag
/// tells concurrent downloads apart. Older versions wrote `.part` alone.
#[derive(Debug, PartialEq, Eq)]
struct DownloadName {
    version: Version,
    format: Format,
}

impl DownloadName {
    /// The name, and whether it is a partial download.
    fn parse(name: &str) -> Option<(DownloadName, bool)> {
        let Some(name) = name.strip_suffix(".part") else {
            return DownloadName::parse_whole(name).map(|name| (name, false));
        };
        let untagged = name
            .rsplit_once('.')
            .filter(|(_, tag)| !tag.is_empty() && tag.bytes().all(|b| b.is_ascii_digit() || b == b'-'))
            .and_then(|(name, _)| DownloadName::parse_whole(name));
        untagged.or_else(|| DownloadName::parse_whole(name)).map(|name| (name, true))
    }

    fn parse_whole(name: &str) -> Option<DownloadName> {
        let rest = name.strip_prefix("vsesvit-")?;
        Format::ALL.into_iter().find_map(|format| {
            let version = Version::parse(rest.strip_suffix(format.file_suffix())?).ok()?;
            Some(DownloadName { version, format })
        })
    }
}

impl fmt::Display for DownloadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vsesvit-{}{}", self.version, self.format.file_suffix())
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

    /// The file's bytes, checked again as [`Update::download`] checked them.
    pub(crate) fn verified_bytes(&self) -> Result<Vec<u8>, Error> {
        let data = fs::read(&self.path)?;
        verify(&data, &self.signature, &self.key, &self.version, self.format)?;
        Ok(data)
    }
}

fn release_channel(version: &Version) -> &str {
    match version.pre.as_str().split('.').next() {
        None | Some("") => "stable",
        Some(channel) => channel,
    }
}

/// Where `channel` stands in [`CHANNELS`]; one no build is released on comes after all of them.
fn steadiness(channel: &str) -> usize {
    CHANNELS.iter().position(|c| *c == channel).unwrap_or(CHANNELS.len())
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

/// Where the part a `206` holds starts, from `Content-Range: bytes <start>-<end>/<length>`.
fn content_range_start(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    let range = response.headers().get("Content-Range")?.to_str().ok()?;
    range.strip_prefix("bytes ")?.split_once('-')?.0.parse().ok()
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
            let name = || DownloadName { version: Version::parse("2.0.0-rc.1").unwrap(), format };
            assert_eq!(DownloadName::parse(&name().to_string()), Some((name(), false)));
            for partial in [format!("{}.123-4.part", name()), format!("{}.part", name())] {
                assert_eq!(DownloadName::parse(&partial), Some((name(), true)), "{partial}");
            }
        }
    }

    #[test]
    fn an_announced_length_over_the_limit_is_refused_before_reading() {
        let mut file = Vec::new();
        let mut body = io::repeat(1).take(10);
        let err = save(&mut body, 0, Some(1025), &mut file, 1024, &mut |_, _| {}).unwrap_err();
        assert!(matches!(err, Error::TooLarge(1024)), "{err:?}");
        assert_eq!((file.len(), body.limit()), (0, 10), "nothing is read or written");
    }

    #[test]
    fn a_body_longer_than_the_limit_is_cut_off() {
        for total in [None, Some(1000)] {
            let mut file = Vec::new();
            let err = save(io::repeat(1).take(1 << 20), 0, total, &mut file, 1024, &mut |_, _| {}).unwrap_err();
            assert!(matches!(err, Error::TooLarge(1024)), "{total:?}: {err:?}");
            assert!(file.len() <= 1024, "{total:?}: wrote {} bytes", file.len());
        }
        let mut file = Vec::new();
        save(io::repeat(1).take(1024), 0, None, &mut file, 1024, &mut |_, _| {}).expect("exactly the limit is fine");
        assert_eq!(file.len(), 1024);
    }

    #[test]
    fn other_names_are_not_downloads() {
        for name in ["vsesvit-1.2.3.tar.gz", "other-1.2.3.deb", "vsesvit-latest.rpm", "vsesvit-1.2.3.deb.part.part", "notes"] {
            assert_eq!(DownloadName::parse(name), None, "{name}");
        }
    }
}
