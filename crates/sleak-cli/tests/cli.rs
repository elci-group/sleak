use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

fn sleak() -> Command {
    Command::cargo_bin("sleak").unwrap()
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn workspace_cargo_toml(members: &[&str]) -> String {
    format!(
        "[workspace]\nresolver = \"2\"\nmembers = [{}]\n",
        members
            .iter()
            .map(|m| format!("\"{m}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn crate_cargo_toml(name: &str, deps: &[&str]) -> String {
    let deps_block = deps
        .iter()
        .map(|d| format!("{d} = {{ path = \"../{d}\" }}\n"))
        .collect::<String>();
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps_block}"
    )
}

/// A two-crate workspace where neither crate approaches the default
/// fan-in/fan-out/cycle thresholds: `check` should pass cleanly.
///
/// Centrality and blast-radius are deliberately relaxed via
/// `.sleak.toml`: those metrics are relative to overall graph size, and
/// in a 4-node graph (2 crates + 2 modules) any node with above-average
/// influence trips the directive's own default thresholds by
/// construction — that's the metric working correctly on a tiny graph,
/// not something this fan-in/out/cycle-focused test is about.
#[test]
fn healthy_workspace_has_no_findings_and_check_passes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&["a", "b"]));
    write(&root.join("a/Cargo.toml"), &crate_cargo_toml("a", &[]));
    write(&root.join("a/src/lib.rs"), "pub fn hello() {}\n");
    write(&root.join("b/Cargo.toml"), &crate_cargo_toml("b", &["a"]));
    write(
        &root.join("b/src/lib.rs"),
        "pub fn call() { a::hello(); }\n",
    );
    write(
        &root.join(".sleak.toml"),
        "[policy]\nmax_centrality = 1.0\nmax_blast_radius = 1.0\n",
    );

    sleak()
        .arg("--format")
        .arg("json")
        .arg("scan")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"findings\": []"));

    sleak().arg("check").arg(root).assert().success();
}

/// One crate depended on by four others, with `max_fan_in` configured
/// low enough (via a temp `.sleak.toml`) that it must fire SLK001, and
/// `check` must fail the build.
#[test]
fn fan_in_hub_triggers_slk001_and_fails_check() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let members = ["hub", "a", "b", "c", "d"];
    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&members));
    write(&root.join("hub/Cargo.toml"), &crate_cargo_toml("hub", &[]));
    write(&root.join("hub/src/lib.rs"), "pub fn shared() {}\n");
    for name in ["a", "b", "c", "d"] {
        write(
            &root.join(format!("{name}/Cargo.toml")),
            &crate_cargo_toml(name, &["hub"]),
        );
        write(
            &root.join(format!("{name}/src/lib.rs")),
            "pub fn call() { hub::shared(); }\n",
        );
    }
    write(
        &root.join(".sleak.toml"),
        "[policy]\nmax_fan_in = 2\ncycles = \"error\"\n",
    );

    sleak()
        .arg("scan")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("SLK001"))
        .stdout(predicate::str::contains("crate:hub"));

    sleak().arg("check").arg(root).assert().code(1);
}

/// A single crate whose two modules import each other — a module-level
/// `use` cycle, which SLK003 must detect (crate-level cycles are
/// structurally impossible since Cargo forbids them).
#[test]
fn module_use_cycle_triggers_slk003() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&["cyclic"]));
    write(
        &root.join("cyclic/Cargo.toml"),
        &crate_cargo_toml("cyclic", &[]),
    );
    write(&root.join("cyclic/src/lib.rs"), "mod foo;\nmod bar;\n");
    write(
        &root.join("cyclic/src/foo.rs"),
        "use crate::bar::Bar;\npub struct Foo;\n",
    );
    write(
        &root.join("cyclic/src/bar.rs"),
        "use crate::foo::Foo;\npub struct Bar;\n",
    );

    sleak()
        .arg("scan")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("SLK003"));

    sleak().arg("check").arg(root).assert().code(1);
}

/// Pointing sleak at a directory with no `Cargo.toml` is a
/// configuration error (exit code 2), not an analysis failure.
#[test]
fn non_cargo_directory_is_a_configuration_error() {
    let dir = tempfile::tempdir().unwrap();

    sleak()
        .arg("scan")
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a Cargo workspace"));
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("git must be on PATH to run this test");
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}

/// A crate whose single module file is touched by many more commits
/// than any other file: with a low `max_change_frequency`, that module
/// should trip SLK009. A workspace with no `.git` at all (the other
/// fixtures in this file) must keep working exactly as before —
/// history is optional evidence, not a requirement.
#[test]
fn hot_file_across_many_commits_triggers_slk009() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&["hot"]));
    write(&root.join("hot/Cargo.toml"), &crate_cargo_toml("hot", &[]));
    write(&root.join("hot/src/lib.rs"), "pub fn v0() {}\n");
    write(
        &root.join(".sleak.toml"),
        "[policy]\nmax_change_frequency = 0.5\n[history]\nwindow = 50\n",
    );

    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "Sleak Test"]);
    git(root, &["config", "user.email", "sleak-test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "initial"]);

    for i in 1..8 {
        write(
            &root.join("hot/src/lib.rs"),
            &format!("pub fn v{i}() {{}}\n"),
        );
        git(root, &["add", "hot/src/lib.rs"]);
        git(root, &["commit", "--quiet", "-m", &format!("edit {i}")]);
    }

    sleak()
        .arg("scan")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("SLK009"))
        .stdout(predicate::str::contains("module:hot"));
}

/// A non-git workspace (every other fixture here) must still scan
/// cleanly with history enabled by default — the absence of `.git` is
/// not an error, and no SLK009 finding is fabricated from nothing.
#[test]
fn non_git_workspace_scans_without_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&["a"]));
    write(&root.join("a/Cargo.toml"), &crate_cargo_toml("a", &[]));
    write(&root.join("a/src/lib.rs"), "pub fn hello() {}\n");

    sleak()
        .arg("scan")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("SLK009").not());
}

fn hub_workspace(root: &Path, hub: &str, dependants: &[&str]) {
    write(
        &root.join(format!("{hub}/Cargo.toml")),
        &crate_cargo_toml(hub, &[]),
    );
    write(
        &root.join(format!("{hub}/src/lib.rs")),
        "pub fn shared() {}\n",
    );
    for name in dependants {
        write(
            &root.join(format!("{name}/Cargo.toml")),
            &crate_cargo_toml(name, &[hub]),
        );
        write(
            &root.join(format!("{name}/src/lib.rs")),
            &format!("pub fn call() {{ {hub}::shared(); }}\n"),
        );
    }
}

/// `sleak baseline create` records current findings as accepted debt:
/// `check` then passes even though the same finding is still there, but
/// still blocks on a genuinely new one introduced afterward.
#[test]
fn baseline_accepts_existing_debt_but_still_blocks_on_new_debt() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let members = ["hub", "a", "b", "c"];
    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&members));
    hub_workspace(root, "hub", &["a", "b", "c"]);
    write(&root.join(".sleak.toml"), "[policy]\nmax_fan_in = 2\n");

    sleak()
        .arg("baseline")
        .arg("create")
        .arg(root)
        .assert()
        .success();
    assert!(root.join(".sleak-baseline.json").exists());

    // Same debt, already accepted: check passes.
    sleak().arg("check").arg(root).assert().success();

    // Introduce genuinely new, unrelated debt.
    let all_members = ["hub", "a", "b", "c", "hub2", "p", "q", "r"];
    write(
        &root.join("Cargo.toml"),
        &workspace_cargo_toml(&all_members),
    );
    hub_workspace(root, "hub2", &["p", "q", "r"]);

    // Adding a whole new, disconnected hub subgraph also shifts blast
    // radius for the rest of the graph — a real, honest side effect,
    // not just the fan-in finding on hub2 — so this only pins the
    // outcome that matters here: check now blocks, and the new hub is
    // named as new debt (not just re-reported as already-accepted).
    sleak()
        .arg("check")
        .arg(root)
        .assert()
        .code(1)
        .stdout(predicate::str::contains("crate:hub2"))
        .stdout(predicate::str::contains("2 existing (accepted)"));
}

/// `sleak diff <ref>` compares the working tree against a past commit
/// without touching it, and reports a newly-introduced finding as a
/// regression.
#[test]
fn diff_detects_a_regression_introduced_in_a_later_commit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    write(
        &root.join("Cargo.toml"),
        &workspace_cargo_toml(&["hub", "a"]),
    );
    hub_workspace(root, "hub", &["a"]);
    write(&root.join(".sleak.toml"), "[policy]\nmax_fan_in = 2\n");

    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "Sleak Test"]);
    git(root, &["config", "user.email", "sleak-test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "healthy"]);

    // A second commit adds enough dependants to trip SLK001.
    let members = ["hub", "a", "b", "c"];
    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&members));
    write(&root.join("b/Cargo.toml"), &crate_cargo_toml("b", &["hub"]));
    write(
        &root.join("b/src/lib.rs"),
        "pub fn call() { hub::shared(); }\n",
    );
    write(&root.join("c/Cargo.toml"), &crate_cargo_toml("c", &["hub"]));
    write(
        &root.join("c/src/lib.rs"),
        "pub fn call() { hub::shared(); }\n",
    );
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "add more dependants"]);

    sleak()
        .arg("diff")
        .arg("HEAD~1")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("SLK001"))
        .stdout(predicate::str::contains("crate:hub"))
        .stdout(predicate::str::contains("Verdict: REGRESSION"));
}

/// Diffing a working tree against itself finds no differences.
#[test]
fn diff_against_head_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&["a"]));
    write(&root.join("a/Cargo.toml"), &crate_cargo_toml("a", &[]));
    write(&root.join("a/src/lib.rs"), "pub fn hello() {}\n");

    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "Sleak Test"]);
    git(root, &["config", "user.email", "sleak-test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "initial"]);

    sleak()
        .arg("diff")
        .arg("HEAD")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("Verdict: UNCHANGED"));
}

/// A ref that doesn't exist is an infrastructure/tooling failure
/// (exit code 4), not a silent empty diff.
#[test]
fn diff_against_an_unknown_ref_is_an_infrastructure_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    write(&root.join("Cargo.toml"), &workspace_cargo_toml(&["a"]));
    write(&root.join("a/Cargo.toml"), &crate_cargo_toml("a", &[]));
    write(&root.join("a/src/lib.rs"), "pub fn hello() {}\n");

    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "Sleak Test"]);
    git(root, &["config", "user.email", "sleak-test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "initial"]);

    sleak()
        .arg("diff")
        .arg("not-a-real-ref")
        .arg(root)
        .assert()
        .code(4);
}
