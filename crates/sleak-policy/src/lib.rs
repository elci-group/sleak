//! `.sleak.toml` configuration and threshold evaluation.
//!
//! This crate takes already-computed graph metrics (fan-in/fan-out
//! degrees, cycle membership — plain `sleak-core` types) rather than
//! depending on `sleak-graph` directly, so policy evaluation stays
//! testable against hand-built numbers without needing a real graph.

mod config;
mod evaluate;

pub use config::{Config, CyclePolicy, PolicyConfig, CONFIG_FILE_NAME};
pub use evaluate::evaluate;
