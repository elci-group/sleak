//! `.sleak.toml` configuration and threshold evaluation.
//!
//! This crate takes already-computed graph metrics (fan-in/fan-out
//! degrees, cycle membership — plain `sleak-core` types) rather than
//! depending on `sleak-graph` directly, so policy evaluation stays
//! testable against hand-built numbers without needing a real graph.

mod boundaries;
mod common;
mod config;
mod evaluate;
mod topology;

pub use boundaries::evaluate_boundaries;
pub use common::sort_findings;
pub use config::{
    BoundariesConfig, Config, CyclePolicy, ForbiddenRule, PolicyConfig, CONFIG_FILE_NAME,
};
pub use evaluate::evaluate;
pub use topology::{evaluate_blast_radius, evaluate_centrality};
