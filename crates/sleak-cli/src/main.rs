use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use sleak_core::{Result as SleakResult, Severity, SleakError};
use sleak_policy::Config;
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

    /// Output format: human or json.
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
    let (path, mode) = match command {
        Command::Scan { path } => (path, Mode::Scan),
        Command::Check { path } => (path, Mode::Check),
    };

    let model = sleak_rust::build_model(&path)?;
    let graph = sleak_graph::ArchGraph::from_model(&model);
    let degrees = graph.degrees();
    let cycles = graph.cycles();

    let config = Config::load(path.as_std_path())?;
    let findings = sleak_policy::evaluate(&degrees, &cycles, &config);
    let blocking = findings.iter().any(|f| f.severity >= Severity::Major);

    let report = Report::new(findings);
    println!("{}", sleak_report::render(&report, format));

    Ok(match mode {
        Mode::Scan => ExitCode::SUCCESS,
        Mode::Check if blocking => ExitCode::from(1),
        Mode::Check => ExitCode::SUCCESS,
    })
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
