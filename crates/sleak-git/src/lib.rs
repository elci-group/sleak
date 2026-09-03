//! Git history ingestion (directive §9) and point-in-time snapshots for
//! differential analysis (directive §17).
//!
//! Shells out to the `git` CLI rather than linking `git2`, matching
//! Scrawny's own git abstraction ("a `git2`-backed implementation can
//! replace this later without touching any caller"). This crate knows
//! nothing about Sleak's architectural graph — it only reports which
//! files changed in which commits, or extracts a ref's file tree;
//! correlating either with the graph is the caller's job.
//!
//! [`recent_changes`] treats git history as optional evidence, not a
//! requirement (directive §9 says Sleak "SHOULD" consume it "where
//! available"): it degrades to an empty result — never an error — when
//! `root` isn't a git repository, `git` isn't on `PATH`, or there's no
//! history yet. Core analysis stays offline and functional either way
//! (§24). [`archive_ref`] is different: a caller that asked to diff
//! against a specific ref needs to know if that ref doesn't exist, so
//! it returns a real error instead of degrading silently.

use camino::{Utf8Path, Utf8PathBuf};
use sleak_core::{Result as SleakResult, SleakError};
use std::process::{Command, Stdio};

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

/// Extracts the file tree of `git_ref` (as it existed at that commit,
/// no `.git` directory, no working-tree state) into `dest`, which must
/// already exist and be empty. Used for differential analysis (§17):
/// building an `ArchitectureModel` from `dest` gives the architecture
/// exactly as it was at `git_ref`, without touching the real working
/// tree at `root` in any way — no checkout, no stash, no branch switch.
///
/// Unlike [`recent_changes`], failure here is reported, not swallowed:
/// a caller that named a specific ref needs to know if it doesn't
/// exist, or if `root` isn't a repository at all.
pub fn archive_ref(root: &Utf8Path, git_ref: &str, dest: &Utf8Path) -> SleakResult<()> {
    let command_desc = format!("git -C {root} archive {git_ref}");
    let mut git_child = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("archive")
        .arg(git_ref)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|source| SleakError::GitFailure {
            command: command_desc.clone(),
            reason: source.to_string(),
        })?;

    let git_stdout = git_child.stdout.take().expect("stdout was piped");
    let tar_status = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dest)
        .stdin(git_stdout)
        .status()
        .map_err(|source| SleakError::GitFailure {
            command: format!("tar -x -C {dest}"),
            reason: source.to_string(),
        })?;

    let git_status = git_child.wait().map_err(|source| SleakError::GitFailure {
        command: command_desc.clone(),
        reason: source.to_string(),
    })?;

    if !git_status.success() {
        return Err(SleakError::GitFailure {
            command: command_desc,
            reason: format!("exited with {git_status}"),
        });
    }
    if !tar_status.success() {
        return Err(SleakError::GitFailure {
            command: format!("tar -x -C {dest}"),
            reason: format!("exited with {tar_status}"),
        });
    }
    Ok(())
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

    #[test]
    fn archive_ref_extracts_the_files_as_they_were_at_that_commit() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        commit(repo.path(), "f.txt", "version one", "first");
        let sha_one = recent_changes(Utf8Path::from_path(repo.path()).unwrap(), 1)[0]
            .sha
            .clone();
        commit(repo.path(), "f.txt", "version two", "second");

        let dest = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(repo.path()).unwrap();
        let dest_path = Utf8Path::from_path(dest.path()).unwrap();
        archive_ref(root, &sha_one, dest_path).unwrap();

        let extracted = std::fs::read_to_string(dest.path().join("f.txt")).unwrap();
        assert_eq!(extracted, "version one");
    }

    #[test]
    fn archive_ref_reports_an_unknown_ref_as_an_error() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        commit(repo.path(), "f.txt", "one", "first");

        let dest = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(repo.path()).unwrap();
        let dest_path = Utf8Path::from_path(dest.path()).unwrap();

        assert!(archive_ref(root, "not-a-real-ref", dest_path).is_err());
    }

    #[test]
    fn archive_ref_reports_a_non_repository_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();
        let dest_path = Utf8Path::from_path(dest.path()).unwrap();

        assert!(archive_ref(root, "HEAD", dest_path).is_err());
    }
}
