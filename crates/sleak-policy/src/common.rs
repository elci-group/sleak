use sleak_core::{Finding, Severity};
use std::cmp::Reverse;

/// Severity from magnitude over threshold (directive §19: a metric
/// exceeding a threshold is not automatically CRITICAL). Only meaningful
/// once a value is already known to exceed its threshold — a ratio at
/// or below 1.0 has no caller.
pub(crate) fn severity_for_ratio(ratio: f64) -> Severity {
    if ratio > 2.0 {
        Severity::Critical
    } else if ratio > 1.5 {
        Severity::Major
    } else {
        Severity::Minor
    }
}

/// The one ordering every `evaluate_*` function returns findings in:
/// highest severity first, then finding id, then target — shared so
/// every finding source is comparable and a CLI concatenating several
/// of them can re-sort with the same rule and get the same result.
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by_key(|f| (Reverse(f.severity), f.id.clone(), f.target.clone()));
}
