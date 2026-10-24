//! `cargo xtask vendor-gpui`: the vendored, patched GPUI backends in `third_party/`
//! (docs/design/linux-cef/design.md section 2.2).
//!
//! `third_party/vendor.toml` records, per crate, the crates.io version, the `.crate`'s SHA-256 and the
//! patch's SHA-256. `Cargo.lock` cannot be the reference: a `[patch]`ed path crate is locked by name and
//! version only. `--check` rebuilds each tree from crates.io and the committed patch and fails on any
//! difference; `--bump` is the only step that writes a tree.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Result;

pub const USAGE: &str = "cargo xtask vendor-gpui (--check | --bump CRATE VERSION)";

/// File path inside the crate (with `/` separators) to its bytes.
type Tree = BTreeMap<String, Vec<u8>>;

#[derive(serde::Deserialize, serde::Serialize)]
struct Entry {
    version: String,
    crate_sha256: String,
    patch_sha256: String,
}

pub fn run(args: &[String]) -> Result {
    let third_party = crate::workspace_root().join("third_party");
    match args {
        [check] if check == "--check" => check_all(&third_party),
        [bump, name, version] if bump == "--bump" => bump_crate(&third_party, name, version),
        _ => Err(USAGE.to_owned()),
    }
}

fn check_all(third_party: &Path) -> Result {
    let entries = read_manifest(third_party)?;
    if entries.is_empty() {
        return Err("third_party/vendor.toml lists no crate".to_owned());
    }
    for (name, entry) in &entries {
        let patch = read(&third_party.join(format!("{name}.patch")))?;
        let patch_sha = hex_sha256(&patch);
        if patch_sha != entry.patch_sha256 {
            return Err(format!("{name}.patch has SHA-256 {patch_sha}, vendor.toml records {}", entry.patch_sha256));
        }
        let expected = patched_tree(name, &entry.version, &entry.crate_sha256, &patch)?;
        let actual = read_tree(&third_party.join(name))?;
        compare(name, &expected, &actual)?;
        println!("{name} {}: matches crates.io plus {name}.patch", entry.version);
    }
    Ok(())
}

fn bump_crate(third_party: &Path, name: &str, version: &str) -> Result {
    let mut entries = read_manifest(third_party)?;
    let patch_path = third_party.join(format!("{name}.patch"));
    let patch = read(&patch_path)?;
    let crate_sha256 = index_checksum(name, version)?;
    let tree = patched_tree(name, version, &crate_sha256, &patch)?;
    let dir = third_party.join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    for (path, bytes) in &tree {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().expect("a file inside the crate has a parent"))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    entries.insert(
        name.to_owned(),
        Entry { version: version.to_owned(), crate_sha256, patch_sha256: hex_sha256(&patch) },
    );
    let text = toml::to_string(&entries).map_err(|e| format!("vendor.toml: {e}"))?;
    let manifest = third_party.join("vendor.toml");
    fs::write(&manifest, format!("{MANIFEST_HEADER}{text}")).map_err(|e| format!("{}: {e}", manifest.display()))?;
    println!("{name} {version}: vendored into {}", dir.display());
    Ok(())
}

const MANIFEST_HEADER: &str = "\
# Written by `cargo xtask vendor-gpui --bump`; checked by `cargo xtask vendor-gpui --check`.
# crate_sha256 comes from the crates.io index at bump time; patch_sha256 is that of <crate>.patch.

";

fn read_manifest(third_party: &Path) -> Result<BTreeMap<String, Entry>> {
    let path = third_party.join("vendor.toml");
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The `.crate` from crates.io, verified against `sha256`, unpacked, with `patch` applied.
fn patched_tree(name: &str, version: &str, sha256: &str, patch: &[u8]) -> Result<Tree> {
    let url = format!("https://static.crates.io/crates/{name}/{name}-{version}.crate");
    let archive = download(&url)?;
    let got = hex_sha256(&archive);
    if got != sha256 {
        return Err(format!("{url}: SHA-256 {got}, expected {sha256}"));
    }
    let mut tree = unpack(&archive, &format!("{name}-{version}/"))?;
    apply_patch(&mut tree, patch).map_err(|e| format!("{name}.patch: {e}"))?;
    Ok(tree)
}

/// The `cksum` of `name` `version` in the crates.io sparse index.
fn index_checksum(name: &str, version: &str) -> Result<String> {
    let prefix = match name.len() {
        1 => "1".to_owned(),
        2 => "2".to_owned(),
        3 => format!("3/{}", &name[..1]),
        _ => format!("{}/{}", &name[..2], &name[2..4]),
    };
    let url = format!("https://index.crates.io/{prefix}/{name}");
    let index = String::from_utf8(download(&url)?).map_err(|e| format!("{url}: {e}"))?;
    #[derive(serde::Deserialize)]
    struct Line {
        vers: String,
        cksum: String,
        yanked: bool,
    }
    for line in index.lines() {
        let line: Line = serde_json::from_str(line).map_err(|e| format!("{url}: {e}"))?;
        if line.vers == version {
            if line.yanked {
                return Err(format!("{name} {version} is yanked"));
            }
            return Ok(line.cksum);
        }
    }
    Err(format!("{name} {version} is not in the crates.io index"))
}

fn download(url: &str) -> Result<Vec<u8>> {
    let mut response = ureq::get(url).call().map_err(|e| format!("{url}: {e}"))?;
    response.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| format!("{url}: {e}"))
}

fn unpack(archive: &[u8], root: &str) -> Result<Tree> {
    let mut tree = Tree::new();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries().map_err(|e| format!("reading the .crate: {e}"))? {
        let mut entry = entry.map_err(|e| format!("reading the .crate: {e}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(|e| format!("reading the .crate: {e}"))?.to_string_lossy().replace('\\', "/");
        let Some(inner) = path.strip_prefix(root) else {
            return Err(format!("the .crate has {path} outside {root}"));
        };
        let inner = inner.to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| format!("{path}: {e}"))?;
        tree.insert(inner, bytes);
    }
    Ok(tree)
}

fn read_tree(dir: &Path) -> Result<Tree> {
    fn walk(dir: &Path, base: &Path, tree: &mut Tree) -> Result {
        for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
            if path.is_dir() {
                walk(&path, base, tree)?;
            } else {
                let rel = path.strip_prefix(base).expect("walked under base").to_string_lossy().replace('\\', "/");
                tree.insert(rel, read(&path)?);
            }
        }
        Ok(())
    }
    let mut tree = Tree::new();
    walk(dir, dir, &mut tree)?;
    Ok(tree)
}

fn compare(name: &str, expected: &Tree, actual: &Tree) -> Result {
    let mut problems = Vec::new();
    for (path, bytes) in expected {
        match actual.get(path) {
            None => problems.push(format!("missing {path}")),
            Some(b) if b != bytes => problems.push(format!("differs: {path}")),
            Some(_) => {}
        }
    }
    problems.extend(actual.keys().filter(|p| !expected.contains_key(*p)).map(|p| format!("not in crates.io plus the patch: {p}")));
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!("third_party/{name} is not crates.io plus {name}.patch:\n  {}", problems.join("\n  ")))
}

/// Applies a unified diff whose paths are `a/<path in the crate>` and `b/<path in the crate>`. Strict:
/// every hunk must match at its stated line, with no offset and no fuzz.
fn apply_patch(tree: &mut Tree, patch: &[u8]) -> Result {
    let text = std::str::from_utf8(patch).map_err(|e| e.to_string())?;
    let mut lines = text.split_inclusive('\n').peekable();
    let mut files = 0;
    while let Some(line) = lines.next() {
        let Some(old) = line.strip_prefix("--- ") else { continue };
        let new = lines.next().and_then(|l| l.strip_prefix("+++ ")).ok_or("a --- line without +++")?;
        let old_path = patch_path(old, "a/")?;
        let new_path = patch_path(new, "b/")?;
        if old_path != new_path {
            return Err(format!("renames are not supported: {old_path} -> {new_path}"));
        }
        let original = tree.get(&old_path).ok_or_else(|| format!("{old_path} is not in the crate"))?;
        let original = std::str::from_utf8(original).map_err(|e| format!("{old_path}: {e}"))?;
        let source: Vec<&str> = original.split_inclusive('\n').collect();
        let mut out = String::with_capacity(original.len() + 4096);
        let mut next = 0; // index into `source` of the first line not yet copied
        while lines.peek().is_some_and(|l| l.starts_with("@@ ")) {
            let header = lines.next().expect("peeked");
            let start = hunk_start(header)?;
            if start < next {
                return Err(format!("{old_path}: overlapping hunk {}", header.trim_end()));
            }
            out.extend(source[next..start].iter().copied());
            next = start;
            let mut last = ' ';
            while let Some(&l) = lines.peek() {
                let (kind, body) = match l.chars().next() {
                    Some(c @ (' ' | '-' | '+')) => (c, &l[1..]),
                    // "\ No newline at end of file" applies to the line before it.
                    Some('\\') => {
                        lines.next();
                        if last != '-' && out.ends_with('\n') {
                            out.pop();
                        }
                        continue;
                    }
                    _ => break,
                };
                lines.next();
                last = kind;
                if kind == '+' {
                    out.push_str(body);
                    continue;
                }
                let have = source.get(next).copied().unwrap_or_default();
                if have.trim_end_matches('\n') != body.trim_end_matches('\n') {
                    return Err(format!("{old_path}:{}: expected {:?}, found {:?}", next + 1, body.trim_end(), have.trim_end()));
                }
                if kind == ' ' {
                    out.push_str(have);
                }
                next += 1;
            }
        }
        out.extend(source[next..].iter().copied());
        tree.insert(old_path, out.into_bytes());
        files += 1;
    }
    if files == 0 {
        return Err("the patch changes no file".to_owned());
    }
    Ok(())
}

fn patch_path(line: &str, prefix: &str) -> Result<String> {
    let path = line.trim_end().split('\t').next().unwrap_or_default();
    path.strip_prefix(prefix).map(str::to_owned).ok_or_else(|| format!("patch path {path:?} does not start with {prefix}"))
}

/// The 0-based index of the first old line of a `@@ -start,len +start,len @@` hunk.
fn hunk_start(header: &str) -> Result<usize> {
    let old = header.split_whitespace().nth(1).and_then(|s| s.strip_prefix('-')).ok_or("a malformed @@ line")?;
    let start: usize = old.split(',').next().unwrap_or_default().parse().map_err(|_| format!("a malformed @@ line: {header}"))?;
    let len = old.split(',').nth(1).map_or(Ok(1), str::parse::<usize>).map_err(|_| format!("a malformed @@ line: {header}"))?;
    Ok(if len == 0 { start } else { start - 1 })
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{Tree, apply_patch};

    fn tree(text: &str) -> Tree {
        Tree::from([("src/a.rs".to_owned(), text.as_bytes().to_vec())])
    }

    const PATCH: &str = "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -2,3 +2,4 @@\n two\n-three\n+THREE\n+three and a half\n four\n";

    #[test]
    fn applies_a_hunk_at_its_line() {
        let mut t = tree("one\ntwo\nthree\nfour\nfive\n");
        apply_patch(&mut t, PATCH.as_bytes()).unwrap();
        assert_eq!(t["src/a.rs"], b"one\ntwo\nTHREE\nthree and a half\nfour\nfive\n");
    }

    #[test]
    fn refuses_a_hunk_whose_context_moved() {
        let mut t = tree("zero\none\ntwo\nthree\nfour\nfive\n");
        let err = apply_patch(&mut t, PATCH.as_bytes()).unwrap_err();
        assert!(err.contains("src/a.rs:2"), "{err}");
    }

    #[test]
    fn refuses_a_file_not_in_the_crate() {
        let mut t = Tree::new();
        assert!(apply_patch(&mut t, PATCH.as_bytes()).unwrap_err().contains("not in the crate"));
    }

    #[test]
    fn honours_no_newline_at_end_of_file() {
        let mut t = tree("x\n");
        let patch = "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-x\n+y\n\\ No newline at end of file\n";
        apply_patch(&mut t, patch.as_bytes()).unwrap();
        assert_eq!(t["src/a.rs"], b"y");
    }
}
