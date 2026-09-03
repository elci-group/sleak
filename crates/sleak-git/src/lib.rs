//! Git history ingestion (directive §9): the raw material for detecting
//! architectural locations that repeatedly attract change.
//!
//! Shells out to the `git` CLI rather than linking `git2`, matching
//! Scrawny's own git abstraction ("a `git2`-backed implementation can
//! replace this later without touching any caller"). This crate knows
//! nothing about Sleak's architectural graph — it only reports which
//! files changed in which commits; correlating that with the graph is
//! the caller's job.
//!
//! Git history is optional evidence, not a requirement (directive §9
//! says Sleak "SHOULD" consume it "where available"): every function
//! here degrades to an empty result — never an error — when `root`
//! isn't a git repository, `git` isn't on `PATH`, or there's no history
//! yet. Core analysis stays offline and functional either way (§24).

use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;

/// One commit's changed files, in the order `git log` reported them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitChange {
    pub sha: String,
    pub files: Vec<Utf8PathBuf>,
}

/// The most recent `window` commits reachable from `HEAD` at `root`,
/// most-recent-first, with each commit's changed files (paths relative
/// to `root` — this assumes `root` is itself the repository's top
/// level, which holds whenever the Cargo workspace root and the git
/// repository root coincide; where they don't, paths simply won't
/// correlate and no history findings will fire for that repository).
///
/// Returns an empty `Vec` — deterministically, never an `Err` — if
/// history isn't available for any reason.
pub fn recent_changes(root: &Utf8Path, window: usize) -> Vec<CommitChange> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("log")
        .arg(format!("--max-count={window}"))
        .arg("--name-only")
        .arg("--pretty=format:%H")
        .arg("--no-color")
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return Vec::new();
    };

    parse_log(&text)
}

fn parse_log(text: &str) -> Vec<CommitChange> {
    let mut commits = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        let sha = line.to_string();
        let mut files = Vec::new();
        while let Some(&next) = lines.peek() {
            if next.is_empty() {
                break;
            }
            files.push(Utf8PathBuf::from(next));
            lines.next();
        }
        commits.push(CommitChange { sha, files });
    }
    commits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command as StdCommand;

    fn git(dir: &Path, args: &[&str]) {
        let status = StdCommand::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .expect("git must be on PATH to run this test");
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "--quiet"]);
        git(dir, &["config", "user.name", "Sleak Test"]);
        git(dir, &["config", "user.email", "sleak-test@example.com"]);
    }

    fn commit(dir: &Path, file: &str, contents: &str, message: &str) {
        std::fs::write(dir.join(file), contents).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "--quiet", "-m", message]);
    }

    #[test]
    fn non_git_directory_yields_no_history() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();
        assert!(recent_changes(root, 500).is_empty());
    }

    #[test]
    fn repo_with_no_commits_yields_no_history() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let root = Utf8Path::from_path(dir.path()).unwrap();
        assert!(recent_changes(root, 500).is_empty());
    }

    #[test]
    fn reports_commits_most_recent_first_with_their_files() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "one", "first");
        commit(dir.path(), "b.txt", "two", "second");

        let root = Utf8Path::from_path(dir.path()).unwrap();
        let commits = recent_changes(root, 500);

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].files, vec![Utf8PathBuf::from("b.txt")]);
        assert_eq!(commits[1].files, vec![Utf8PathBuf::from("a.txt")]);
        assert_ne!(commits[0].sha, commits[1].sha);
    }

    #[test]
    fn window_limits_how_far_back_history_goes() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        for i in 0..5 {
            commit(dir.path(), "f.txt", &i.to_string(), &format!("commit {i}"));
        }

        let root = Utf8Path::from_path(dir.path()).unwrap();
        assert_eq!(recent_changes(root, 2).len(), 2);
        assert_eq!(recent_changes(root, 500).len(), 5);
    }

    #[test]
    fn a_commit_touching_multiple_files_lists_all_of_them() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        git(dir.path(), &["add", "a.txt", "b.txt"]);
        git(dir.path(), &["commit", "--quiet", "-m", "two files"]);

        let root = Utf8Path::from_path(dir.path()).unwrap();
        let commits = recent_changes(root, 500);

        assert_eq!(commits.len(), 1);
        assert_eq!(
            commits[0].files,
            vec![Utf8PathBuf::from("a.txt"), Utf8PathBuf::from("b.txt")]
        );
    }
}
