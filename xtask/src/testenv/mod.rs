//! Owned by W0.5.
//! The launcher half of the test environment (design section 7.3): a private Xvfb display with its
//! own D-Bus session on Linux, a private desktop with a foreground watchdog on Windows. `selftest`
//! and `probe` use it; it has no command of its own. It writes `OUT_DIR/testenv/env.json`, which
//! `vsesvit_testenv::require()` checks against the machine in the started program.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

// One schema for the writer and the guard: the guard crate's own file.
#[path = "../../../crates/vsesvit-testenv/src/envfile.rs"]
#[allow(dead_code)]
mod envfile;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::Result;
use envfile::{EnvFile, Token};

/// One program to run inside a fresh test environment.
pub struct Run {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Holds `testenv/` (env.json, the environment's own files) and the run's home directory;
    /// the program's own output goes here too.
    pub out_dir: PathBuf,
    /// The run is killed, and fails, after this long.
    pub timeout: Duration,
}

/// Runs `run.program` in a test environment made for it alone, waits for it, and takes the
/// environment down with every process started in it. Returns the program's exit code; a
/// timeout, or a watchdog trip on Windows, is an error.
pub fn run(run: &Run) -> Result<i32> {
    let dir = run.out_dir.join("testenv");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let token = Token::from_bytes(random_bytes()?);
    #[cfg(target_os = "linux")]
    return linux::launch(run, &dir, token);
    #[cfg(windows)]
    return windows::launch(run, &dir, token);
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = token;
        Err("the test environment exists on Linux and Windows only".into())
    }
}

/// Writes `dir/env.json`, readable by its owner only, and returns its path.
fn write_env_file(dir: &Path, file: &EnvFile) -> Result<PathBuf> {
    let path = dir.join("env.json");
    write_private(&path, file.to_json().as_bytes())?;
    Ok(path)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut f = options.open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn random_bytes() -> Result<[u8; 16]> {
    #[cfg(windows)]
    return windows::random_bytes();
    #[cfg(not(windows))]
    {
        use std::io::Read as _;
        let mut bytes = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|e| format!("/dev/urandom: {e}"))?;
        Ok(bytes)
    }
}

/// Builds with `cargo build ARGS` and returns the executable of the target named `name`.
pub fn cargo_build(args: &[&str], name: &str) -> Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .arg("build")
        .args(args)
        .arg("--message-format=json-render-diagnostics")
        .current_dir(crate::workspace_root())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo build {} failed", args.join(" ")));
    }
    executable(&String::from_utf8_lossy(&output.stdout), name)
        .ok_or(format!("cargo build {} built no executable {name:?}", args.join(" ")))
}

/// The last `executable` cargo reports for a target named `name`.
fn executable(messages: &str, name: &str) -> Option<PathBuf> {
    messages
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == name)
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_named_executable_in_cargos_messages() {
        let messages = r#"{"reason":"compiler-artifact","target":{"name":"vsesvit_testenv"},"executable":null}
not json
{"reason":"compiler-artifact","target":{"name":"raw_window"},"executable":"/t/debug/examples/raw_window"}
{"reason":"build-finished","success":true}"#;
        assert_eq!(super::executable(messages, "raw_window"), Some("/t/debug/examples/raw_window".into()));
        assert_eq!(super::executable(messages, "vsesvit_testenv"), None);
    }

    #[test]
    fn tokens_are_fresh() {
        assert_ne!(super::random_bytes().unwrap(), super::random_bytes().unwrap());
    }
}
