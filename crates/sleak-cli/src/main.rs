mod analysis;
mod graph_render;

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use sleak_core::{Result as SleakResult, Severity, SleakError};
use sleak_report::{Format, Report};
use std::process::ExitCode;
use std::str::FromStr;

/// Sleak — the anti-monolith architecture engine of ELCI. Detects,
/// quantifies and explains where architectural responsibility
/// concentrates in a Rust workspace.
#[derive(Parser)]
#[command(name = "sleak", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Output format for scan/check/explain: human or json.
    #[arg(long, global = true, default_value = "human")]
    format: String,
}

#[derive(Subcommand)]
enum Command {
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let format = match Format::from_str(&cli.format) {
        Ok(format) => format,
        Err(reason) => {
            eprintln!("error: {reason}");
            return ExitCode::from(2);
        }
    };

    let command = cli.command.unwrap_or(Command::Scan {
        path: Utf8PathBuf::from("."),
    });

    match run(command, format) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            exit_code_for(&err)
        }
    }
}

enum Mode {
    Scan,
    Check,
}

fn run(command: Command, format: Format) -> SleakResult<ExitCode> {
    match command {
        Command::Scan { path } => run_scan_or_check(&path, format, Mode::Scan),
        Command::Check { path } => run_scan_or_check(&path, format, Mode::Check),
        Command::Explain { id, path } => run_explain(&id, &path, format),
        Command::Graph { path, output } => run_graph(&path, &output),
    }
}

fn run_scan_or_check(path: &Utf8PathBuf, format: Format, mode: Mode) -> SleakResult<ExitCode> {
    let findings = analysis::analyse(path)?;
    let blocking = findings.iter().any(|f| f.severity >= Severity::Major);

    let report = Report::new(findings);
    println!("{}", sleak_report::render(&report, format));

    Ok(match mode {
        Mode::Scan => ExitCode::SUCCESS,
        Mode::Check if blocking => ExitCode::from(1),
        Mode::Check => ExitCode::SUCCESS,
    })
}

/// Static, taxonomy-level descriptions (directive §18) — not tied to
/// any one run. Only ids Phase 1/2 can actually emit are listed; an
/// unknown id (including the reserved-but-unimplemented SLK005/008/009/
/// 010) is reported as unknown rather than described from a doc string
/// that describes nothing this build can produce.
fn describe(id: &str) -> Option<&'static str> {
    match id {
        "SLK001" => Some(
            "Excessive fan-in — more direct dependants than the configured limit. A change here has an outsized chance of rippling outward.",
        ),
        "SLK002" => Some(
            "Excessive fan-out — depends on more other nodes than the configured limit. This node has accumulated more coordination responsibility than a single component should carry.",
        ),
        "SLK003" => Some(
            "Architectural cycle — a group of nodes are mutually dependent, so none of them can be understood, tested, or changed in isolation from the rest.",
        ),
        "SLK004" => Some(
            "Boundary violation — an edge crosses two declared architectural layers in a direction the project's policy forbids.",
        ),
        "SLK006" => Some(
            "Excessive centrality — this node's PageRank-style influence over the dependency graph exceeds the configured limit; the architecture leans on it more than most.",
        ),
        "SLK007" => Some(
            "High blast radius — removing this node would disconnect a large fraction of the rest of the architecture from the remainder; it is a structural single point of failure.",
        ),
        _ => None,
    }
}

fn run_explain(id: &str, path: &Utf8PathBuf, format: Format) -> SleakResult<ExitCode> {
    let normalized = id.to_uppercase();
    let Some(description) = describe(&normalized) else {
        eprintln!(
            "error: unknown finding id '{id}' (known: SLK001, SLK002, SLK003, SLK004, SLK006, SLK007)"
        );
        return Ok(ExitCode::from(2));
    };
    println!("{normalized}: {description}\n");

    let findings: Vec<_> = analysis::analyse(path)?
        .into_iter()
        .filter(|f| f.id.as_str() == normalized)
        .collect();

    if findings.is_empty() {
        println!("No current {normalized} findings in {path}.");
    } else {
        let report = Report::new(findings);
        println!("{}", sleak_report::render(&report, format));
    }

    Ok(ExitCode::SUCCESS)
}

fn run_graph(path: &Utf8PathBuf, output: &str) -> SleakResult<ExitCode> {
    let model = sleak_rust::build_model(path)?;
    match output {
        "dot" => println!("{}", graph_render::to_dot(&model)),
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&model).expect("ArchitectureModel is always serializable")
        ),
        other => {
            eprintln!("error: unsupported graph output '{other}' (supported: dot, json)");
            return Ok(ExitCode::from(2));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Maps a failure to the stable exit-code contract (directive §14):
/// 2 configuration error, 3 analysis failure, 4 infrastructure/tooling
/// failure. `run` never returns `Ok` with a non-blocking-but-failed
/// state — every non-zero-for-a-reason-other-than-findings exit comes
/// through here.
fn exit_code_for(err: &SleakError) -> ExitCode {
    match err {
        SleakError::NotACargoWorkspace { .. } | SleakError::Configuration { .. } => {
            ExitCode::from(2)
        }
        SleakError::ParseFailure { .. } => ExitCode::from(3),
        SleakError::MetadataFailure { .. } | SleakError::Io { .. } => ExitCode::from(4),
    }
}
