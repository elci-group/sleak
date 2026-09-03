//! Loads an `ArchitectureModel` (sleak-ir) into a real graph structure
//! and answers the structural questions Phase 1 cares about: fan-in,
//! fan-out, and cycles. `petgraph` is an implementation detail of this
//! crate only — it never appears in `sleak-ir`'s or `sleak-core`'s
//! public API.

mod topology;

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use sleak_core::{EdgeKind, NodeId};
use sleak_ir::ArchitectureModel;
use std::collections::BTreeMap;

pub use topology::BlastRadius;

pub struct ArchGraph {
    pub(crate) graph: DiGraph<NodeId, EdgeKind>,
    pub(crate) index: BTreeMap<NodeId, NodeIndex>,
}

impl ArchGraph {
    /// Builds the graph from a model. Nodes are inserted in the model's
    /// (already-sorted) order, so resulting `NodeIndex` assignment is
    /// deterministic. Edges referencing a node absent from the model
    /// are silently dropped rather than panicking — the adapter that
    /// built the model is responsible for only emitting edges between
    /// nodes it also emitted.
    pub fn from_model(model: &ArchitectureModel) -> Self {
        let mut graph = DiGraph::new();
        let mut index = BTreeMap::new();
        for node in &model.nodes {
            let idx = graph.add_node(node.id.clone());
            index.insert(node.id.clone(), idx);
        }
        for edge in &model.edges {
            if let (Some(&from), Some(&to)) = (index.get(&edge.from), index.get(&edge.to)) {
                graph.add_edge(from, to, edge.kind);
            }
        }
        Self { graph, index }
    }

    /// Direct dependants, counting only structural (`DependsOn`,
    /// `Imports`, ...) edges — `Owns` edges (crate → its own modules)
    /// are excluded, since they exist to keep the graph connected for
    /// centrality/blast-radius purposes and don't represent a real
    /// dependant relationship a fan-in policy should react to.
    pub fn fan_in(&self, id: &NodeId) -> usize {
        self.index.get(id).map_or(0, |&idx| {
            self.graph
                .edges_directed(idx, Direction::Incoming)
                .filter(|e| *e.weight() != EdgeKind::Owns)
                .count()
        })
    }

    /// Direct dependencies, with the same `Owns`-exclusion as
    /// [`ArchGraph::fan_in`].
    pub fn fan_out(&self, id: &NodeId) -> usize {
        self.index.get(id).map_or(0, |&idx| {
            self.graph
                .edges_directed(idx, Direction::Outgoing)
                .filter(|e| *e.weight() != EdgeKind::Owns)
                .count()
        })
    }

    /// `(fan_in, fan_out)` for every node, in `NodeId` order.
    pub fn degrees(&self) -> BTreeMap<NodeId, (usize, usize)> {
        self.index
            .keys()
            .map(|id| (id.clone(), (self.fan_in(id), self.fan_out(id))))
            .collect()
    }

    /// Strongly-connected components larger than one node (i.e. import
    /// cycles), each returned as a sorted, deterministic node list.
    /// Computed on the structural (non-`Owns`) subgraph — a crate owning
    /// several modules must never be reported as "cyclic" with them.
    ///
    /// This reports *cycle membership* — every node mutually reachable
    /// within the component — not a minimal simple cycle. Extracting
    /// the shortest simple cycle within a component needs a separate
    /// algorithm (e.g. Johnson's); deferred until evidence shows the
    /// distinction matters to a caller.
    pub fn cycles(&self) -> Vec<Vec<NodeId>> {
        let structural = self.structural_subgraph();
        let sccs = tarjan_scc(&structural);
        let mut cycles: Vec<Vec<NodeId>> = sccs
            .into_iter()
            .filter(|scc| {
                scc.len() > 1
                    || scc
                        .first()
                        .is_some_and(|&idx| structural.contains_edge(idx, idx))
            })
            .map(|scc| {
                let mut ids: Vec<NodeId> =
                    scc.into_iter().map(|idx| structural[idx].clone()).collect();
                ids.sort();
                ids
            })
            .collect();
        cycles.sort();
        cycles
    }

    /// A copy of the graph with `Owns` edges removed — cheap enough to
    /// rebuild per call at the sizes Sleak targets, and it keeps
    /// `self.graph` (used by centrality/blast-radius, which *want*
    /// `Owns` edges for connectivity) as the single source of truth.
    fn structural_subgraph(&self) -> DiGraph<NodeId, EdgeKind> {
        self.graph.filter_map(
            |_, node| Some(node.clone()),
            |_, edge| (*edge != EdgeKind::Owns).then_some(*edge),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sleak_core::NodeKind;
    use sleak_ir::{Edge, Node};

    fn node(id: &str) -> Node {
        Node {
            id: NodeId::new(id),
            kind: NodeKind::Module,
            label: id.to_string(),
            path: None,
        }
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: NodeId::new(from),
            to: NodeId::new(to),
            kind: EdgeKind::Imports,
        }
    }

    #[test]
    fn fan_in_and_fan_out_count_directed_edges() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![node("a"), node("b"), node("c"), node("hub")];
        model.edges = vec![
            edge("a", "hub"),
            edge("b", "hub"),
            edge("c", "hub"),
            edge("hub", "a"),
        ];
        model.canonicalize();
        let graph = ArchGraph::from_model(&model);

        assert_eq!(graph.fan_in(&NodeId::new("hub")), 3);
        assert_eq!(graph.fan_out(&NodeId::new("hub")), 1);
        assert_eq!(graph.fan_in(&NodeId::new("a")), 1);
        assert_eq!(graph.fan_out(&NodeId::new("a")), 1);
    }

    #[test]
    fn degrees_covers_every_node_including_isolated_ones() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![node("isolated"), node("a"), node("b")];
        model.edges = vec![edge("a", "b")];
        model.canonicalize();
        let graph = ArchGraph::from_model(&model);

        let degrees = graph.degrees();
        assert_eq!(degrees.len(), 3);
        assert_eq!(degrees[&NodeId::new("isolated")], (0, 0));
        assert_eq!(degrees[&NodeId::new("a")], (0, 1));
        assert_eq!(degrees[&NodeId::new("b")], (1, 0));
    }

    #[test]
    fn cycles_detects_a_simple_two_node_cycle() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![node("a"), node("b"), node("c")];
        model.edges = vec![edge("a", "b"), edge("b", "a"), edge("b", "c")];
        model.canonicalize();
        let graph = ArchGraph::from_model(&model);

        let cycles = graph.cycles();
        assert_eq!(cycles, vec![vec![NodeId::new("a"), NodeId::new("b")]]);
    }

    #[test]
    fn acyclic_graph_reports_no_cycles() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![node("a"), node("b"), node("c")];
        model.edges = vec![edge("a", "b"), edge("b", "c")];
        model.canonicalize();
        let graph = ArchGraph::from_model(&model);

        assert!(graph.cycles().is_empty());
    }
}
