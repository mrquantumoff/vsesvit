//! Owned by W1.3.
//! `cargo xtask helper-check`: builds `vsesvit-helper` and reads its dynamic section with `readelf -d`;
//! fails if `NEEDED` lists anything beyond libcef, libc, libm and libgcc_s (design sections 1.1 and
//! 6.2). The helper is every CEF sub-process's image, so a library it needs is loaded into every
//! renderer (SC sections 2 and 3.1).

use std::process::Command;

use crate::{Result, Status, testenv};

const USAGE: &str = "usage: cargo xtask helper-check";

pub fn run(args: &[String]) -> Result<Status> {
    if !args.is_empty() {
        return Err(USAGE.to_owned());
    }
    if !cfg!(target_os = "linux") {
        return Err("helper-check runs on Linux: the helper is the Linux sub-process image".to_owned());
    }
    let helper = testenv::cargo_build(&["-p", "vsesvit-helper"], "vsesvit-helper")?;
    let output = Command::new("readelf").arg("-d").arg(&helper).output().map_err(|e| format!("readelf: {e}"))?;
    if !output.status.success() {
        return Err(format!("readelf -d {} failed: {}", helper.display(), String::from_utf8_lossy(&output.stderr)));
    }
    let needed = needed(&String::from_utf8_lossy(&output.stdout));
    if needed.is_empty() {
        return Err(format!("readelf -d {} lists no NEEDED library; is it dynamic?", helper.display()));
    }
    let extra: Vec<&String> = needed.iter().filter(|lib| !allowed(lib)).collect();
    println!("vsesvit-helper NEEDED: {}", needed.join(", "));
    if !extra.is_empty() {
        let extra: Vec<&str> = extra.iter().map(|s| s.as_str()).collect();
        return Err(format!("vsesvit-helper needs more than libcef, libc, libm and libgcc_s: {}", extra.join(", ")));
    }
    Ok(Status::Done)
}

/// The libraries of the `(NEEDED)` lines of `readelf -d`.
fn needed(readelf: &str) -> Vec<String> {
    readelf
        .lines()
        .filter(|line| line.contains("(NEEDED)"))
        .filter_map(|line| Some(line.split_once('[')?.1.split_once(']')?.0.to_owned()))
        .collect()
}

/// libcef and the C runtime: libc, libm, libgcc_s, and glibc's own dynamic loader, which glibc 2.34
/// and later name as `NEEDED` (`ld-linux-x86-64.so.2`) and every dynamic executable loads anyway.
fn allowed(lib: &str) -> bool {
    lib == "libcef.so" || ["libc.so.", "libm.so.", "libgcc_s.so.", "ld-linux-"].iter().any(|prefix| lib.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    const READELF: &str = "
Dynamic section at offset 0x2a5d48 contains 30 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libcef.so]
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
 0x000000000000001d (RUNPATH)            Library runpath: [$ORIGIN]
";

    #[test]
    fn reads_the_needed_libraries() {
        assert_eq!(needed(READELF), ["libcef.so", "libgcc_s.so.1", "libm.so.6", "libc.so.6", "ld-linux-x86-64.so.2"]);
        assert!(needed(READELF).iter().all(|lib| allowed(lib)));
    }

    #[test]
    fn anything_else_fails() {
        for lib in ["libxcb.so.1", "libvulkan.so.1", "libX11.so.6", "libglib-2.0.so.0", "libcef.so.2", "libcrypto.so.3"] {
            assert!(!allowed(lib), "{lib}");
        }
    }

    #[test]
    fn takes_no_arguments() {
        assert!(run(&["x".into()]).is_err());
    }
}
