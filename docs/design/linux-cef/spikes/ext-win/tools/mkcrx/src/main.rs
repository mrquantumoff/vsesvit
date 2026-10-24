//! mkcrx <key.pem> <dir> <out.crx>: pack <dir> into a CRX3 signed with <key.pem>, using
//! vsesvit-core's own testkit writer. Prints the extension id the key derives to.
use std::path::Path;
use vsesvit_core::testkit::{CrxKey, write_crx3};

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        if rel == "_metadata" || rel.starts_with("_metadata/") { continue; }
        if p.is_dir() { walk(root, &p, out) } else { out.push((rel, std::fs::read(&p).unwrap())) }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let key = CrxKey::rsa_pem(&std::fs::read_to_string(&a[1]).unwrap());
    let mut files = Vec::new();
    walk(Path::new(&a[2]), Path::new(&a[2]), &mut files);
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    std::fs::write(&a[3], write_crx3(&refs, &key)).unwrap();
    println!("{}", key.extension_id().as_str());
}
