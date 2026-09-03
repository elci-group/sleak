use sleak_ir::ArchitectureModel;

/// Renders the raw architecture graph as Graphviz `dot`, bypassing
/// policy findings entirely (directive §27). Node and edge order
/// follows the model's own (already-sorted) order.
pub fn to_dot(model: &ArchitectureModel) -> String {
    let mut out = String::from("digraph sleak {\n");
    for node in &model.nodes {
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\"];\n",
            escape(node.id.as_str()),
            escape(&node.label)
        ));
    }
    for edge in &model.edges {
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{:?}\"];\n",
            escape(edge.from.as_str()),
            escape(edge.to.as_str()),
            edge.kind
        ));
    }
    out.push_str("}\n");
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sleak_core::{EdgeKind, NodeId, NodeKind};
    use sleak_ir::{Edge, Node};

    #[test]
    fn renders_every_node_and_edge() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![
            Node {
                id: NodeId::new("crate:a"),
                kind: NodeKind::Crate,
                label: "a".to_string(),
                path: None,
            },
            Node {
                id: NodeId::new("crate:b"),
                kind: NodeKind::Crate,
                label: "b".to_string(),
                path: None,
            },
        ];
        model.edges = vec![Edge {
            from: NodeId::new("crate:b"),
            to: NodeId::new("crate:a"),
            kind: EdgeKind::DependsOn,
        }];

        let dot = to_dot(&model);
        assert!(dot.starts_with("digraph sleak {\n"));
        assert!(dot.contains("\"crate:a\" [label=\"a\"];"));
        assert!(dot.contains("\"crate:b\" -> \"crate:a\" [label=\"DependsOn\"];"));
        assert!(dot.trim_end().ends_with('}'));
    }
}
