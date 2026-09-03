use crate::common;
use crate::config::BoundariesConfig;
use camino::Utf8Path;
use sleak_core::{Finding, FindingId, NodeId, Severity};
use sleak_ir::ArchitectureModel;
use std::collections::{BTreeMap, BTreeSet};

/// The layer a node's path falls under, chosen as the longest matching
/// configured prefix — so a more specific layer (`src/domain/billing`)
/// wins over a broader one (`src/domain`) if both are declared.
fn classify<'a>(path: &Utf8Path, layers: &'a BTreeMap<String, String>) -> Option<&'a str> {
    layers
        .iter()
        .filter(|(_, prefix)| path.as_str().starts_with(prefix.as_str()))
        .max_by_key(|(_, prefix)| prefix.len())
        .map(|(name, _)| name.as_str())
}

fn boundary_finding(from: &NodeId, to: &NodeId, from_layer: &str, to_layer: &str) -> Finding {
    let mut evidence = BTreeMap::new();
    evidence.insert("from_layer".to_string(), from_layer.into());
    evidence.insert("to_layer".to_string(), to_layer.into());
    evidence.insert("depends_on".to_string(), to.to_string().into());
    Finding {
        id: FindingId::boundary_violation(),
        // A declared boundary is either crossed or not — there's no
        // notion of "how much", so unlike the magnitude-driven findings
        // this is a fixed severity (directive §19 still applies: not
        // automatically CRITICAL, but a real policy violation).
        severity: Severity::Major,
        target: from.clone(),
        summary: format!(
            "{from} ({from_layer}) depends on {to} ({to_layer}), a disallowed crossing"
        ),
        evidence,
        recommendation: Some(
            "investigate inverting the dependency or introducing an interface at the boundary"
                .to_string(),
        ),
    }
}

/// Evaluates the model's edges against declared boundaries (directive
/// §4.3, SLK004). A no-op when boundaries are disabled or no forbidden
/// crossings are declared. Only edges whose *both* endpoints resolve to
/// a declared layer are checked — an edge touching an unclassified node
/// is not evidence of anything, so it's skipped rather than guessed at.
pub fn evaluate_boundaries(model: &ArchitectureModel, cfg: &BoundariesConfig) -> Vec<Finding> {
    if !cfg.enabled || cfg.forbidden.is_empty() {
        return Vec::new();
    }
    let forbidden: BTreeSet<(&str, &str)> = cfg
        .forbidden
        .iter()
        .map(|r| (r.from.as_str(), r.to.as_str()))
        .collect();

    let node_paths: BTreeMap<&NodeId, &Utf8Path> = model
        .nodes
        .iter()
        .filter_map(|n| n.path.as_deref().map(|p| (&n.id, p)))
        .collect();

    let mut findings = Vec::new();
    for edge in &model.edges {
        let (Some(&from_path), Some(&to_path)) =
            (node_paths.get(&edge.from), node_paths.get(&edge.to))
        else {
            continue;
        };
        let (Some(from_layer), Some(to_layer)) = (
            classify(from_path, &cfg.layers),
            classify(to_path, &cfg.layers),
        ) else {
            continue;
        };
        if from_layer != to_layer && forbidden.contains(&(from_layer, to_layer)) {
            findings.push(boundary_finding(&edge.from, &edge.to, from_layer, to_layer));
        }
    }

    common::sort_findings(&mut findings);
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ForbiddenRule;
    use sleak_core::{EdgeKind, NodeKind};
    use sleak_ir::{Edge, Node};

    fn module(id: &str, path: &str) -> Node {
        Node {
            id: NodeId::new(id),
            kind: NodeKind::Module,
            label: id.to_string(),
            path: Some(Utf8Path::new(path).to_path_buf()),
        }
    }

    fn cfg(layers: &[(&str, &str)], forbidden: &[(&str, &str)]) -> BoundariesConfig {
        BoundariesConfig {
            enabled: true,
            layers: layers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            forbidden: forbidden
                .iter()
                .map(|(from, to)| ForbiddenRule {
                    from: from.to_string(),
                    to: to.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn disallowed_crossing_emits_slk004() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![
            module("domain::order", "src/domain/order.rs"),
            module("infra::db", "src/infrastructure/db.rs"),
        ];
        model.edges = vec![Edge {
            from: NodeId::new("domain::order"),
            to: NodeId::new("infra::db"),
            kind: EdgeKind::Imports,
        }];
        model.canonicalize();

        let policy = cfg(
            &[
                ("domain", "src/domain"),
                ("infrastructure", "src/infrastructure"),
            ],
            &[("domain", "infrastructure")],
        );
        let findings = evaluate_boundaries(&model, &policy);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id.as_str(), "SLK004");
        assert_eq!(findings[0].target, NodeId::new("domain::order"));
    }

    #[test]
    fn allowed_crossing_is_silent() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![
            module("application::svc", "src/application/svc.rs"),
            module("domain::order", "src/domain/order.rs"),
        ];
        model.edges = vec![Edge {
            from: NodeId::new("application::svc"),
            to: NodeId::new("domain::order"),
            kind: EdgeKind::Imports,
        }];
        model.canonicalize();

        // "application -> domain" was never declared forbidden.
        let policy = cfg(
            &[("domain", "src/domain"), ("application", "src/application")],
            &[("domain", "application")],
        );
        assert!(evaluate_boundaries(&model, &policy).is_empty());
    }

    #[test]
    fn disabled_boundaries_are_always_silent() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![
            module("domain::order", "src/domain/order.rs"),
            module("infra::db", "src/infrastructure/db.rs"),
        ];
        model.edges = vec![Edge {
            from: NodeId::new("domain::order"),
            to: NodeId::new("infra::db"),
            kind: EdgeKind::Imports,
        }];
        model.canonicalize();

        let mut policy = cfg(
            &[
                ("domain", "src/domain"),
                ("infrastructure", "src/infrastructure"),
            ],
            &[("domain", "infrastructure")],
        );
        policy.enabled = false;
        assert!(evaluate_boundaries(&model, &policy).is_empty());
    }

    #[test]
    fn unclassified_node_is_skipped_not_guessed() {
        let mut model = ArchitectureModel::new();
        model.nodes = vec![
            module("domain::order", "src/domain/order.rs"),
            module("misc::helper", "src/misc/helper.rs"),
        ];
        model.edges = vec![Edge {
            from: NodeId::new("domain::order"),
            to: NodeId::new("misc::helper"),
            kind: EdgeKind::Imports,
        }];
        model.canonicalize();

        let policy = cfg(&[("domain", "src/domain")], &[("domain", "misc")]);
        assert!(evaluate_boundaries(&model, &policy).is_empty());
    }
}
