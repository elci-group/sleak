use crate::common::{self, severity_for_ratio};
use crate::config::Config;
use sleak_core::{Finding, FindingId, NodeId};
use std::collections::BTreeMap;

fn change_concentration_finding(
    id: &NodeId,
    commit_count: usize,
    frequency: f64,
    max: f64,
) -> Finding {
    let ratio = frequency / max.max(f64::EPSILON);
    let mut evidence = BTreeMap::new();
    evidence.insert("commits_touching_node".to_string(), commit_count.into());
    evidence.insert("change_frequency".to_string(), frequency.into());
    evidence.insert("max_change_frequency".to_string(), max.into());
    Finding {
        id: FindingId::architectural_change_concentration(),
        severity: severity_for_ratio(ratio),
        target: id.clone(),
        summary: format!(
            "{id} was touched by {commit_count} of the analysed commits ({:.0}% > limit {:.0}%) — this location repeatedly attracts change",
            frequency * 100.0,
            max * 100.0
        ),
        evidence,
        recommendation: Some(
            "investigate whether this node mixes concerns that change for different reasons and could be split"
                .to_string(),
        ),
    }
}

/// Evaluates already-tallied per-node commit-touch counts (how many of
/// the analysed commits touched each node, each commit counted at most
/// once per node) against `max_change_frequency` (SLK009, directive
/// §9). A no-op when no commits were analysed — there's no frequency to
/// compute, and reporting nothing is more honest than reporting 0%
/// everywhere.
pub fn evaluate_change_concentration(
    commit_touches: &BTreeMap<NodeId, usize>,
    commits_analyzed: usize,
    cfg: &Config,
) -> Vec<Finding> {
    if commits_analyzed == 0 {
        return Vec::new();
    }
    let mut findings: Vec<Finding> = commit_touches
        .iter()
        .filter_map(|(id, &count)| {
            let frequency = count as f64 / commits_analyzed as f64;
            (frequency > cfg.policy.max_change_frequency).then(|| {
                change_concentration_finding(id, count, frequency, cfg.policy.max_change_frequency)
            })
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
    fn no_commits_analyzed_is_silent() {
        let mut touches = BTreeMap::new();
        touches.insert(NodeId::new("hot"), 10);
        assert!(evaluate_change_concentration(&touches, 0, &Config::default()).is_empty());
    }

    #[test]
    fn under_threshold_is_silent() {
        let mut touches = BTreeMap::new();
        touches.insert(NodeId::new("a"), 1); // 1/100 = 1%
        assert!(evaluate_change_concentration(&touches, 100, &Config::default()).is_empty());
    }

    #[test]
    fn over_threshold_emits_slk009() {
        let mut touches = BTreeMap::new();
        touches.insert(NodeId::new("hot"), 40); // 40/100 = 40%, default limit 15% -> 2.67x -> Critical
        let findings = evaluate_change_concentration(&touches, 100, &Config::default());

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id.as_str(), "SLK009");
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].target, NodeId::new("hot"));
    }
}
