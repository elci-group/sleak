# Sleak

The anti-monolith architecture engine of ELCI.

> Where Fract prevents monolithic files and Scrawny prevents monolithic
> commits, Sleak prevents monolithic architecture. It does not decide
> that an architecture is bad — it reports where responsibility,
> dependency, and change concentrate, with the evidence to back it, and
> lets a human or a governance policy decide whether that concentration
> is acceptable.

## Status: Phase 2 — Topology

Deterministic, offline, read-only analysis of a Rust Cargo workspace.
No mutation of any kind.

Implemented:
- A typed architectural graph (`sleak-ir`): `Crate` and `Module` nodes,
  `DependsOn`, `Imports`, and `Owns` edges. `Owns` (crate → its own
  modules) exists purely to keep the graph connected across
  granularities for centrality/blast-radius purposes, and is deliberately
  excluded from fan-in/fan-out/cycle detection.
- Crate-dependency graph construction straight from `cargo metadata`
  (`sleak-rust`) — precise, not heuristic.
- Module-level import graph from `syn`-based parsing of `mod`/`use`
  items (`sleak-rust`) — a best-effort heuristic: only `crate`/`self`/
  `super`-relative and other-workspace-crate-relative imports are
  resolved (correctly matching a Cargo package's hyphenated name, e.g.
  `sleak-core`, against its underscored Rust identifier, `sleak_core`,
  in `use` paths); macros, glob re-exports, `#[path = "..."]`
  attributes, and `use` statements inside inline `mod { ... }` blocks
  are not. Scope statement lives in `sleak-rust/src/lib.rs`.
- Graph metrics (`sleak-graph`, backed by `petgraph`):
  - fan-in / fan-out (structural edges only)
  - cycle detection via strongly-connected components — reports
    *component membership*, not a minimal simple cycle
  - PageRank-style centrality
  - articulation points and blast radius (what fraction of the rest of
    the architecture would be cut off from the largest remaining piece
    if this node were removed), computed on an undirected view of the
    full graph including `Owns` edges
- Declared architectural boundaries (`sleak-policy`): configurable
  named layers (path-prefix based) and forbidden layer-crossing rules —
  no paradigm (layered, hexagonal, ...) is hard-coded.
- Policy evaluation (`sleak-policy`) against a `.sleak.toml` config,
  emitting:
  - **SLK001** excessive fan-in
  - **SLK002** excessive fan-out
  - **SLK003** architectural cycle (module-level in practice — Cargo
    itself forbids cyclic crate dependencies, so SLK003 essentially
    never fires at crate granularity; the algorithm is still built
    generically over the graph so it's ready for edge kinds added later,
    e.g. `calls`, that genuinely can cycle)
  - **SLK004** declared boundary violation
  - **SLK006** excessive centrality
  - **SLK007** high blast radius
- Human and versioned-JSON output (`sleak-report`,
  `{"schema_version":"1","tool":"sleak","findings":[...]}`), each
  finding carrying severity, target, evidence, and (where meaningful) a
  remediation suggestion.
- `sleak scan`, `sleak check` (stable exit-code contract, §14: `0`
  clean, `1` blocking findings on `check`, `2` configuration error, `3`
  analysis failure, `4` infrastructure/tooling failure), `sleak explain
  <ID>` (taxonomy description + every current finding with that id), and
  `sleak graph` (raw node/edge export as `dot` or `json`, bypassing
  policy entirely).

Not yet implemented (later phases per the project directive):
- **SLK005** (god component) and **SLK008** (responsibility clustering)
  are reserved but not built — the directive marks the semantic signals
  they need (naming, documentation terminology, ...) as advisory, and
  none of that exists yet. **SLK010** (dependency bottleneck) is also
  deferred: every crisp definition tried either duplicated SLK001/002/
  006/007 or needed a semantic layer this build doesn't have.
- **Phase 3 — Temporal**: Git-history ingestion (`sleak-git`),
  change-concentration detection (SLK009), correlation with Scrawny.
- **Phase 4 — Governance**: baselines (`sleak baseline create`),
  differential analysis (`sleak diff`), an Architecture Health score.
- **Phase 5 — Integration**: SARIF output (`sleak-sarif`), Traci/
  Kaptaind machine-interface polish (`sleak-integration`), advisory
  responsibility clustering, remediation automation.

Type/call/trait-graph analysis (real semantic resolution — `Calls`,
`Implements`, generic instantiation) needs a real Rust semantic source
(`rustc` or `rust-analyzer`'s resolver, or a future `Furnace`
integration per the directive's §11) and isn't attempted here.

## Usage

```bash
sleak                          # scan the current directory (human output)
sleak scan .                    # same, explicit
sleak scan --format json .
sleak check .                    # scan + fail (exit 1) on any MAJOR/CRITICAL finding — the CI gate
sleak explain SLK007 .            # what SLK007 means, plus any current instances
sleak graph .                      # architecture graph as Graphviz dot
sleak graph . --output json         # ...or as raw JSON (independent of --format)
```

## Building

```bash
cargo build --workspace
cargo test --workspace
```

## Configuration

`.sleak.toml` at the workspace root (all keys optional; shown values are
the defaults):

```toml
[policy]
max_fan_in = 25
max_fan_out = 20
cycles = "error"        # error | warn | ignore
max_centrality = 0.20    # raw PageRank score (0.0-1.0)
max_blast_radius = 0.30   # fraction of the rest of the architecture (0.0-1.0)

[boundaries]
enabled = false

[boundaries.layers]
# layer name = crate-relative path prefix
domain = "src/domain"
infrastructure = "src/infrastructure"

[[boundaries.forbidden]]
from = "domain"
to = "infrastructure"
```

A missing file uses the defaults above. A present-but-invalid file is a
hard configuration error (exit code 2) — Sleak never silently falls back
to defaults on a malformed config, only on an absent one.

Boundaries are disabled by default and inert with no `forbidden` rules —
Sleak never assumes a paradigm; the project declares its own layers and
which crossings aren't allowed. A same-layer edge is never a violation,
and an edge touching a node with no matching layer is skipped rather
than guessed at.
