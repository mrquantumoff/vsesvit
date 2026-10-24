//! Owned by W0.5.
//! `cargo xtask probe EXAMPLE [ARGS]...`: builds one example of `vsesvit-testenv`, `vsesvit-gpui` or
//! `vsesvit-cef` and runs it inside a fresh test environment (design section 6.2). The only
//! documented way to run an example.

use std::ffi::OsString;
use std::time::Duration;

use crate::testenv::{self, Run};
use crate::{Result, Status, workspace_root};

const USAGE: &str = "usage: cargo xtask probe EXAMPLE [ARGS]...";
/// The crates whose examples run here, searched in this order.
const CRATES: &[&str] = &["vsesvit-testenv", "vsesvit-gpui", "vsesvit-cef"];
const TIMEOUT: Duration = Duration::from_secs(300);

pub fn run(args: &[String]) -> Result<Status> {
    let Some((example, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    let krate = owner(example, |krate, file| workspace_root().join("crates").join(krate).join("examples").join(file).exists())?;
    if krate == "vsesvit-cef" && cfg!(target_os = "linux") {
        testenv::cargo_build(&["-p", "vsesvit-helper"], "vsesvit-helper")?;
    }
    let program = testenv::cargo_build(&["-p", krate, "--example", example], example)?;
    // cef-dll-sys copies libcef.dll and its runtime beside the bins, one level above `examples\`,
    // and Windows looks for DLLs on PATH, not on an rpath.
    if cfg!(windows) && let Some(bins) = program.parent().and_then(|examples| examples.parent()) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let joined = std::env::join_paths(std::iter::once(bins.to_path_buf()).chain(std::env::split_paths(&path)))
            .map_err(|e| e.to_string())?;
        // SAFETY: xtask has started no other threads at this point.
        unsafe { std::env::set_var("PATH", joined) };
    }
    let out_dir = workspace_root().join("target").join("probe").join(example);
    let code = testenv::run(&Run {
        program,
        args: rest.iter().map(OsString::from).collect(),
        out_dir: fresh(out_dir)?,
        timeout: TIMEOUT,
    })?;
    match code {
        0 => Ok(Status::Done),
        code => Err(format!("{example} exited with {code}")),
    }
}

/// The crate whose `examples/` holds `NAME.rs` or `NAME/main.rs`.
fn owner(example: &str, exists: impl Fn(&str, &str) -> bool) -> Result<&'static str> {
    let plain = example.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !plain || example.is_empty() {
        return Err(format!("{example:?} is not an example name\n{USAGE}"));
    }
    CRATES
        .iter()
        .copied()
        .find(|krate| exists(krate, &format!("{example}.rs")) || exists(krate, &format!("{example}/main.rs")))
        .ok_or(format!("no example {example:?} in {}", CRATES.join(", ")))
}

/// `dir`, emptied.
pub(crate) fn fresh(dir: std::path::PathBuf) -> Result<std::path::PathBuf> {
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::owner;

    #[test]
    fn finds_the_crate_that_owns_an_example() {
        let tree = |krate: &str, file: &str| matches!((krate, file), ("vsesvit-testenv", "raw_window.rs") | ("vsesvit-gpui", "native_probe/main.rs"));
        assert_eq!(owner("raw_window", tree), Ok("vsesvit-testenv"));
        assert_eq!(owner("native_probe", tree), Ok("vsesvit-gpui"));
        assert!(owner("smoke", tree).unwrap_err().contains("no example"));
        assert!(owner("../escape", tree).is_err());
        assert!(owner("", tree).is_err());
    }

    #[test]
    fn needs_an_example() {
        assert!(super::run(&[]).is_err());
    }
}
