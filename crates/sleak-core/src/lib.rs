//! Shared primitives for Sleak: stable node identity, relationship kinds,
//! the finding taxonomy, and the crate's error type.
//!
//! `sleak-core` has no dependency on any other Sleak crate and no
//! dependency on a graph library — it defines only the vocabulary that
//! every other layer (IR, graph algorithms, policy, report) shares.

mod error;
mod finding;
mod node;

pub use error::SleakError;
pub use finding::{describe, Finding, FindingId, Severity};
pub use node::{EdgeKind, NodeId, NodeKind};

pub type Result<T> = std::result::Result<T, SleakError>;
