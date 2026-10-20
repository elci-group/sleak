//! Command-line surface of `sleak`, defined as a standalone module so the
//! `gen-man` example can render it into man pages.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

/// Sleak — the anti-monolith architecture engine of ELCI. Detects,
/// quantifies and explains where architectural responsibility
/// concentrates in a Rust workspace.
#[derive(Parser)]
#[command(name = "sleak", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Output format for scan/check/explain/diff: human or json.
    /// `scan`/`check` also accept sarif.
    #[arg(long, global = true, default_value = "human")]
    pub format: String,
}

#[derive(Subcommand)]
pub enum Command {
    /// Analyse the workspace and report findings. Always exits 0 on a
    /// successful run, regardless of what it finds.
    Scan {
        #[arg(default_value = ".")]
        path: Utf8PathBuf,
    },
    /// Analyse the workspace and fail (exit 1) if any finding is MAJOR
    /// or CRITICAL. The canonical CI invocation.
    Check {
        #[arg(default_value = ".")]
        path: Utf8PathBuf,
        /// Canonical CI mode (directive §30): forces JSON output
        /// regardless of `--format`. Behaviour is otherwise identical —
        /// `check` already loads config, consults the baseline, and
        /// returns a deterministic exit code with no interactive
        /// prompts by default.
        #[arg(long)]
        ci: bool,
    },
    /// Explain one finding id: a general description of what it means,
    /// plus every currently-detected finding with that id and its full
    /// evidence.
    Explain {
        id: String,
        #[arg(default_value = ".")]
        path: Utf8PathBuf,
    },
    /// Export the raw architecture graph (nodes + edges), bypassing
    /// policy findings entirely.
    Graph {
        #[arg(default_value = ".")]
        path: Utf8PathBuf,
        /// dot or json. Independent of `--format`, which only applies
        /// to finding reports.
        #[arg(long = "output", default_value = "dot")]
        output: String,
    },
    /// Manage the accepted-debt baseline (directive §16).
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },
    /// Compare the working tree's architecture against `git_ref`
    /// (directive §17) — did this change increase concentration?
    /// Informational: always exits 0 on a successful run, regardless of
    /// what it finds. Requires `path` to be a git repository.
    Diff {
        git_ref: String,
        #[arg(default_value = ".")]
        path: Utf8PathBuf,
    },
}

#[derive(Subcommand)]
pub enum BaselineAction {
    /// Record every current finding as accepted debt. From then on,
    /// `sleak check` only blocks on *new* findings not already in the
    /// baseline — a project doesn't need to reach zero findings before
    /// Sleak becomes useful as a gate.
    Create {
        #[arg(default_value = ".")]
        path: Utf8PathBuf,
    },
}
