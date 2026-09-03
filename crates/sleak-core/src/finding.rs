use crate::node::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Severity is derived from magnitude, architectural impact and policy
/// configuration (directive §19) — never assigned directly by a
/// detector. Variant order is the comparison order (`Info < ... <
/// Critical`), so `finding.severity >= Severity::Major` reads naturally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Minor,
    Major,
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Severity::Info => "INFO",
            Severity::Minor => "MINOR",
            Severity::Major => "MAJOR",
            Severity::Critical => "CRITICAL",
        };
        f.write_str(s)
    }
}

/// A stable finding-taxonomy identifier (directive §18: "Finding IDs
/// MUST remain stable").
///
/// Only the IDs Phase 1 can actually emit are constructible here.
/// SLK004-SLK009 are reserved by the directive's taxonomy but are added
/// as constructors once the phase that detects them lands, so this type
/// never carries a name nothing can produce.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FindingId(String);

impl FindingId {
    /// SLK001 — excessive fan-in.
    pub fn excessive_fan_in() -> Self {
        Self("SLK001".to_string())
    }

    /// SLK002 — excessive fan-out.
    pub fn excessive_fan_out() -> Self {
        Self("SLK002".to_string())
    }

    /// SLK003 — architectural cycle.
    pub fn architectural_cycle() -> Self {
        Self("SLK003".to_string())
    }

    /// SLK004 — declared architectural boundary violation.
    pub fn boundary_violation() -> Self {
        Self("SLK004".to_string())
    }

    /// SLK006 — excessive centrality.
    pub fn excessive_centrality() -> Self {
        Self("SLK006".to_string())
    }

    /// SLK007 — high blast radius.
    pub fn high_blast_radius() -> Self {
        Self("SLK007".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FindingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One piece of evidence-backed architectural output.
///
/// Every field the directive's evidence chain (§2.3) demands is
/// mandatory except `recommendation`: a finding without evidence is not
/// a finding, but a remediation suggestion is optional (§26 — Sleak is
/// primarily diagnostic).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: FindingId,
    pub severity: Severity,
    pub target: NodeId,
    pub summary: String,
    pub evidence: BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
}
