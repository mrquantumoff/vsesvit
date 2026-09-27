//! Process, file and download helpers shared by the Linux formats.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::Result;

pub fn run(cmd: &mut Command) -> Result {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed ({status})")) }
}

pub fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.stderr(Stdio::inherit()).output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{cmd:?} failed ({})", out.status));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{cmd:?}: {e}"))
}

/// `producer | consumer > out`.
pub fn pipe(producer: &mut Command, consumer: &mut Command, out: &Path) -> Result {
    let file = fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut first = producer.stdout(Stdio::piped()).spawn().map_err(|e| format!("{producer:?}: {e}"))?;
    let stdout = first.stdout.take().ok_or("no stdout")?;
    let status = consumer.stdin(stdout).stdout(file).status().map_err(|e| format!("{consumer:?}: {e}"))?;
    let first_status = first.wait().map_err(|e| format!("{producer:?}: {e}"))?;
    if !first_status.success() {
        return Err(format!("{producer:?} failed ({first_status})"));
    }
    if !status.success() {
        return Err(format!("{consumer:?} failed ({status})"));
    }
    Ok(())
}

pub fn exists_on_path(program: &str) -> bool {
    Command::new(program).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> Result {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_to_string(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn create_dir_all(path: &Path) -> Result {
    fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A directory that starts empty on every run, so a rebuild never inherits stale files.
pub fn fresh_dir(path: &Path) -> Result {
    remove_dir_all(path)?;
    create_dir_all(path)
}

pub fn remove_dir_all(path: &Path) -> Result {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Copies a file, following symlinks, creating parent directories.
pub fn copy(from: &Path, to: &Path) -> Result {
    if let Some(parent) = to.parent() {
        create_dir_all(parent)?;
    }
    fs::copy(from, to).map(|_| ()).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

/// Copies a tree, keeping symlinks as symlinks.
pub fn copy_tree(from: &Path, to: &Path) -> Result {
    create_dir_all(to)?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", src.display()))?;
        if kind.is_symlink() {
            let target = fs::read_link(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            symlink(&target, &dst)?;
        } else if kind.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            copy(&src, &dst)?;
        }
    }
    Ok(())
}

pub fn symlink(target: &Path, link: &Path) -> Result {
    if let Some(parent) = link.parent() {
        create_dir_all(parent)?;
    }
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).map_err(|e| format!("{}: {e}", link.display()));
    #[cfg(not(unix))]
    Err(format!("{} -> {}: symlinks need a Unix host", link.display(), target.display()))
}

pub fn set_mode(path: &Path, mode: u32) -> Result {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| format!("{}: {e}", path.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// Every file, directory and symlink under `root`, relative to it, in byte order.
pub fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result {
        for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
            out.push(path.strip_prefix(root).map_err(|e| e.to_string())?.to_path_buf());
            if path.is_dir() && !path.is_symlink() {
                visit(root, &path, out)?;
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

/// Bytes of the regular files under `root` (what packagers call the installed size).
pub fn tree_size(root: &Path) -> Result<u64> {
    let mut total = 0;
    for rel in walk(root)? {
        let meta = fs::symlink_metadata(root.join(&rel)).map_err(|e| format!("{}: {e}", rel.display()))?;
        if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
}

/// A build tool pinned by URL and SHA-256, cached under `target/package/tools`.
pub struct Download {
    pub url: &'static str,
    pub sha256: &'static str,
    pub file_name: &'static str,
}

impl Download {
    /// Returns a verified copy in `cache`, downloading it when missing or corrupt. Never returns
    /// an unverified file.
    pub fn fetch(&self, cache: &Path) -> Result<PathBuf> {
        let path = cache.join(self.file_name);
        if path.is_file() && self.verify(&path).is_ok() {
            return Ok(path);
        }
        create_dir_all(cache)?;
        println!("downloading {}", self.url);
        let mut response = ureq::get(self.url).call().map_err(|e| format!("{}: {e}", self.url))?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(256 << 20)
            .read_to_vec()
            .map_err(|e| format!("{}: {e}", self.url))?;
        let partial = path.with_extension("partial");
        write(&partial, &bytes)?;
        if let Err(e) = self.verify(&partial) {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    fn verify(&self, path: &Path) -> Result {
        let actual = hex(&Sha256::digest(read(path)?));
        if actual == self.sha256 {
            Ok(())
        } else {
            Err(format!("{}: SHA-256 is {actual}, expected {}", self.file_name, self.sha256))
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
