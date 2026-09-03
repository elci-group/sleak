//! The Rust adapter: builds an `ArchitectureModel` (sleak-ir) from a
//! real Cargo workspace.
//!
//! This is a **replaceable adapter boundary**, not the semantic source
//! of truth. It combines `cargo_metadata` (fully precise: the crate
//! dependency graph, straight from Cargo's own resolver) with `syn`-based
//! textual parsing of `mod`/`use` items for the module-level import
//! graph, which is a heuristic, not full name resolution — macros, glob
//! re-exports, and `#[path = "..."]` attributes are not resolved.
//! Per the directive (§11/§22), a `furnace`-backed semantic IR should
//! replace the `syn` half of this crate when it exists, without any
//! downstream caller (sleak-graph, sleak-policy, sleak-cli) needing to
//! change — they only ever see the resulting `ArchitectureModel`.

mod crate_graph;
mod module_graph;

use camino::Utf8Path;
use sleak_core::{Result, SleakError};
use sleak_ir::ArchitectureModel;
use std::collections::{BTreeMap, BTreeSet};

/// Builds the full Phase 1 architectural model for the Cargo workspace
/// rooted at `workspace_root`: one `Crate` node + `DependsOn` edge per
/// workspace-member dependency, plus one `Module` node per source file
/// and a best-effort `Imports` edge per resolvable `use` statement.
pub fn build_model(workspace_root: &Utf8Path) -> Result<ArchitectureModel> {
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(SleakError::NotACargoWorkspace {
            path: workspace_root.as_std_path().to_path_buf(),
        });
    }

    let (mut model, crates) = crate_graph::build(workspace_root)?;
    let known_crate_names: BTreeSet<String> = crates.iter().map(|c| c.name.clone()).collect();

    let mut per_crate_files = BTreeMap::new();
    let mut module_registry = BTreeMap::new();
    for info in &crates {
        let files = module_graph::discover_module_files(&info.name, &info.src_root)?;
        for file in &files {
            model.nodes.push(file.to_node());
        }
        module_registry.insert(
            info.name.clone(),
            files
                .iter()
                .map(|f| f.module_path.clone())
                .collect::<BTreeSet<_>>(),
        );
        per_crate_files.insert(info.name.clone(), files);
    }

    for info in &crates {
        for file in &per_crate_files[&info.name] {
            let edges =
                module_graph::import_edges(file, &info.name, &known_crate_names, &module_registry)?;
            model.edges.extend(edges);
        }
    }

    model.canonicalize();
    Ok(model)
}
