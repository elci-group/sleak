mod analysis;
mod cli;
mod graph_render;

use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use cli::{BaselineAction, Cli, Command};
use sleak_core::{Result as SleakResult, Severity, SleakError};
use sleak_policy::Config;
use sleak_report::{Baseline, BaselineSummary, Format, Report};
use std::process::ExitCode;
use std::str::FromStr;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Scan {
        path: Utf8PathBuf::from("."),
    });

    match run(command, &cli.format) {
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

/// `scan`/`check` accept one more format than everything else: SARIF
/// (directive §13) is meaningful for a finding report but not for
/// `explain`'s or `diff`'s different shapes, so it's scoped to this
/// enum rather than `sleak_report::Format` itself.
enum OutputFormat {
    Human,
    Json,
    Sarif,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            "sarif" => Ok(Self::Sarif),
            other => Err(format!(
                "unsupported format '{other}' (supported: human, json, sarif)"
            )),
        }
    }
}

fn run(command: Command, format: &str) -> SleakResult<ExitCode> {
    match command {
        Command::Scan { path } => run_scan_or_check(&path, format, Mode::Scan),
        Command::Check { path, ci } => {
            run_scan_or_check(&path, if ci { "json" } else { format }, Mode::Check)
        }
        Command::Explain { id, path } => run_explain(&id, &path, format),
        Command::Graph { path, output } => run_graph(&path, &output),
        Command::Baseline {
            action: BaselineAction::Create { path },
        } => run_baseline_create(&path),
        Command::Diff { git_ref, path } => run_diff(&git_ref, &path, format),
    }
}

fn run_scan_or_check(path: &Utf8PathBuf, format: &str, mode: Mode) -> SleakResult<ExitCode> {
    let output_format = match OutputFormat::from_str(format) {
        Ok(f) => f,
        Err(reason) => {
            eprintln!("error: {reason}");
            return Ok(ExitCode::from(2));
        }
    };

    let (findings, locations) = analysis::analyse(path)?;

    let baseline_path = path.join(sleak_report::BASELINE_FILE_NAME);
    let existing_baseline = Baseline::load(baseline_path.as_std_path())?;

    // Blocking severity is judged only against *new* findings once a
    // baseline exists (directive §16) — a project's already-accepted
    // debt shouldn't keep failing `check` forever. With no baseline,
    // every current finding is judged, same as Phase 1.
    let (judged, baseline_summary) = match &existing_baseline {
        Some(baseline) => {
            let outcome = sleak_report::diff_findings(&findings, baseline);
            let summary = BaselineSummary {
                new_count: outcome.new.len(),
                existing_count: outcome.existing.len(),
                resolved: outcome.resolved,
            };
            (outcome.new, Some(summary))
        }
        None => (findings.clone(), None),
    };
    let blocking = judged.iter().any(|f| f.severity >= Severity::Major);

    match output_format {
        OutputFormat::Sarif => println!("{}", sleak_sarif::render_sarif(&findings, &locations)),
        OutputFormat::Human | OutputFormat::Json => {
            let mut report = Report::new(findings);
            report.baseline = baseline_summary;
            let format = if matches!(output_format, OutputFormat::Json) {
                Format::Json
            } else {
                Format::Human
            };
            println!("{}", sleak_report::render(&report, format));
        }
    }

    Ok(match mode {
        Mode::Scan => ExitCode::SUCCESS,
        Mode::Check if blocking => ExitCode::from(1),
        Mode::Check => ExitCode::SUCCESS,
    })
}

fn run_baseline_create(path: &Utf8PathBuf) -> SleakResult<ExitCode> {
    let (findings, _locations) = analysis::analyse(path)?;
    let baseline = Baseline::from_findings(&findings);
    let baseline_path = path.join(sleak_report::BASELINE_FILE_NAME);
    baseline.save(baseline_path.as_std_path())?;

    println!(
        "Baseline created at {baseline_path}: {} finding(s) recorded as accepted debt.",
        baseline.findings.len()
    );
    Ok(ExitCode::SUCCESS)
}

fn run_diff(git_ref: &str, path: &Utf8PathBuf, format: &str) -> SleakResult<ExitCode> {
    let format = match Format::from_str(format) {
        Ok(f) => f,
        Err(reason) => {
            eprintln!("error: {reason}");
            return Ok(ExitCode::from(2));
        }
    };

    let config = Config::load(path.as_std_path())?;
    let (current, _locations) = analysis::analyse_with_config(path, &config)?;

    let snapshot = tempfile::tempdir().map_err(|source| SleakError::Io {
        path: path.as_std_path().to_path_buf(),
        source,
    })?;
    let snapshot_path = Utf8Path::from_path(snapshot.path())
        .expect("tempdir path is valid UTF-8")
        .to_path_buf();
    sleak_git::archive_ref(path, git_ref, &snapshot_path)?;
    let (reference, _locations) = analysis::analyse_with_config(&snapshot_path, &config)?;

    // `git archive` extracts a bare file tree with no `.git` directory,
    // so the reference snapshot can never have SLK009 findings of its
    // own — comparing it against the working tree's real history would
    // make every current SLK009 finding look "new" on every diff,
    // regardless of whether this change touched anything. SLK009 is
    // inherently a trend-over-real-history signal, not a snapshot
    // property, so it's excluded from both sides here; `sleak scan`/
    // `sleak check` remain the way to see it.
    let current = without_change_concentration(current);
    let reference = without_change_concentration(reference);

    let baseline = Baseline::from_findings(&reference);
    let outcome = sleak_report::diff_findings(&current, &baseline);

    let verdict = if !outcome.new.is_empty() {
        "REGRESSION"
    } else if !outcome.resolved.is_empty() {
        "IMPROVEMENT"
    } else {
        "UNCHANGED"
    };

    match format {
        Format::Json => {
            let payload = serde_json::json!({
                "schema_version": sleak_report::REPORT_SCHEMA_VERSION,
                "tool": "sleak",
                "compared_to": git_ref,
                "verdict": verdict,
                "new": outcome.new,
                "existing": outcome.existing,
                "resolved": outcome.resolved,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).expect("payload is always serializable")
            );
        }
        Format::Human => {
            println!("Architectural diff: {git_ref} -> working tree\n");
            print_finding_list(
                &format!("New findings ({})", outcome.new.len()),
                &outcome.new,
            );
            if !outcome.resolved.is_empty() {
                println!("Resolved findings ({}):", outcome.resolved.len());
                for entry in &outcome.resolved {
                    println!("  {} {}", entry.id, entry.target);
                }
                println!();
            }
            println!(
                "Unchanged findings still present: {}",
                outcome.existing.len()
            );
            println!("\nVerdict: {verdict}");
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn without_change_concentration(findings: Vec<sleak_core::Finding>) -> Vec<sleak_core::Finding> {
    findings
        .into_iter()
        .filter(|f| f.id.as_str() != "SLK009")
        .collect()
}

fn print_finding_list(heading: &str, findings: &[sleak_core::Finding]) {
    println!("{heading}:");
    if findings.is_empty() {
        println!("  (none)");
    } else {
        for finding in findings {
            println!("  {} {}", finding.id, finding.target);
        }
    }
    println!();
}

fn run_explain(id: &str, path: &Utf8PathBuf, format: &str) -> SleakResult<ExitCode> {
    let format = match Format::from_str(format) {
        Ok(f) => f,
        Err(reason) => {
            eprintln!("error: {reason}");
            return Ok(ExitCode::from(2));
        }
    };

    let normalized = id.to_uppercase();
    let Some(description) = sleak_core::describe(&normalized) else {
        eprintln!(
            "error: unknown finding id '{id}' (known: SLK001, SLK002, SLK003, SLK004, SLK006, SLK007, SLK009)"
        );
        return Ok(ExitCode::from(2));
    };
    println!("{normalized}: {description}\n");

    let (all_findings, _locations) = analysis::analyse(path)?;
    let findings: Vec<_> = all_findings
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
    let (model, _file_index) = sleak_rust::build_model(path)?;
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
        SleakError::MetadataFailure { .. }
        | SleakError::GitFailure { .. }
        | SleakError::Io { .. } => ExitCode::from(4),
    }
}
