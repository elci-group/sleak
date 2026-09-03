use crate::common::{self, severity_for_ratio};
use crate::config::Config;
use sleak_core::{Finding, FindingId, NodeId};
use std::collections::BTreeMap;

fn centrality_finding(id: &NodeId, score: f64, max: f64) -> Finding {
    let ratio = score / max.max(f64::EPSILON);
    let mut evidence = BTreeMap::new();
    evidence.insert("pagerank".to_string(), score.into());
    evidence.insert("max_centrality".to_string(), max.into());
    Finding {
        id: FindingId::excessive_centrality(),
        severity: severity_for_ratio(ratio),
        target: id.clone(),
        summary: format!(
            "{id} holds a disproportionate share of the architecture's influence (PageRank {score:.3} > limit {max:.3})"
        ),
        evidence,
        recommendation: Some(
            "investigate whether this node's influence reflects a real architectural role or accumulated incidental coupling"
                .to_string(),
        ),
    }
}

fn blast_radius_finding(
    id: &NodeId,
    disconnected_count: usize,
    fraction: f64,
    max: f64,
) -> Finding {
    let ratio = fraction / max.max(f64::EPSILON);
    let mut evidence = BTreeMap::new();
    evidence.insert("blast_radius_fraction".to_string(), fraction.into());
    evidence.insert(
        "blast_radius_disconnected_count".to_string(),
        disconnected_count.into(),
    );
    evidence.insert("max_blast_radius".to_string(), max.into());
    Finding {
        id: FindingId::high_blast_radius(),
        severity: severity_for_ratio(ratio),
        target: id.clone(),
        summary: format!(
            "removing {id} would disconnect {disconnected_count} other node(s) ({:.0}% of the remaining architecture, > limit {:.0}%)",
            fraction * 100.0,
            max * 100.0
        ),
        evidence,
        recommendation: Some(
            "investigate introducing a redundant path or interface so this node is no longer a single point of failure"
                .to_string(),
        ),
    }
}

/// Evaluates already-computed PageRank scores against `max_centrality`
/// (SLK006).
pub fn evaluate_centrality(page_rank: &BTreeMap<NodeId, f64>, cfg: &Config) -> Vec<Finding> {
    let mut findings: Vec<Finding> = page_rank
        .iter()
        .filter(|(_, &score)| score > cfg.policy.max_centrality)
        .map(|(id, &score)| centrality_finding(id, score, cfg.policy.max_centrality))
        .collect();
    common::sort_findings(&mut findings);
    findings
}

/// Evaluates already-computed blast-radius metrics — `(disconnected_count,
/// fraction)` per node — against `max_blast_radius` (SLK007).
pub fn evaluate_blast_radius(
    blast_radius: &BTreeMap<NodeId, (usize, f64)>,
    cfg: &Config,
) -> Vec<Finding> {
    let mut findings: Vec<Finding> = blast_radius
        .iter()
        .filter(|(_, &(_, fraction))| fraction > cfg.policy.max_blast_radius)
        .map(|(id, &(count, fraction))| {
            blast_radius_finding(id, count, fraction, cfg.policy.max_blast_radius)
        })
        .collect();
    common::sort_findings(&mut findings);
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use sleak_core::Severity;

    #[test]
    fn centrality_under_threshold_is_silent() {
        let mut ranks = BTreeMap::new();
        ranks.insert(NodeId::new("a"), 0.10);
        let findings = evaluate_centrality(&ranks, &Config::default());
        assert!(findings.is_empty());
    }

    #[test]
    fn centrality_over_threshold_emits_slk006() {
        let mut ranks = BTreeMap::new();
        ranks.insert(NodeId::new("hub"), 0.50); // 2.5x the 0.20 default -> Critical
        let findings = evaluate_centrality(&ranks, &Config::default());

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id.as_str(), "SLK006");
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn blast_radius_over_threshold_emits_slk007() {
        let mut radii = BTreeMap::new();
        radii.insert(NodeId::new("hub"), (8usize, 0.80)); // ~2.67x the 0.30 default
        let findings = evaluate_blast_radius(&radii, &Config::default());

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id.as_str(), "SLK007");
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn zero_blast_radius_is_silent() {
        let mut radii = BTreeMap::new();
        radii.insert(NodeId::new("leaf"), (0usize, 0.0));
        let findings = evaluate_blast_radius(&radii, &Config::default());
        assert!(findings.is_empty());
    }
}
