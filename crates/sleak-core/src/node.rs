use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identity for a node in the architectural graph.
///
/// Wraps the node's canonical path (e.g. `crate:sleak-core` or
/// `module:sleak-core::node`) rather than a hash or an insertion-order
/// index, so identity is human-readable, trivially deterministic across
/// runs, and directly sortable — the property the whole engine leans on
/// for reproducible output (directive §2.2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(canonical_path: impl Into<String>) -> Self {
        Self(canonical_path.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What kind of architectural element a node represents.
///
/// The directive's IR (§3) also names Package/Type/Function/Trait —
/// those are reserved for phases that build the semantic graph needed to
/// populate them; introducing the variants now, unconstructed, would be
/// dead surface area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Crate,
    Module,
}

/// Relationship classes the architectural graph can carry (directive §3).
///
/// Phase 1 only ever constructs `DependsOn` (crate → crate, from Cargo
/// manifests) and `Imports` (module → module, from `use` statements) —
/// the rest of the enum is the stable contract later phases build edges
/// against (`Calls`, `Implements`, ... once a semantic source exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Imports,
    Calls,
    Implements,
    Extends,
    Owns,
    DependsOn,
    Publishes,
    Consumes,
    References,
    Generates,
    Tests,
}
