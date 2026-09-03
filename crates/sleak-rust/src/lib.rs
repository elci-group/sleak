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

use camino::{Utf8Path, Utf8PathBuf};
use sleak_core::{EdgeKind, NodeId, Result, SleakError};
use sleak_ir::{ArchitectureModel, Edge};
use std::collections::{BTreeMap, BTreeSet};

/// Workspace-root-relative source-file path -> the module node it maps
/// to. Built alongside the model (not recomputed separately) so it can
/// never drift from it. Its purpose is correlating git history (which
/// reports paths this way) with the architectural graph — see
/// `sleak-git` and `sleak_policy::evaluate_change_concentration`.
pub type FileIndex = BTreeMap<Utf8PathBuf, NodeId>;

/// Builds the full architectural model for the Cargo workspace rooted at
/// `workspace_root`: one `Crate` node + `DependsOn` edge per
/// workspace-member dependency, one `Module` node per source file with
/// a best-effort `Imports` edge per resolvable `use` statement, and an
/// `Owns` edge from each crate to each of its own modules.
///
/// `Owns` edges exist purely to keep the graph connected across
/// granularities for centrality/blast-radius purposes — without them, a
/// combined crate+module graph is really several disconnected
/// subgraphs (nothing links `crate:foo` to `module:foo::bar`), which
/// makes "what breaks if this is removed" meaningless at the whole-
/// architecture scale. `sleak-graph` deliberately excludes `Owns` edges
/// from fan-in/fan-out/cycle detection, where they'd only add noise.
///
/// Also returns a [`FileIndex`]. Building it here assumes the git
/// repository root (if any) coincides with `workspace_root` — where it
/// doesn't, the index simply won't match any git-reported path, and
/// history-based findings silently won't fire for that repository
/// rather than firing on wrong data.
pub fn build_model(workspace_root: &Utf8Path) -> Result<(ArchitectureModel, FileIndex)> {
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(SleakError::NotACargoWorkspace {
            path: workspace_root.as_std_path().to_path_buf(),
        });
    }
    let canonical_root = workspace_root
        .canonicalize_utf8()
        .map_err(|source| SleakError::Io {
            path: workspace_root.as_std_path().to_path_buf(),
            source,
        })?;

    let (mut model, crates) = crate_graph::build(workspace_root)?;
    // Rust identifier form (as it appears in `use` paths) -> Cargo
    // package name (may be hyphenated) — see `module_graph::resolve_use`.
    let known_crate_names: BTreeMap<String, String> = crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), c.name.clone()))
        .collect();

    let mut per_crate_files = BTreeMap::new();
    let mut module_registry = BTreeMap::new();
    let mut file_index = FileIndex::new();
    for info in &crates {
        let files = module_graph::discover_module_files(&info.name, &info.src_root)?;
        let crate_id = crate_graph::crate_node_id(&info.name);
        for file in &files {
            model.nodes.push(file.to_node());
            model.edges.push(Edge {
                from: crate_id.clone(),
                to: file.node_id.clone(),
                kind: EdgeKind::Owns,
            });
            if let Ok(relative) = file.file_path.strip_prefix(&canonical_root) {
                file_index.insert(relative.to_path_buf(), file.node_id.clone());
            }
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
    Ok((model, file_index))
}
