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
