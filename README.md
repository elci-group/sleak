# Sleak

The anti-monolith architecture engine of ELCI.

> Where Fract prevents monolithic files and Scrawny prevents monolithic
> commits, Sleak prevents monolithic architecture. It does not decide
> that an architecture is bad — it reports where responsibility,
> dependency, and change concentrate, with the evidence to back it, and
> lets a human or a governance policy decide whether that concentration
> is acceptable.

## Status: Phase 1 — Cartography

Deterministic, offline, read-only analysis of a Rust Cargo workspace.
No mutation of any kind.

Implemented:
- A typed architectural graph (`sleak-ir`): `Crate` and `Module` nodes,
  `DependsOn` and `Imports` edges.
- Crate-dependency graph construction straight from `cargo metadata`
  (`sleak-rust`) — precise, not heuristic.
- Module-level import graph from `syn`-based parsing of `mod`/`use`
  items (`sleak-rust`) — a best-effort heuristic: only `crate`/`self`/
  `super`-relative and other-workspace-crate-relative imports are
  resolved; macros, glob re-exports and `#[path = "..."]` attributes are
  not. Scope statement lives in `sleak-rust/src/lib.rs`.
- Graph metrics (`sleak-graph`, backed by `petgraph`): fan-in, fan-out,
  and cycle detection via strongly-connected components. Cycles report
  *component membership*, not a minimal simple cycle.
- Policy evaluation (`sleak-policy`) against a `.sleak.toml` config,
  emitting three finding types:
  - **SLK001** excessive fan-in
  - **SLK002** excessive fan-out
  - **SLK003** architectural cycle (module-level in practice — Cargo
    itself forbids cyclic crate dependencies, so SLK003 essentially
    never fires at crate granularity; the algorithm is still built
    generically over the graph so it's ready for edge kinds added later,
    e.g. `calls`, that genuinely can cycle)
- Human and versioned-JSON output (`sleak-report`,
  `{"schema_version":"1","tool":"sleak","findings":[...]}`), each
  finding carrying severity, target, evidence, and (where meaningful) a
  remediation suggestion.
- `sleak scan` and `sleak check`, with the directive's stable exit-code
  contract (§14): `0` clean, `1` blocking findings on `check`, `2`
  configuration error, `3` analysis failure, `4` infrastructure/tooling
  failure.

Not yet implemented (later phases per the project directive):
- **Phase 2 — Topology**: centrality (degree/betweenness/PageRank),
  blast radius / articulation points, declared architectural boundaries
  and boundary-violation findings (SLK004/006/007/010), `sleak explain`,
  `sleak graph` (dot/json export).
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
sleak                    # scan the current directory (human output)
sleak scan .              # same, explicit
sleak scan --format json .
sleak check .              # scan + fail (exit 1) on any MAJOR/CRITICAL finding — the CI gate
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
cycles = "error"   # error | warn | ignore
```

A missing file uses the defaults above. A present-but-invalid file is a
hard configuration error (exit code 2) — Sleak never silently falls back
to defaults on a malformed config, only on an absent one.
