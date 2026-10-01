//! Checks on .github/workflows/release.yml, whose jobs hold the updater signing key and a token
//! that can write to the repository.

use std::path::Path;

fn workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/release.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

/// Each job's name and lines, comments left out. A job starts at a key indented by two spaces
/// under `jobs:`.
fn jobs(workflow: &str) -> Vec<(String, String)> {
    let (_, body) = workflow.split_once("\njobs:\n").expect("the workflow has jobs");
    let mut jobs: Vec<(String, String)> = Vec::new();
    for line in body.lines().filter(|line| !line.trim_start().starts_with('#')) {
        match line.strip_prefix("  ").and_then(|key| key.strip_suffix(':')) {
            Some(name) if !name.starts_with(' ') => jobs.push((name.to_owned(), String::new())),
            _ => {
                let (_, job) = jobs.last_mut().expect("a line before the first job");
                job.push_str(line);
                job.push('\n');
            }
        }
    }
    jobs
}

/// A job that runs cargo runs the build scripts and proc-macros of everything it builds, and any
/// of them could read the updater signing key from the environment and send it away.
#[test]
fn the_signing_key_is_only_in_a_job_that_runs_no_cargo() {
    let jobs = jobs(&workflow());
    let holding: Vec<_> = jobs.iter().filter(|(_, job)| job.contains("TAURI_SIGNING_PRIVATE_KEY")).collect();
    assert!(!holding.is_empty(), "some job signs the updater artifacts");
    for (name, job) in holding {
        assert!(!job.contains("cargo"), "{name} holds the signing key and runs cargo:\n{job}");
        assert!(job.contains("xtask sign "), "{name} holds the signing key but does not sign:\n{job}");
    }
}
