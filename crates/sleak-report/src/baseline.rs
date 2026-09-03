//! A persisted baseline (directive §16): the set of findings a project
//! has accepted as existing debt, so `sleak check` can distinguish that
//! from newly-introduced debt without demanding architectural
//! perfection first.

use serde::{Deserialize, Serialize};
use sleak_core::{Finding, Result as SleakResult, SleakError};
use std::path::Path;

pub const BASELINE_SCHEMA_VERSION: &str = "1";
pub const BASELINE_FILE_NAME: &str = ".sleak-baseline.json";

/// A finding's stable identity for baseline/diff comparison — what a
/// finding *is*, not the (recomputed-every-run) evidence for it. Two
/// findings with the same id and target are considered "the same
/// finding" across runs even if their evidence values (a fan-in count,
/// say) shifted slightly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub id: String,
    pub target: String,
}

impl BaselineEntry {
    fn from_finding(finding: &Finding) -> Self {
        Self {
            id: finding.id.as_str().to_string(),
            target: finding.target.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema_version: String,
    pub tool: String,
    /// Sorted, deduplicated.
    pub findings: Vec<BaselineEntry>,
}

impl Baseline {
    /// Builds a baseline from a set of findings (typically a full
    /// `analyse()` result), independent of whatever order they came in.
    pub fn from_findings(findings: &[Finding]) -> Self {
        let mut entries: Vec<BaselineEntry> =
            findings.iter().map(BaselineEntry::from_finding).collect();
        entries.sort();
        entries.dedup();
        Self {
            schema_version: BASELINE_SCHEMA_VERSION.to_string(),
            tool: "sleak".to_string(),
            findings: entries,
        }
    }

    /// `Ok(None)` when no baseline file exists at `path` — a
    /// meaningfully different state from an empty baseline, since it
    /// tells the caller whether to apply baseline logic at all. A
    /// present-but-unparseable file is a hard error, matching
    /// `sleak_policy::Config::load`'s convention.
    pub fn load(path: &Path) -> SleakResult<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                serde_json::from_str(&contents)
                    .map(Some)
                    .map_err(|e| SleakError::ParseFailure {
                        file: path.to_path_buf(),
                        reason: e.to_string(),
                    })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(SleakError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    pub fn save(&self, path: &Path) -> SleakResult<()> {
        let contents =
            serde_json::to_string_pretty(self).expect("Baseline contains only serializable types");
        std::fs::write(path, contents).map_err(|source| SleakError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// Additive, optional report annotation (directive §16) summarising how
/// the current findings compare to a loaded baseline. Present only when
/// `scan`/`check` actually consulted one — `Report.findings` always
/// lists every current finding regardless, so nothing is hidden by
/// having a baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineSummary {
    pub new_count: usize,
    pub existing_count: usize,
    pub resolved: Vec<BaselineEntry>,
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
    fn from_findings_sorts_and_dedups() {
        let findings = vec![
            finding(FindingId::excessive_fan_out(), "b"),
            finding(FindingId::excessive_fan_in(), "a"),
            finding(FindingId::excessive_fan_in(), "a"),
        ];
        let baseline = Baseline::from_findings(&findings);
        assert_eq!(baseline.findings.len(), 2);
        assert_eq!(baseline.findings[0].id, "SLK001");
        assert_eq!(baseline.findings[1].id, "SLK002");
    }

    #[test]
    fn missing_file_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = Baseline::load(&dir.path().join(BASELINE_FILE_NAME)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(BASELINE_FILE_NAME);
        let baseline = Baseline::from_findings(&[finding(FindingId::architectural_cycle(), "x")]);
        baseline.save(&path).unwrap();

        let loaded = Baseline::load(&path).unwrap().unwrap();
        assert_eq!(loaded.findings, baseline.findings);
        assert_eq!(loaded.schema_version, BASELINE_SCHEMA_VERSION);
    }

    #[test]
    fn malformed_file_is_a_hard_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(BASELINE_FILE_NAME);
        std::fs::write(&path, "not json").unwrap();
        assert!(Baseline::load(&path).is_err());
    }
}
