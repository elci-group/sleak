//! SARIF 2.1.0 output (directive §13/§27) — lets `sleak` findings feed
//! GitHub code scanning and any other SARIF-consuming CI system.
//!
//! Rendering is a pure function of already-computed findings plus a
//! location index; it never re-runs analysis and never fabricates a
//! location it doesn't actually know — a finding whose target isn't in
//! `locations` is emitted with no `locations` array at all (SARIF
//! allows this) rather than pointed at a plausible-looking but
//! incorrect file.

use camino::Utf8PathBuf;
use serde_json::{json, Value};
use sleak_core::{Finding, NodeId, Severity};
use std::collections::BTreeMap;

const SARIF_SCHEMA: &str =
    "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json";
const INFORMATION_URI: &str = "https://github.com/elci-group/sleak";

/// SARIF `level` for a Sleak severity. SARIF has no level between
/// `warning` and `error`, so `Major` and `Critical` both map to
/// `error` — the actual Sleak severity is never lost, it's carried in
/// each result's `properties.severity` too.
fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "note",
        Severity::Minor => "warning",
        Severity::Major | Severity::Critical => "error",
    }
}

fn rule(id: &str) -> Value {
    let description = sleak_core::describe(id).unwrap_or("(no description available)");
    json!({
        "id": id,
        "shortDescription": { "text": description.split(" — ").next().unwrap_or(description) },
        "fullDescription": { "text": description },
    })
}

fn result(finding: &Finding, locations: &BTreeMap<NodeId, Utf8PathBuf>) -> Value {
    let mut result = json!({
        "ruleId": finding.id.as_str(),
        "level": sarif_level(finding.severity),
        "message": { "text": finding.summary },
        "properties": {
            "severity": finding.severity.to_string().to_lowercase(),
            "target": finding.target.to_string(),
            "evidence": finding.evidence,
        },
    });

    if let Some(path) = locations.get(&finding.target) {
        result["locations"] = json!([{
            "physicalLocation": {
                "artifactLocation": { "uri": path.as_str() },
                // Sleak's findings are node-level (a whole module/crate),
                // not tied to a specific line — startLine 1 anchors the
                // result to the file without claiming precision it
                // doesn't have.
                "region": { "startLine": 1 },
            },
        }]);
    }

    result
}

/// Renders `findings` as a SARIF 2.1.0 log (pretty-printed). `locations`
/// maps a finding's target `NodeId` to a workspace-relative file path
/// (e.g. `sleak_rust::FileIndex`, inverted) — typically only module
/// nodes have one; crate-level findings are emitted without a
/// `locations` array rather than guessed at.
pub fn render_sarif(findings: &[Finding], locations: &BTreeMap<NodeId, Utf8PathBuf>) -> String {
    let mut rule_ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
    rule_ids.sort();
    rule_ids.dedup();

    let log = json!({
        "$schema": SARIF_SCHEMA,
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "sleak",
                    "informationUri": INFORMATION_URI,
                    "version": env!("CARGO_PKG_VERSION"),
                    "rules": rule_ids.iter().map(|id| rule(id)).collect::<Vec<_>>(),
                },
            },
            "results": findings.iter().map(|f| result(f, locations)).collect::<Vec<_>>(),
        }],
    });

    serde_json::to_string_pretty(&log).expect("SARIF payload is always serializable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sleak_core::FindingId;

    fn finding(id: FindingId, target: &str, severity: Severity) -> Finding {
        Finding {
            id,
            severity,
            target: NodeId::new(target),
            summary: "summary text".to_string(),
            evidence: BTreeMap::new(),
            recommendation: None,
        }
    }

    #[test]
    fn produces_valid_json_with_the_sarif_schema_and_version() {
        let sarif = render_sarif(&[], &BTreeMap::new());
        let parsed: Value = serde_json::from_str(&sarif).unwrap();
        assert_eq!(parsed["version"], "2.1.0");
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["name"], "sleak");
    }

    #[test]
    fn known_target_gets_a_physical_location() {
        let mut locations = BTreeMap::new();
        locations.insert(
            NodeId::new("module:a"),
            Utf8PathBuf::from("crates/a/src/lib.rs"),
        );
        let findings = vec![finding(
            FindingId::excessive_fan_in(),
            "module:a",
            Severity::Major,
        )];

        let sarif = render_sarif(&findings, &locations);
        let parsed: Value = serde_json::from_str(&sarif).unwrap();
        let result = &parsed["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "SLK001");
        assert_eq!(result["level"], "error");
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "crates/a/src/lib.rs"
        );
    }

    #[test]
    fn unknown_target_has_no_fabricated_location() {
        let findings = vec![finding(
            FindingId::excessive_fan_out(),
            "crate:b",
            Severity::Minor,
        )];
        let sarif = render_sarif(&findings, &BTreeMap::new());
        let parsed: Value = serde_json::from_str(&sarif).unwrap();
        let result = &parsed["runs"][0]["results"][0];
        assert_eq!(result["level"], "warning");
        assert!(result.get("locations").is_none());
    }

    #[test]
    fn rules_are_deduplicated_and_sorted() {
        let findings = vec![
            finding(FindingId::excessive_fan_out(), "a", Severity::Minor),
            finding(FindingId::excessive_fan_in(), "b", Severity::Minor),
            finding(FindingId::excessive_fan_in(), "c", Severity::Minor),
        ];
        let sarif = render_sarif(&findings, &BTreeMap::new());
        let parsed: Value = serde_json::from_str(&sarif).unwrap();
        let rules = parsed["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["id"], "SLK001");
        assert_eq!(rules[1]["id"], "SLK002");
    }
}
