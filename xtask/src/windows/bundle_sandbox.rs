//! Owned by W1.3.
//! `cargo xtask bundle-sandbox [--release]`: builds `vsesvit-sandbox` into `target/gpui` and lays out
//! the sandboxed Windows image in `target/gpui/bundle` (design sections 1.1 and 6.2, SW section 3.2):
//! CEF's `bootstrap.exe` renamed `vsesvit.exe`, our `vsesvit_app.dll` as `vsesvit.dll`, and the CEF
//! runtime `cef-dll-sys` copied beside them. `package nsisgpui` stages from it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Result, Status, workspace_root};

const USAGE: &str = "usage: cargo xtask bundle-sandbox [--release]";

pub fn run(args: &[String]) -> Result<Status> {
    let release = match args {
        [] => false,
        [flag] if flag == "--release" => true,
        _ => return Err(USAGE.to_owned()),
    };
    if !cfg!(windows) {
        return Err("bundle-sandbox runs on Windows".to_owned());
    }
    let target = workspace_root().join("target").join("gpui");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build.args(["build", "-p", "vsesvit-sandbox", "--target-dir"]).arg(&target).current_dir(workspace_root());
    if release {
        build.arg("--release");
    }
    let status = build.status().map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build -p vsesvit-sandbox failed: {status}"));
    }
    let built = target.join(if release { "release" } else { "debug" });
    let bundle = crate::probe::fresh(target.join("bundle"))?;
    let names = std::fs::read_dir(&built)
        .map_err(|e| format!("{}: {e}", built.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    let plan = plan(&names)?;
    for (from, to) in &plan {
        copy(&built.join(from), &bundle.join(to))?;
    }
    copy_dir(&built.join("locales"), &bundle.join("locales"))?;
    println!("{} ({} files and locales)", bundle.display(), plan.len());
    Ok(Status::Done)
}

/// What to copy from the build directory, and under which name: the bootstrap becomes the product's
/// exe, our DLL takes the name the bootstrap loads (`<exe>.dll`), and the CEF runtime files keep theirs.
fn plan(names: &[String]) -> Result<Vec<(String, String)>> {
    let has = |n: &str| names.iter().any(|name| name == n);
    for required in ["bootstrap.exe", "vsesvit_app.dll", "libcef.dll"] {
        if !has(required) {
            return Err(format!("{required} is missing from the build directory"));
        }
    }
    let runtime = |name: &str| {
        let lower = name.to_ascii_lowercase();
        [".dll", ".pak", ".dat", ".bin", ".json"].iter().any(|ext| lower.ends_with(ext)) && lower != "vsesvit_app.dll"
    };
    let mut plan = vec![("bootstrap.exe".to_owned(), "vsesvit.exe".to_owned()), ("vsesvit_app.dll".to_owned(), "vsesvit.dll".to_owned())];
    let mut files: Vec<&String> = names.iter().filter(|n| runtime(n)).collect();
    files.sort();
    plan.extend(files.into_iter().map(|n| (n.clone(), n.clone())));
    Ok(plan)
}

fn copy(from: &Path, to: &Path) -> Result {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let path: PathBuf = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()))?;
        } else {
            copy(&path, &to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::plan;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_bootstrap_loads_our_dll_by_its_own_name() {
        let built = names(&[
            "bootstrap.exe",
            "bootstrapc.exe",
            "vsesvit_app.dll",
            "vsesvit_app.pdb",
            "libcef.dll",
            "chrome_elf.dll",
            "icudtl.dat",
            "resources.pak",
            "v8_context_snapshot.bin",
            "vk_swiftshader_icd.json",
            "vsesvit_app.dll.lib",
        ]);
        let plan = plan(&built).expect("a plan");
        let pairs: Vec<(&str, &str)> = plan.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        assert_eq!(pairs[..2], [("bootstrap.exe", "vsesvit.exe"), ("vsesvit_app.dll", "vsesvit.dll")]);
        assert!(pairs.contains(&("libcef.dll", "libcef.dll")));
        assert!(pairs.contains(&("icudtl.dat", "icudtl.dat")));
        assert!(pairs.contains(&("vk_swiftshader_icd.json", "vk_swiftshader_icd.json")));
        assert!(!pairs.iter().any(|(from, _)| from.ends_with(".pdb") || from.ends_with(".lib") || *from == "bootstrapc.exe"));
        assert_eq!(pairs.iter().filter(|(_, to)| *to == "vsesvit.dll").count(), 1);
    }

    #[test]
    fn a_missing_part_fails() {
        assert!(plan(&names(&["vsesvit_app.dll", "libcef.dll"])).unwrap_err().contains("bootstrap.exe"));
        assert!(plan(&names(&["bootstrap.exe", "libcef.dll"])).unwrap_err().contains("vsesvit_app.dll"));
    }
}
