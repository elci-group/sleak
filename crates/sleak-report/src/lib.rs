//! The stable, versioned finding report (directive §13) and its human /
//! JSON renderers.

mod baseline;
mod diff;

pub use baseline::{
    Baseline, BaselineEntry, BaselineSummary, BASELINE_FILE_NAME, BASELINE_SCHEMA_VERSION,
};
pub use diff::{diff_findings, DiffOutcome};

use serde::{Deserialize, Serialize};
use sleak_core::{Finding, Severity};
use std::str::FromStr;

pub const REPORT_SCHEMA_VERSION: &str = "1";

/// Plain counts by severity — the evidence-backed alternative to an
/// invented aggregate score (directive §20's warning against opaque
/// scoring). This is what a consumer like Traci (directive §11, "Sleak
/// findings as architectural-health evidence") reads: real totals, not
/// a synthesized 0-100 number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub critical: usize,
    pub major: usize,
    pub minor: usize,
    pub info: usize,
}

impl Summary {
    fn from_findings(findings: &[Finding]) -> Self {
        let mut summary = Summary::default();
        for finding in findings {
            match finding.severity {
                Severity::Critical => summary.critical += 1,
                Severity::Major => summary.major += 1,
                Severity::Minor => summary.minor += 1,
                Severity::Info => summary.info += 1,
            }
        }
        summary
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: String,
    pub tool: String,
    pub summary: Summary,
    pub findings: Vec<Finding>,
    /// Present only when the run consulted a baseline (directive §16).
    /// `findings` above always lists every current finding regardless —
    /// this is an additive summary, not a filter.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub baseline: Option<BaselineSummary>,
}

impl Report {
    /// `findings` is expected pre-sorted (as `sleak_policy::evaluate`
    /// returns it) — this constructor doesn't re-sort, so ordering
    /// stays owned by whichever crate computed it. `summary` is
    /// computed from `findings` here, always in sync with them.
    pub fn new(findings: Vec<Finding>) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION.to_string(),
            tool: "sleak".to_string(),
            summary: Summary::from_findings(&findings),
            findings,
            baseline: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
}

impl FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(Format::Human),
            "json" => Ok(Format::Json),
            other => Err(format!(
                "unsupported format '{other}' (supported: human, json)"
            )),
        }
    }
}

pub fn render(report: &Report, format: Format) -> String {
    match format {
        Format::Json => {
            serde_json::to_string_pretty(report).expect("Report contains only serializable types")
        }
        Format::Human => render_human(report),
    }
}

fn render_human(report: &Report) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Summary: {} critical, {} major, {} minor, {} info\n\n",
        report.summary.critical, report.summary.major, report.summary.minor, report.summary.info
    ));
    if let Some(summary) = &report.baseline {
        out.push_str(&format!(
            "Baseline: {} new, {} existing (accepted), {} resolved\n\n",
            summary.new_count,
            summary.existing_count,
            summary.resolved.len()
        ));
    }

    if report.findings.is_empty() {
        out.push_str("No findings.\n");
        return out;
    }

    for finding in &report.findings {
        out.push_str(&format!("Finding: {}\n", finding.id));
        out.push_str(&format!("Severity: {}\n", finding.severity));
        out.push_str(&format!("Target: {}\n", finding.target));
        out.push_str(&format!("Reason: {}\n", finding.summary));
        if !finding.evidence.is_empty() {
            out.push_str("Evidence:\n");
            for (key, value) in &finding.evidence {
                out.push_str(&format!("  {key}: {value}\n"));
            }
        }
        if let Some(recommendation) = &finding.recommendation {
            out.push_str(&format!("Recommendation:\n  {recommendation}\n"));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sleak_core::{FindingId, NodeId, Severity};
    use std::collections::BTreeMap;

    fn sample_finding() -> Finding {
        let mut evidence = BTreeMap::new();
        evidence.insert("fan_in".to_string(), 41.into());
        Finding {
            id: FindingId::excessive_fan_in(),
            severity: Severity::Major,
            target: NodeId::new("crate:orchestrator"),
            summary: "too many dependants".to_string(),
            evidence,
            recommendation: Some("split responsibilities".to_string()),
        }
    }

    #[test]
    fn empty_report_renders_no_findings_for_humans() {
        let report = Report::new(vec![]);
        let text = render(&report, Format::Human);
        assert!(text.contains("Summary: 0 critical, 0 major, 0 minor, 0 info"));
        assert!(text.ends_with("No findings.\n"));
    }

    #[test]
    fn summary_counts_findings_by_severity() {
        let report = Report::new(vec![sample_finding()]); // Major
        assert_eq!(report.summary.major, 1);
        assert_eq!(report.summary.critical, 0);
    }

    #[test]
    fn json_round_trips_and_carries_the_schema_version() {
        let report = Report::new(vec![sample_finding()]);
        let json = render(&report, Format::Json);
        let parsed: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, REPORT_SCHEMA_VERSION);
        assert_eq!(parsed.tool, "sleak");
        assert_eq!(parsed.findings.len(), 1);
    }

    #[test]
    fn human_render_includes_evidence_and_recommendation() {
        let report = Report::new(vec![sample_finding()]);
        let text = render(&report, Format::Human);
        assert!(text.contains("SLK001"));
        assert!(text.contains("fan_in: 41"));
        assert!(text.contains("split responsibilities"));
    }

    #[test]
    fn format_from_str_rejects_unknown_values() {
        assert!(Format::from_str("sarif").is_err());
        assert_eq!(Format::from_str("json").unwrap(), Format::Json);
    }
}
