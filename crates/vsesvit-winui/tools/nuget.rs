//! Pinned NuGet packages: download, SHA-256 verification and entry extraction.
//!
//! Shared by `build.rs` (which copies `Microsoft.Web.WebView2.Core.dll` next to the exe) and
//! `tools/bindgen` (which regenerates `src/bindings.rs` from the packages' `.winmd` files).
//! Downloads go through the `curl.exe` that ships with Windows, so neither needs a TLS stack.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

pub struct Package {
    pub id: &'static str,
    pub version: &'static str,
    pub sha256: &'static str,
}

pub const WEBVIEW2: Package = Package {
    id: "Microsoft.Web.WebView2",
    version: "1.0.4191.47",
    sha256: "f492bbf547d0da329553b6727435b677579b1e9f91cc9e4a1ad029366d5f23d0",
};

pub const WINUI: Package = Package {
    id: "Microsoft.WindowsAppSDK.WinUI",
    version: "2.3.9",
    sha256: "d36145bd6c0f46fc04e07fb4af365d093fb42c39b625948d0ead07a0b26fce96",
};

pub const FOUNDATION: Package = Package {
    id: "Microsoft.WindowsAppSDK.Foundation",
    version: "2.3.12",
    sha256: "ce04d01d68cb16b5dc85a5c9efb8162dba876bc2e5b766b5968979899d4efa9e",
};

pub const INTERACTIVE_EXPERIENCES: Package = Package {
    id: "Microsoft.WindowsAppSDK.InteractiveExperiences",
    version: "2.1.9",
    sha256: "f9159d73e3e46717f159606e43b4acf8336aaff5bdd4d395fe3a93fdb41437ac",
};

impl Package {
    pub fn file_name(&self) -> String {
        format!("{}.{}.nupkg", self.id.to_ascii_lowercase(), self.version)
    }

    fn url(&self) -> String {
        let id = self.id.to_ascii_lowercase();
        format!(
            "https://api.nuget.org/v3-flatcontainer/{id}/{v}/{id}.{v}.nupkg",
            v = self.version
        )
    }

    /// Returns the path of a verified copy in `cache`, downloading it when it is missing or
    /// does not match the pinned hash. Never returns an unverified file.
    pub fn fetch(&self, cache: &Path) -> Result<PathBuf, String> {
        let path = cache.join(self.file_name());
        if path.is_file() && self.verify(&path).is_ok() {
            return Ok(path);
        }
        fs::create_dir_all(cache).map_err(|e| format!("create {}: {e}", cache.display()))?;
        let partial = path.with_extension("nupkg.partial");
        let curl = curl_exe();
        let status = Command::new(&curl)
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--retry",
                "3",
            ])
            .arg("--output")
            .arg(&partial)
            .arg(self.url())
            .status()
            .map_err(|e| format!("run {}: {e}", curl.display()))?;
        if !status.success() {
            let _ = fs::remove_file(&partial);
            return Err(format!("download of {} failed ({status})", self.url()));
        }
        if let Err(e) = self.verify(&partial) {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        fs::rename(&partial, &path).map_err(|e| format!("rename {}: {e}", partial.display()))?;
        Ok(path)
    }

    fn verify(&self, path: &Path) -> Result<(), String> {
        let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let actual = hex(&Sha256::digest(&bytes));
        if actual == self.sha256 {
            Ok(())
        } else {
            Err(format!(
                "{} {}: SHA-256 is {actual}, expected {}",
                self.id, self.version, self.sha256
            ))
        }
    }
}

/// Copies one archive entry (forward-slash path inside the package) to `dest`. Leaves an
/// identical `dest` untouched, so a running executable that has the file loaded is no obstacle.
pub fn extract(nupkg: &Path, entry: &str, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(nupkg).map_err(|e| format!("open {}: {e}", nupkg.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("read {}: {e}", nupkg.display()))?;
    let mut source = archive
        .by_name(entry)
        .map_err(|e| format!("{entry} in {}: {e}", nupkg.display()))?;
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut source, &mut bytes).map_err(|e| format!("extract {entry}: {e}"))?;
    if fs::read(dest).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let partial = dest.with_extension("partial");
    fs::write(&partial, &bytes).map_err(|e| format!("write {}: {e}", partial.display()))?;
    fs::rename(&partial, dest).map_err(|e| format!("rename to {}: {e}", dest.display()))
}

/// Entry names in the archive that start with `prefix` and end with `suffix`.
pub fn entries(nupkg: &Path, prefix: &str, suffix: &str) -> Result<Vec<String>, String> {
    let file = fs::File::open(nupkg).map_err(|e| format!("open {}: {e}", nupkg.display()))?;
    let archive =
        zip::ZipArchive::new(file).map_err(|e| format!("read {}: {e}", nupkg.display()))?;
    let mut matching = Vec::new();
    for name in archive.file_names() {
        let name = name.map_err(|e| format!("read {}: {e}", nupkg.display()))?;
        if name.starts_with(prefix) && name.ends_with(suffix) {
            matching.push(name.into_owned());
        }
    }
    Ok(matching)
}

fn curl_exe() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| Path::new(&root).join("System32").join("curl.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("curl.exe"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
