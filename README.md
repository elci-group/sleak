# Sleak

The anti-monolith architecture engine of ELCI.

> Where Fract prevents monolithic files and Scrawny prevents monolithic
> commits, Sleak prevents monolithic architecture. It does not decide
> that an architecture is bad — it reports where responsibility,
> dependency, and change concentrate, with the evidence to back it, and
> lets a human or a governance policy decide whether that concentration
> is acceptable.

## Status: Phase 4 — Governance

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
  - **SLK009** architectural change concentration
- Git-history ingestion (`sleak-git`): shells out to the `git` CLI
  (matching Scrawny's own abstraction) to find files that repeatedly
  change together across recent commits. History is optional evidence —
  every function degrades to "no history" rather than an error when the
  target isn't a git repository, `git` isn't on `PATH`, or there's no
  history yet; this assumes the git repository root coincides with the
  Cargo workspace root. SLK009 fires when a node was touched by a
  disproportionate fraction of the analysed commits — a signal on the
  *location* ("this keeps attracting change"), complementing Scrawny's
  per-commit "this one change is too broad" (actual cross-tool
  correlation, per the directive's unified `FRACT-001 SCRAWNY-004
  SLEAK-007`-style finding IDs, is Phase 5 scope).
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
- Baselines (directive §16, `sleak-report::baseline`): `sleak baseline
  create` records every current finding's identity (`id` + `target`,
  not its evidence — a finding is "the same" across runs even as its
  numbers shift slightly) to `.sleak-baseline.json`. When that file
  exists, `sleak scan`/`sleak check` still report every current finding
  (nothing is hidden) but `check` only blocks on ones *not* already in
  the baseline — a project doesn't need to reach zero findings before
  Sleak becomes a useful gate.
- Differential analysis (directive §17): `sleak diff <ref>` builds the
  architecture as it exists at `<ref>` into a throwaway temp directory
  via `git archive` (no checkout, no stash, no mutation of the working
  tree in any way) and compares it against the working tree using the
  same baseline machinery, judged against one consistent `.sleak.toml`
  so a mere policy change can't masquerade as an architectural
  regression. Reports new / existing / resolved findings and a
  REGRESSION / IMPROVEMENT / UNCHANGED verdict. SLK009 is excluded from
  the comparison on both sides — a `git archive` snapshot has no `.git`
  history to compute it from, so including it would make every current
  SLK009 finding look "new" on every diff regardless of what changed;
  `sleak scan`/`sleak check` remain the way to see it.

Not yet implemented (later phases per the project directive):
- **SLK005** (god component) and **SLK008** (responsibility clustering)
  are reserved but not built — the directive marks the semantic signals
  they need (naming, documentation terminology, ...) as advisory, and
  none of that exists yet. **SLK010** (dependency bottleneck) is also
  deferred: every crisp definition tried either duplicated SLK001/002/
  006/007 or needed a semantic layer this build doesn't have.
- An **Architecture Health score** (directive §20) is deliberately not
  built yet: every aggregate-percentage design considered either
  invented an unjustifiable weighting or risked exactly the "opaque
  score" the directive itself warns against. The finding-identity-based
  baseline/diff above is the evidence-backed alternative built instead.
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
sleak baseline create .              # accept every current finding as existing debt
sleak diff HEAD~1 .                   # did the working tree regress since HEAD~1?
sleak diff origin/main .
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
max_change_frequency = 0.15  # fraction of analysed commits touching one node (0.0-1.0)

[history]
enabled = true
window = 500   # how many recent commits to analyse

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

History is enabled by default but always optional in practice: scanning
a directory with no `.git` (or where `git` isn't installed) simply
skips SLK009 rather than erroring.

`.sleak-baseline.json` (written by `sleak baseline create`, meant to be
committed) records accepted debt as a plain list of `{"id", "target"}`
pairs — no evidence, since evidence is recomputed fresh every run.
`sleak diff` never reads or writes it; it builds its own reference set
on the fly from the compared ref.
