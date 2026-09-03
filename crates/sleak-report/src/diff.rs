//! Differential analysis (directive §17): comparing the current set of
//! findings against a reference set (a saved baseline, or a fresh
//! analysis of another git ref — both are just a [`Baseline`] as far as
//! this module is concerned).

use crate::baseline::{Baseline, BaselineEntry};
use sleak_core::Finding;
use std::collections::BTreeSet;

/// The three ways a finding can relate to a reference set: newly
/// introduced, still present from before, or no longer present.
/// `resolved` only carries the identity (id + target), not full
/// evidence — a finding that's gone has nothing left to show evidence
/// for.
#[derive(Debug, Clone, Default)]
pub struct DiffOutcome {
    pub new: Vec<Finding>,
    pub existing: Vec<Finding>,
    pub resolved: Vec<BaselineEntry>,
}

/// Classifies `current` against `reference` by `(id, target)` identity.
pub fn diff_findings(current: &[Finding], reference: &Baseline) -> DiffOutcome {
    let reference_set: BTreeSet<BaselineEntry> = reference.findings.iter().cloned().collect();
    let mut outcome = DiffOutcome::default();
    let mut seen: BTreeSet<BaselineEntry> = BTreeSet::new();

    for finding in current {
        let key = BaselineEntry {
            id: finding.id.as_str().to_string(),
            target: finding.target.to_string(),
        };
        seen.insert(key.clone());
        if reference_set.contains(&key) {
            outcome.existing.push(finding.clone());
        } else {
            outcome.new.push(finding.clone());
        }
    }

    outcome.resolved = reference
        .findings
        .iter()
        .filter(|entry| !seen.contains(*entry))
        .cloned()
        .collect();

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use sleak_core::{FindingId, NodeId, Severity};
    use std::collections::BTreeMap;

    fn finding(id: FindingId, target: &str) -> Finding {
        Finding {
            id,
            severity: Severity::Major,
            target: NodeId::new(target),
            summary: "summary".to_string(),
            evidence: BTreeMap::new(),
            recommendation: None,
        }
    }

    #[test]
    fn classifies_new_existing_and_resolved() {
        let reference = Baseline::from_findings(&[
            finding(FindingId::excessive_fan_in(), "a"), // will persist
            finding(FindingId::excessive_fan_out(), "b"), // will be resolved
        ]);
        let current = vec![
            finding(FindingId::excessive_fan_in(), "a"), // existing
            finding(FindingId::architectural_cycle(), "c"), // new
        ];

        let outcome = diff_findings(&current, &reference);

        assert_eq!(outcome.new.len(), 1);
        assert_eq!(outcome.new[0].id.as_str(), "SLK003");
        assert_eq!(outcome.existing.len(), 1);
        assert_eq!(outcome.existing[0].id.as_str(), "SLK001");
        assert_eq!(outcome.resolved.len(), 1);
        assert_eq!(outcome.resolved[0].id, "SLK002");
    }

    #[test]
    fn empty_reference_marks_everything_new() {
        let reference = Baseline::from_findings(&[]);
        let current = vec![finding(FindingId::excessive_fan_in(), "a")];

        let outcome = diff_findings(&current, &reference);
        assert_eq!(outcome.new.len(), 1);
        assert!(outcome.existing.is_empty());
        assert!(outcome.resolved.is_empty());
    }
}
