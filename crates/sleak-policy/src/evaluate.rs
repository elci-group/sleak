use crate::common::{self, severity_for_ratio};
use crate::config::{Config, CyclePolicy};
use sleak_core::{Finding, FindingId, NodeId, Severity};
use std::collections::BTreeMap;

fn fan_in_finding(id: &NodeId, fan_in: usize, max: u32) -> Finding {
    let ratio = fan_in as f64 / max.max(1) as f64;
    let mut evidence = BTreeMap::new();
    evidence.insert("fan_in".to_string(), fan_in.into());
    evidence.insert("max_fan_in".to_string(), max.into());
    Finding {
        id: FindingId::excessive_fan_in(),
        severity: severity_for_ratio(ratio),
        target: id.clone(),
        summary: format!(
            "{id} has {fan_in} direct dependants, exceeding the configured limit of {max}"
        ),
        evidence,
        recommendation: Some(
            "investigate whether dependants can depend on a narrower interface".to_string(),
        ),
    }
}

fn fan_out_finding(id: &NodeId, fan_out: usize, max: u32) -> Finding {
    let ratio = fan_out as f64 / max.max(1) as f64;
    let mut evidence = BTreeMap::new();
    evidence.insert("fan_out".to_string(), fan_out.into());
    evidence.insert("max_fan_out".to_string(), max.into());
    Finding {
        id: FindingId::excessive_fan_out(),
        severity: severity_for_ratio(ratio),
        target: id.clone(),
        summary: format!(
            "{id} depends on {fan_out} other nodes, exceeding the configured limit of {max}"
        ),
        evidence,
        recommendation: Some(
            "investigate splitting responsibilities so no single node needs this many dependencies"
                .to_string(),
        ),
    }
}

fn cycle_finding(cycle: &[NodeId], policy: CyclePolicy) -> Option<Finding> {
    let severity = match policy {
        CyclePolicy::Error => Severity::Critical,
        CyclePolicy::Warn => Severity::Minor,
        CyclePolicy::Ignore => return None,
    };
    let anchor = cycle.first()?.clone();
    let mut evidence = BTreeMap::new();
    evidence.insert(
        "cycle_members".to_string(),
        cycle.iter().map(|n| n.to_string()).collect(),
    );
    evidence.insert("cycle_size".to_string(), cycle.len().into());
    Some(Finding {
        id: FindingId::architectural_cycle(),
        severity,
        target: anchor,
        summary: format!(
            "{} nodes form a mutually-dependent (cyclic) group",
            cycle.len()
        ),
        evidence,
        recommendation: Some(
            "investigate inverting or removing one of the edges to break the cycle".to_string(),
        ),
    })
}

/// Evaluates already-computed graph metrics against policy thresholds
/// and returns findings sorted deterministically by severity (highest
/// first), then finding id, then target.
pub fn evaluate(
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    cycles: &[Vec<NodeId>],
    cfg: &Config,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (id, &(fan_in, fan_out)) in degrees {
        if fan_in as u32 > cfg.policy.max_fan_in {
            findings.push(fan_in_finding(id, fan_in, cfg.policy.max_fan_in));
        }
        if fan_out as u32 > cfg.policy.max_fan_out {
            findings.push(fan_out_finding(id, fan_out, cfg.policy.max_fan_out));
        }
    }

    for cycle in cycles {
        if let Some(finding) = cycle_finding(cycle, cfg.policy.cycles) {
            findings.push(finding);
        }
    }

    common::sort_findings(&mut findings);
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PolicyConfig;

    fn cfg(max_fan_in: u32, max_fan_out: u32, cycles: CyclePolicy) -> Config {
        Config {
            policy: PolicyConfig {
                max_fan_in,
                max_fan_out,
                cycles,
                ..PolicyConfig::default()
            },
            ..Config::default()
        }
    }

    #[test]
    fn no_findings_when_under_every_threshold() {
        let mut degrees = BTreeMap::new();
        degrees.insert(NodeId::new("a"), (2, 2));
        let findings = evaluate(&degrees, &[], &cfg(25, 20, CyclePolicy::Error));
        assert!(findings.is_empty());
    }

    #[test]
    fn fan_in_over_threshold_emits_slk001_with_severity_by_magnitude() {
        let mut degrees = BTreeMap::new();
        degrees.insert(NodeId::new("hub"), (26, 0)); // 1.04x -> Minor
        degrees.insert(NodeId::new("mega"), (60, 0)); // 2.4x -> Critical
        let findings = evaluate(&degrees, &[], &cfg(25, 20, CyclePolicy::Error));

        assert_eq!(findings.len(), 2);
        // Sorted with highest severity first.
        assert_eq!(findings[0].target, NodeId::new("mega"));
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].id.as_str(), "SLK001");
        assert_eq!(findings[1].target, NodeId::new("hub"));
        assert_eq!(findings[1].severity, Severity::Minor);
    }

    #[test]
    fn fan_out_over_threshold_emits_slk002() {
        let mut degrees = BTreeMap::new();
        degrees.insert(NodeId::new("orchestrator"), (0, 45)); // 2.25x -> Critical
        let findings = evaluate(&degrees, &[], &cfg(25, 20, CyclePolicy::Error));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id.as_str(), "SLK002");
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn cycles_respect_policy_error_warn_ignore() {
        let cycle = vec![NodeId::new("a"), NodeId::new("b")];

        let err = evaluate(
            &BTreeMap::new(),
            std::slice::from_ref(&cycle),
            &cfg(25, 20, CyclePolicy::Error),
        );
        assert_eq!(err[0].severity, Severity::Critical);

        let warn = evaluate(
            &BTreeMap::new(),
            std::slice::from_ref(&cycle),
            &cfg(25, 20, CyclePolicy::Warn),
        );
        assert_eq!(warn[0].severity, Severity::Minor);

        let ignored = evaluate(
            &BTreeMap::new(),
            &[cycle],
            &cfg(25, 20, CyclePolicy::Ignore),
        );
        assert!(ignored.is_empty());
    }
}
