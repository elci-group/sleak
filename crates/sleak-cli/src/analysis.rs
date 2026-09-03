use camino::{Utf8Path, Utf8PathBuf};
use sleak_core::{Finding, NodeId, Result as SleakResult};
use sleak_policy::Config;
use std::collections::{BTreeMap, BTreeSet};

/// A finding's target `NodeId` -> the workspace-relative source file it
/// maps to (module nodes only — crate nodes have no single file).
/// Consumed by the SARIF renderer; irrelevant to human/JSON output.
pub type Locations = BTreeMap<NodeId, Utf8PathBuf>;

/// Runs the full Phase 1-4 pipeline against `path` and returns every
/// finding (already sorted, highest severity first) plus a location
/// index for SARIF output. Shared by `scan`, `check`, and `explain` so
/// all three see identical results for the same workspace. Loads
/// `.sleak.toml` from `path` itself.
pub fn analyse(path: &Utf8Path) -> SleakResult<(Vec<Finding>, Locations)> {
    let config = Config::load(path.as_std_path())?;
    analyse_with_config(path, &config)
}

/// As [`analyse`], but with an already-loaded config rather than
/// loading `path`'s own `.sleak.toml`. `sleak diff` needs this: it
/// evaluates two different snapshots (the working tree and a ref) and
/// must judge both against the *same* policy, not whatever `.sleak.toml`
/// happened to say at each point in history — otherwise a loosened
/// threshold could masquerade as an architectural improvement.
pub fn analyse_with_config(
    path: &Utf8Path,
    config: &Config,
) -> SleakResult<(Vec<Finding>, Locations)> {
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

    let mut findings = Vec::new();
    findings.extend(sleak_policy::evaluate(&degrees, &cycles, config));
    findings.extend(sleak_policy::evaluate_centrality(&page_rank, config));
    findings.extend(sleak_policy::evaluate_blast_radius(&blast_radius, config));
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
                config,
            ));
        }
    }

    sleak_policy::sort_findings(&mut findings);

    let locations: Locations = file_index
        .into_iter()
        .map(|(path, id)| (id, path))
        .collect();
    Ok((findings, locations))
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
