use camino::Utf8Path;
use sleak_core::{Finding, Result as SleakResult};
use sleak_policy::Config;

/// Runs the full Phase 1 + Phase 2 pipeline against `path` and returns
/// every finding, already sorted (highest severity first). Shared by
/// `scan`, `check`, and `explain` so all three see identical results
/// for the same workspace.
pub fn analyse(path: &Utf8Path) -> SleakResult<Vec<Finding>> {
    let model = sleak_rust::build_model(path)?;
    let graph = sleak_graph::ArchGraph::from_model(&model);

    let degrees = graph.degrees();
    let cycles = graph.cycles();
    let page_rank = graph.page_rank();
    let blast_radius = graph
        .blast_radius()
        .into_iter()
        .map(|(id, radius)| (id, (radius.disconnected_count, radius.fraction)))
        .collect();

    let config = Config::load(path.as_std_path())?;

    let mut findings = Vec::new();
    findings.extend(sleak_policy::evaluate(&degrees, &cycles, &config));
    findings.extend(sleak_policy::evaluate_centrality(&page_rank, &config));
    findings.extend(sleak_policy::evaluate_blast_radius(&blast_radius, &config));
    findings.extend(sleak_policy::evaluate_boundaries(
        &model,
        &config.boundaries,
    ));
    sleak_policy::sort_findings(&mut findings);

    Ok(findings)
}
