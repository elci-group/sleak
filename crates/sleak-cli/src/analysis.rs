use camino::Utf8Path;
use sleak_core::{Finding, NodeId, Result as SleakResult};
use sleak_policy::Config;
use std::collections::{BTreeMap, BTreeSet};

/// Runs the full Phase 1-3 pipeline against `path` and returns every
/// finding, already sorted (highest severity first). Shared by `scan`,
/// `check`, and `explain` so all three see identical results for the
/// same workspace.
pub fn analyse(path: &Utf8Path) -> SleakResult<Vec<Finding>> {
    let (model, file_index) = sleak_rust::build_model(path)?;
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

    if config.history.enabled {
        let commits = sleak_git::recent_changes(path, config.history.window);
        if !commits.is_empty() {
            let touches = tally_commit_touches(&commits, &file_index);
            findings.extend(sleak_policy::evaluate_change_concentration(
                &touches,
                commits.len(),
                &config,
            ));
        }
    }

    sleak_policy::sort_findings(&mut findings);
    Ok(findings)
}

/// How many distinct commits touched each node — a commit that changes
/// several files mapping to the same node counts once for that node,
/// since the question is "how many commits landed here", not "how many
/// file-touches".
fn tally_commit_touches(
    commits: &[sleak_git::CommitChange],
    file_index: &sleak_rust::FileIndex,
) -> BTreeMap<NodeId, usize> {
    let mut touches: BTreeMap<NodeId, usize> = BTreeMap::new();
    for commit in commits {
        let touched_nodes: BTreeSet<&NodeId> = commit
            .files
            .iter()
            .filter_map(|file| file_index.get(file))
            .collect();
        for node_id in touched_nodes {
            *touches.entry(node_id.clone()).or_insert(0) += 1;
        }
    }
    touches
}
