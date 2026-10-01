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

/// A tag or branch can be moved to other code, which would then run beside the signing key; a
/// commit cannot. Every workflow follows the rule, not only this one.
#[test]
fn actions_are_pinned_to_commits() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            let line = line.trim_start().trim_start_matches("- ");
            let Some(action) = line.strip_prefix("uses:").map(str::trim) else { continue };
            if action.starts_with("./") || action.starts_with("docker://") {
                continue;
            }
            let commit = action.split_once('@').map_or("", |(_, rest)| rest.split_whitespace().next().unwrap_or(""));
            assert!(
                commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
                "{}: {action} is not pinned to a commit",
                path.display()
            );
        }
    }
}

/// Only plan, which tags, and release, which publishes, can write to the repository, and no
/// checkout leaves the token in .git/config for a build script to find.
#[test]
fn only_the_jobs_that_tag_and_publish_can_write_to_the_repository() {
    let workflow = workflow();
    let (head, _) = workflow.split_once("\njobs:\n").unwrap();
    let (_, permissions) = head.split_once("\npermissions:\n").expect("workflow-level permissions");
    let permissions: Vec<_> = permissions.lines().take_while(|line| line.starts_with("  ")).collect();
    assert_eq!(permissions, ["  contents: read"]);

    let jobs = jobs(&workflow);
    let writers: Vec<_> = jobs.iter().filter(|(_, job)| job.contains("contents: write")).map(|(name, _)| name).collect();
    assert_eq!(writers, ["plan", "release"]);
    for (name, job) in &jobs {
        for step in job.split("\n      - ").filter(|step| step.contains("actions/checkout@")) {
            assert!(step.contains("persist-credentials: false"), "{name} leaves the token on disk:\n{step}");
        }
    }
}
