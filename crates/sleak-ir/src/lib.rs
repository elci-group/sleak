//! The architectural intermediate representation: plain, serializable
//! node/edge data (directive §3). This crate holds no graph library and
//! no algorithms — `sleak-graph` is where the IR gets loaded into a real
//! graph structure and queried.

use serde::{Deserialize, Serialize};
use sleak_core::{EdgeKind, NodeId, NodeKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Human-readable label (e.g. crate name, or `foo::bar` module path).
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

/// A complete architectural graph, always kept sorted so that
/// construction order (filesystem traversal, `cargo metadata` output
/// order, ...) never leaks into results (directive §2.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureModel {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl ArchitectureModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sorts nodes by id and edges by `(from, kind, to)`, and removes
    /// exact-duplicate edges. Callers building a model from an
    /// unordered source (directory walks, external command output)
    /// call this once before handing the model to `sleak-graph`.
    pub fn canonicalize(&mut self) {
        self.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        self.nodes.dedup_by(|a, b| a.id == b.id);
        self.edges
            .sort_by(|a, b| (&a.from, a.kind, &a.to).cmp(&(&b.from, b.kind, &b.to)));
        self.edges
            .dedup_by(|a, b| a.from == b.from && a.to == b.to && a.kind == b.kind);
    }
}
