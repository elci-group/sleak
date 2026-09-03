use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{MetadataCommand, Package, PackageId};
use sleak_core::{EdgeKind, NodeId, NodeKind, Result, SleakError};
use sleak_ir::{ArchitectureModel, Edge, Node};
use std::collections::{BTreeMap, BTreeSet};

pub struct CrateInfo {
    pub name: String,
    pub src_root: Utf8PathBuf,
}

pub fn crate_node_id(name: &str) -> NodeId {
    NodeId::new(format!("crate:{name}"))
}

/// Runs `cargo metadata --offline` at `workspace_root` and turns the
/// resolved dependency graph into `Crate` nodes and `DependsOn` edges,
/// restricted to workspace members — external dependencies aren't
/// graphed as nodes in Phase 1, to avoid burying the workspace's own
/// structure under its leaf dependency tree.
pub fn build(workspace_root: &Utf8Path) -> Result<(ArchitectureModel, Vec<CrateInfo>)> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .other_options(["--offline".to_string()])
        .exec()
        .map_err(|e| SleakError::MetadataFailure {
            path: workspace_root.as_std_path().to_path_buf(),
            reason: e.to_string(),
        })?;

    let member_ids: BTreeSet<PackageId> = metadata.workspace_members.iter().cloned().collect();
    let packages_by_id: BTreeMap<PackageId, &Package> = metadata
        .packages
        .iter()
        .map(|p| (p.id.clone(), p))
        .collect();

    let mut model = ArchitectureModel::new();
    let mut infos = Vec::new();

    for id in &member_ids {
        let Some(pkg) = packages_by_id.get(id) else {
            continue;
        };
        let name = pkg.name.to_string();
        model.nodes.push(Node {
            id: crate_node_id(&name),
            kind: NodeKind::Crate,
            label: name.clone(),
        });
        let src_root = pkg
            .manifest_path
            .parent()
            .map(|p| p.join("src"))
            .unwrap_or_else(|| Utf8PathBuf::from("src"));
        infos.push(CrateInfo { name, src_root });
    }

    if let Some(resolve) = &metadata.resolve {
        for node in &resolve.nodes {
            if !member_ids.contains(&node.id) {
                continue;
            }
            let Some(from_pkg) = packages_by_id.get(&node.id) else {
                continue;
            };
            for dep in &node.deps {
                if !member_ids.contains(&dep.pkg) {
                    continue;
                }
                let Some(to_pkg) = packages_by_id.get(&dep.pkg) else {
                    continue;
                };
                model.edges.push(Edge {
                    from: crate_node_id(from_pkg.name.as_ref()),
                    to: crate_node_id(to_pkg.name.as_ref()),
                    kind: EdgeKind::DependsOn,
                });
            }
        }
    }

    infos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((model, infos))
}
