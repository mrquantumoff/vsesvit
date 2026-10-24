//! `cargo xtask check-deps`: the layer rules of docs/design/linux-cef/design.md sections 1 and 6.2 as a
//! check, over the workspace member crates only (never `third_party/` or the excluded spikes).
//!
//! - **Normal dependencies.** `vsesvit-engine` reaches no GPUI and no CEF crate; `vsesvit-gpui` reaches no
//!   CEF crate; `vsesvit-cef` reaches no GPUI crate (all three through any chain of normal dependencies);
//!   `vsesvit-cef-child` depends on `cef` alone; no crate turns on `test-support` in a normal dependency.
//! - **Dev-dependencies** may break a crate's rule, or turn on a workspace crate's `test-support`, only
//!   by the allow list: `vsesvit-cef` on `vsesvit-gpui` with `test-support` and on `vsesvit-testenv`
//!   (the smoke example), `vsesvit-gpui` on `vsesvit-testenv`, any crate on `vsesvit-core`.
//! - **Source.** No `unsafe impl Send` or `Sync` in `vsesvit-cef`. Every example `main`, and the `main`
//!   of every `harness = false` test, starts with the test-environment prologue of section 7.3. Every
//!   Rust file of the GPUI + CEF crates starts with its `//! Owned by <unit>.` line (section 8.1).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use quote::ToTokens;
use serde_json::Value;

use crate::Result;

/// The crates the design adds. Their files carry an owner line.
const NEW_CRATES: &[&str] = &[
    "vsesvit-engine",
    "vsesvit-cef",
    "vsesvit-cef-child",
    "vsesvit-gpui",
    "vsesvit-testenv",
    "vsesvit-helper",
    "vsesvit-sandbox",
];

/// The shells wave 7 deletes. Their examples predate the test-environment rule and open windows on
/// the user's display by design; nothing new may be added to them.
const LEGACY_CRATES: &[&str] = &["vsesvit-gtk", "vsesvit-webext"];

/// (crate, dev-dependency, whether it may turn on `test-support`).
const DEV_ALLOWED: &[(&str, &str, bool)] = &[
    ("vsesvit-cef", "vsesvit-gpui", true),
    ("vsesvit-cef", "vsesvit-testenv", false),
    // The smoke example opens the GPUI window that `platform::Surface` attaches to.
    ("vsesvit-cef", "gpui-pre", false),
    ("vsesvit-cef", "gpui-pre-platform", false),
    ("vsesvit-gpui", "vsesvit-testenv", false),
];

fn is_gpui(name: &str) -> bool {
    name.starts_with("gpui") || name == "vsesvit-gpui"
}

fn is_cef(name: &str) -> bool {
    matches!(name, "cef" | "cef-dll-sys" | "vsesvit-cef" | "vsesvit-cef-child")
}

/// What a rule bans, in words, and the test for a dependency's name.
type Ban = (&'static str, fn(&str) -> bool);

/// What a crate may not depend on, if it has a rule.
fn banned(krate: &str) -> Option<Ban> {
    match krate {
        "vsesvit-engine" => Some(("a GPUI or CEF crate", |n| is_gpui(n) || is_cef(n))),
        "vsesvit-gpui" => Some(("a CEF crate", is_cef)),
        "vsesvit-cef" => Some(("a GPUI crate", is_gpui)),
        "vsesvit-cef-child" => Some(("anything but cef", |n| n != "cef")),
        _ => None,
    }
}

struct Dep {
    name: String,
    dev: bool,
    features: Vec<String>,
}

struct Krate {
    name: String,
    dir: PathBuf,
    deps: Vec<Dep>,
    examples: Vec<PathBuf>,
}

pub fn run(args: &[String]) -> Result {
    if !args.is_empty() {
        return Err("usage: cargo xtask check-deps".to_owned());
    }
    let root = crate::workspace_root();
    let metadata = cargo_metadata(&root)?;
    let crates = members(&metadata)?;
    let mut problems = Vec::new();
    problems.extend(manifest_problems(&crates));
    problems.extend(reach_problems(&metadata)?);
    for k in &crates {
        problems.extend(source_problems(k)?);
    }
    if problems.is_empty() {
        println!("check-deps: {} crates, OK", crates.len());
        return Ok(());
    }
    Err(format!("check-deps found {} problem(s):\n  {}", problems.len(), problems.join("\n  ")))
}

fn cargo_metadata(root: &Path) -> Result<Value> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

fn members(metadata: &Value) -> Result<Vec<Krate>> {
    let ids: BTreeSet<&str> = metadata["workspace_members"].as_array().ok_or("no workspace_members")?.iter().filter_map(Value::as_str).collect();
    let mut out = Vec::new();
    for package in metadata["packages"].as_array().ok_or("no packages")? {
        if !ids.contains(package["id"].as_str().unwrap_or_default()) {
            continue;
        }
        let manifest = PathBuf::from(package["manifest_path"].as_str().ok_or("no manifest_path")?);
        let deps = package["dependencies"]
            .as_array()
            .ok_or("no dependencies")?
            .iter()
            .filter(|d| d["kind"].is_null() || d["kind"] == "dev")
            .map(|d| Dep {
                name: d["name"].as_str().unwrap_or_default().to_owned(),
                dev: d["kind"] == "dev",
                features: d["features"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
            })
            .collect();
        // A test that opens a window needs the main thread, so it is a `harness = false` test with
        // its own `main`, held to the examples' prologue.
        let own_main = own_main_tests(&manifest)?;
        let examples = package["targets"]
            .as_array()
            .ok_or("no targets")?
            .iter()
            .filter(|t| {
                t["kind"].as_array().is_some_and(|k| {
                    k.iter().any(|k| k == "example" || (k == "test" && own_main.contains(t["name"].as_str().unwrap_or_default())))
                })
            })
            .filter_map(|t| t["src_path"].as_str().map(PathBuf::from))
            .collect();
        out.push(Krate {
            name: package["name"].as_str().unwrap_or_default().to_owned(),
            dir: manifest.parent().ok_or("a manifest without a directory")?.to_path_buf(),
            deps,
            examples,
        });
    }
    Ok(out)
}

/// The names of the `[[test]]` targets with `harness = false`.
fn own_main_tests(manifest: &Path) -> Result<BTreeSet<String>> {
    let text = read(manifest)?;
    let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let tests = table.get("test").and_then(toml::Value::as_array).into_iter().flatten();
    Ok(tests
        .filter(|t| t.get("harness").and_then(toml::Value::as_bool) == Some(false))
        .filter_map(|t| t.get("name").and_then(toml::Value::as_str).map(str::to_owned))
        .collect())
}

fn manifest_problems(crates: &[Krate]) -> Vec<String> {
    let mut problems = Vec::new();
    for k in crates {
        for d in &k.deps {
            let test_support = d.features.iter().any(|f| f == "test-support");
            let allowed = DEV_ALLOWED.iter().find(|(c, dep, _)| *c == k.name && *dep == d.name);
            // A third-party test-support (gpui's TestAppContext) is fine in tests; ours is allow-listed.
            let ours = d.name.starts_with("vsesvit");
            if test_support && (!d.dev || (ours && !allowed.is_some_and(|a| a.2))) {
                let kind = if d.dev { "dev-dependency" } else { "normal dependency" };
                problems.push(format!("{}: `test-support` on {} as a {kind}", k.name, d.name));
            }
            let Some((what, ban)) = banned(&k.name) else { continue };
            let core = d.dev && d.name == "vsesvit-core";
            if ban(&d.name) && !(d.dev && (allowed.is_some() || core)) {
                let kind = if d.dev { "dev-dependency" } else { "dependency" };
                problems.push(format!("{}: {kind} {} is {what}", k.name, d.name));
            }
        }
    }
    problems
}

/// The bans over every chain of normal dependencies, from the resolved graph (all targets).
fn reach_problems(metadata: &Value) -> Result<Vec<String>> {
    let names: BTreeMap<&str, &str> = metadata["packages"]
        .as_array()
        .ok_or("no packages")?
        .iter()
        .map(|p| (p["id"].as_str().unwrap_or_default(), p["name"].as_str().unwrap_or_default()))
        .collect();
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in metadata["resolve"]["nodes"].as_array().ok_or("no resolve graph")? {
        let id = node["id"].as_str().unwrap_or_default();
        let normal = node["deps"].as_array().into_iter().flatten().filter(|d| {
            d["dep_kinds"].as_array().is_some_and(|ks| ks.iter().any(|k| k["kind"].is_null()))
        });
        edges.insert(id, normal.filter_map(|d| d["pkg"].as_str()).collect());
    }
    let mut problems = Vec::new();
    for (&id, &name) in &names {
        if name == "vsesvit-cef-child" {
            continue; // its rule is about direct dependencies; cef's own tree is cef's business
        }
        let Some((what, ban)) = banned(name) else { continue };
        let mut seen = BTreeSet::new();
        let mut todo = vec![(id, vec![name])];
        while let Some((at, path)) = todo.pop() {
            for &next in edges.get(at).into_iter().flatten() {
                let next_name = names.get(next).copied().unwrap_or_default();
                let mut chain = path.clone();
                chain.push(next_name);
                if ban(next_name) {
                    problems.push(format!("{name} reaches {what}: {}", chain.join(" -> ")));
                } else if seen.insert(next) {
                    todo.push((next, chain));
                }
            }
        }
    }
    Ok(problems)
}

fn source_problems(k: &Krate) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let new = NEW_CRATES.contains(&k.name.as_str());
    let files = if new { rust_files(&k.dir)? } else { Vec::new() };
    for file in &files {
        let text = read(file)?;
        if !owner_line(&text) {
            problems.push(format!("{}: does not start with `//! Owned by <unit>.`", file.display()));
        }
        if k.name == "vsesvit-cef" && unsafe_send_sync(&text) {
            problems.push(format!("{}: `unsafe impl Send` or `Sync` (design section 1.6)", file.display()));
        }
    }
    if LEGACY_CRATES.contains(&k.name.as_str()) {
        return Ok(problems);
    }
    let links_cef = k.deps.iter().any(|d| d.name == "cef" || d.name == "vsesvit-cef");
    for example in &k.examples {
        if let Err(e) = prologue(&read(example)?, links_cef) {
            problems.push(format!("{}: {e}", example.display()));
        }
    }
    Ok(problems)
}

fn owner_line(text: &str) -> bool {
    let first = text.lines().next().unwrap_or_default();
    first.strip_prefix("//! Owned by ").and_then(|r| r.strip_suffix('.')).is_some_and(|unit| {
        let (wave, n) = unit.split_once('.').unwrap_or_default();
        matches!(wave, "W0" | "W1" | "W2" | "W3" | "W4" | "W5" | "W6" | "W7" | "WX" | "C")
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
    })
}

fn unsafe_send_sync(text: &str) -> bool {
    let words: Vec<&str> = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
    words.windows(2).enumerate().any(|(i, w)| {
        w == ["unsafe", "impl"] && words[i + 2..].iter().take(8).take_while(|w| **w != "for").any(|w| *w == "Send" || *w == "Sync")
    })
}

/// Section 7.3: a crate that links CEF starts `main` with the sub-process return, then `require()`;
/// any other starts it with `require()`.
fn prologue(source: &str, links_cef: bool) -> std::result::Result<(), String> {
    let file = syn::parse_file(source).map_err(|e| format!("does not parse: {e}"))?;
    let mains: Vec<&syn::ItemFn> = file
        .items
        .iter()
        .filter_map(|i| match i {
            syn::Item::Fn(f) if f.sig.ident == "main" => Some(f),
            _ => None,
        })
        .collect();
    if mains.is_empty() {
        return Err("no `fn main`".to_owned());
    }
    for main in mains {
        let stmts: Vec<String> = main.block.stmts.iter().map(|s| s.to_token_stream().to_string().replace(' ', "")).collect();
        let mut stmts = stmts.iter();
        if links_cef {
            let first = stmts.next().map(String::as_str).unwrap_or_default();
            if !(first.starts_with("ifletSome(") && first.contains("=vsesvit_cef::run_subprocess(") && first.contains("{returncode;}")) {
                return Err("`main` must start with `if let Some(code) = vsesvit_cef::run_subprocess(..) { return code; }`".to_owned());
            }
        }
        let next = stmts.next().map(String::as_str).unwrap_or_default();
        let call = next.strip_prefix("let").map_or(next, |rest| rest.split_once('=').map_or("", |(_, rhs)| rhs));
        if !call.starts_with("vsesvit_testenv::require()") {
            let place = if links_cef { "next" } else { "first" };
            return Err(format!("the {place} statement of `main` must be `vsesvit_testenv::require()`"));
        }
    }
    Ok(())
}

fn rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let path = entry.map_err(|e| format!("{}: {e}", d.display()))?.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{owner_line, prologue, unsafe_send_sync};

    #[test]
    fn owner_lines() {
        assert!(owner_line("//! Owned by W1.3.\n"));
        assert!(owner_line("//! Owned by WX.1."));
        assert!(owner_line("//! Owned by C.6.\n//! more"));
        assert!(!owner_line("//! Owned by somebody.\n"));
        assert!(!owner_line("\n//! Owned by W1.3.\n"));
        assert!(!owner_line("//! Owned by W1.3\n"));
    }

    #[test]
    fn finds_unsafe_send_and_sync_impls() {
        assert!(unsafe_send_sync("unsafe impl Send for X {}"));
        assert!(unsafe_send_sync("unsafe impl<T> Sync for Holder<T> {}"));
        assert!(!unsafe_send_sync("impl Send for X {}"));
        assert!(!unsafe_send_sync("unsafe impl Foo for Sender {}"));
    }

    #[test]
    fn prologue_without_cef() {
        assert!(prologue("fn main() { let env = vsesvit_testenv::require(); run(env) }", false).is_ok());
        assert!(prologue("fn main() { vsesvit_testenv::require(); }", false).is_ok());
        assert!(prologue("fn main() { open_window(); vsesvit_testenv::require(); }", false).is_err());
        assert!(prologue("fn main() {}", false).is_err());
        assert!(prologue("fn helper() {}", false).is_err());
    }

    #[test]
    fn prologue_with_cef() {
        let good = "fn main() -> ExitCode {
            if let Some(code) = vsesvit_cef::run_subprocess(&Sandbox::none()) { return code; }
            let env = vsesvit_testenv::require();
            run(env)
        }";
        assert!(prologue(good, true).is_ok());
        let guard_first = "fn main() { let env = vsesvit_testenv::require(); }";
        assert!(prologue(guard_first, true).is_err());
        let no_guard = "fn main() -> ExitCode {
            if let Some(code) = vsesvit_cef::run_subprocess(&Sandbox::none()) { return code; }
            run()
        }";
        assert!(prologue(no_guard, true).is_err());
    }

    #[test]
    fn every_cfg_variant_of_main_is_checked() {
        let src = "#[cfg(unix)] fn main() { vsesvit_testenv::require(); }
                   #[cfg(not(unix))] fn main() { eprintln!(\"no\"); }";
        assert!(prologue(src, false).is_err());
    }
}
