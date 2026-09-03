use crate::ArchGraph;
use petgraph::visit::EdgeRef;
use sleak_core::NodeId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Fixed, non-random PageRank parameters (directive §2.2: deterministic
/// analysis) — the same graph always yields the same ranks.
const PAGE_RANK_DAMPING: f64 = 0.85;
const PAGE_RANK_ITERATIONS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlastRadius {
    /// How many other nodes end up outside the largest remaining
    /// connected component if this node is removed.
    pub disconnected_count: usize,
    /// `disconnected_count` as a fraction of every other node.
    pub fraction: f64,
}

impl ArchGraph {
    /// PageRank-style influence per node (directive §4.2). Node
    /// insertion order into `self.graph` matches `self.index`'s
    /// (`NodeId`-sorted) iteration order — both come from the same
    /// already-sorted `ArchitectureModel::nodes` — so zipping the two
    /// is a valid, index-free way to recover which score belongs to
    /// which node.
    pub fn page_rank(&self) -> BTreeMap<NodeId, f64> {
        let ranks = petgraph::algo::page_rank(&self.graph, PAGE_RANK_DAMPING, PAGE_RANK_ITERATIONS);
        self.index.keys().cloned().zip(ranks).collect()
    }

    /// Nodes whose removal would increase the number of connected
    /// components (directive §7). Computed on an undirected view of the
    /// dependency graph — architectural disconnection is a structural,
    /// not directional, question. Edge weights and self-loops carry no
    /// meaning here and are dropped.
    pub fn articulation_points(&self) -> BTreeSet<NodeId> {
        let mut undirected = petgraph::graph::UnGraph::<NodeId, ()>::new_undirected();
        let mut index = BTreeMap::new();
        for id in self.index.keys() {
            index.insert(id.clone(), undirected.add_node(id.clone()));
        }
        for edge in self.graph.edge_references() {
            let from = &self.graph[edge.source()];
            let to = &self.graph[edge.target()];
            if from != to {
                undirected.add_edge(index[from], index[to], ());
            }
        }
        petgraph::algo::articulation_points::articulation_points(&undirected)
            .into_iter()
            .map(|idx| undirected[idx].clone())
            .collect()
    }

    /// Blast radius per node (directive §7): "what breaks if we remove
    /// this?", measured as how much of the rest of the architecture
    /// stops being reachable from the largest surviving piece. Only
    /// articulation points can have a non-zero blast radius — removing
    /// any other node cannot disconnect anything by definition.
    pub fn blast_radius(&self) -> BTreeMap<NodeId, BlastRadius> {
        let adjacency = self.undirected_adjacency();
        let all_nodes: Vec<NodeId> = self.index.keys().cloned().collect();
        let cut_vertices = self.articulation_points();

        all_nodes
            .iter()
            .map(|id| {
                if !cut_vertices.contains(id) {
                    return (
                        id.clone(),
                        BlastRadius {
                            disconnected_count: 0,
                            fraction: 0.0,
                        },
                    );
                }
                let remaining: Vec<&NodeId> = all_nodes.iter().filter(|n| *n != id).collect();
                let total = remaining.len();
                let largest = connected_component_sizes(&adjacency, id, &remaining)
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                let disconnected = total.saturating_sub(largest);
                let fraction = if total == 0 {
                    0.0
                } else {
                    disconnected as f64 / total as f64
                };
                (
                    id.clone(),
                    BlastRadius {
                        disconnected_count: disconnected,
                        fraction,
                    },
                )
            })
            .collect()
    }

    fn undirected_adjacency(&self) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
        let mut adjacency: BTreeMap<NodeId, BTreeSet<NodeId>> = self
            .index
            .keys()
            .map(|id| (id.clone(), BTreeSet::new()))
            .collect();
        for edge in self.graph.edge_references() {
            let from = self.graph[edge.source()].clone();
            let to = self.graph[edge.target()].clone();
            if from != to {
                adjacency
                    .entry(from.clone())
                    .or_default()
                    .insert(to.clone());
                adjacency.entry(to).or_default().insert(from);
            }
        }
        adjacency
    }
}

/// Sizes of every connected component reachable within `nodes`,
/// treating `exclude` as removed from the graph entirely. Iteration
/// over `nodes` (expected pre-sorted) and each node's `BTreeSet`
/// neighbours keeps this deterministic.
fn connected_component_sizes(
    adjacency: &BTreeMap<NodeId, BTreeSet<NodeId>>,
    exclude: &NodeId,
    nodes: &[&NodeId],
) -> Vec<usize> {
    let mut visited: BTreeSet<NodeId> = BTreeSet::new();
    let mut sizes = Vec::new();

    for &start in nodes {
        if visited.contains(start) {
            continue;
        }
        let mut queue = VecDeque::from([start.clone()]);
        visited.insert(start.clone());
        let mut size = 0usize;
        while let Some(current) = queue.pop_front() {
            size += 1;
            if let Some(neighbors) = adjacency.get(&current) {
                for neighbor in neighbors {
                    if neighbor == exclude || visited.contains(neighbor) {
                        continue;
                    }
                    visited.insert(neighbor.clone());
                    queue.push_back(neighbor.clone());
                }
            }
        }
        sizes.push(size);
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchGraph;
    use sleak_core::{EdgeKind, NodeKind};
    use sleak_ir::{ArchitectureModel, Edge, Node};

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

    /// hub -> a, hub -> b, hub -> c: a "bridge" chain a-hub-b, c dangling
    /// off hub too. Removing hub disconnects a, b, and c from each
    /// other.
    fn bridge_model() -> ArchitectureModel {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![node("hub"), node("a"), node("b"), node("c")];
        model.edges = vec![edge("hub", "a"), edge("hub", "b"), edge("hub", "c")];
        model.canonicalize();
        model
    }

    #[test]
    fn page_rank_sums_to_one_and_favours_the_most_depended_on_node() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![node("hub"), node("a"), node("b"), node("c")];
        model.edges = vec![edge("a", "hub"), edge("b", "hub"), edge("c", "hub")];
        model.canonicalize();
        let graph = ArchGraph::from_model(&model);

        let ranks = graph.page_rank();
        let total: f64 = ranks.values().sum();
        assert!((total - 1.0).abs() < 1e-6);
        assert!(ranks[&NodeId::new("hub")] > ranks[&NodeId::new("a")]);
    }

    #[test]
    fn hub_of_a_star_is_the_only_articulation_point() {
        let graph = ArchGraph::from_model(&bridge_model());
        let points = graph.articulation_points();
        assert_eq!(points, [NodeId::new("hub")].into_iter().collect());
    }

    #[test]
    fn removing_a_leaf_never_has_blast_radius() {
        let graph = ArchGraph::from_model(&bridge_model());
        let radii = graph.blast_radius();
        assert_eq!(radii[&NodeId::new("a")].disconnected_count, 0);
        assert_eq!(radii[&NodeId::new("a")].fraction, 0.0);
    }

    #[test]
    fn removing_the_hub_disconnects_the_other_leaves() {
        let graph = ArchGraph::from_model(&bridge_model());
        let radii = graph.blast_radius();
        let hub = &radii[&NodeId::new("hub")];
        // 3 other nodes remain; the largest surviving component after
        // removing hub is a single isolated leaf (size 1), so 2 of the
        // 3 remaining nodes are cut off.
        assert_eq!(hub.disconnected_count, 2);
        assert!((hub.fraction - (2.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn fully_connected_triangle_has_no_articulation_points() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![node("a"), node("b"), node("c")];
        model.edges = vec![edge("a", "b"), edge("b", "c"), edge("c", "a")];
        model.canonicalize();
        let graph = ArchGraph::from_model(&model);

        assert!(graph.articulation_points().is_empty());
        assert!(graph.blast_radius().values().all(|r| r.fraction == 0.0));
    }
}
