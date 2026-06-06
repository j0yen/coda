//! coda — summary-debt model for ctrace session logs.
//!
//! ## Usage
//!
//! ```text
//! coda plan [--format table|json] [--sessions-dir <path>] [--grace-secs <n>]
//! ```
//!
//! Exit code: 1 when at least one log is `Orphaned`, 0 otherwise.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use coda::{sweep, CodaConfig, RawLog, SweepAction};

#[derive(Parser, Debug)]
#[command(
    name = "coda",
    version,
    about = "Summary-debt model and sweep planner for ctrace session logs"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show the sweep plan: classify all session logs and print a table or JSON.
    Plan(PlanArgs),
}

#[derive(clap::Args, Debug)]
struct PlanArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,

    /// Override the sessions directory from config.
    #[arg(long)]
    sessions_dir: Option<PathBuf>,

    /// Override the grace period (seconds) from config.
    #[arg(long)]
    grace_secs: Option<u64>,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Table,
    Json,
}

fn main() -> ExitCode {
    // SIGPIPE reset must be first: `coda plan | head` must not panic.
    // Safety: this crate sets deny(unsafe_code); sigpipe::reset() is safe.
    #[allow(unused_must_use)]
    {
        sigpipe::reset();
    }

    let cli = Cli::parse();
    match cli.command {
        Commands::Plan(args) => run_plan(args),
    }
}

fn run_plan(args: PlanArgs) -> ExitCode {
    // Load config (absent config yields defaults — no error).
    let mut cfg = match CodaConfig::load_default() {
        Ok(c) => c,
        Err(e) => {
            #[allow(clippy::print_stderr)]
            { eprintln!("coda: config error: {e}"); }
            return ExitCode::from(2);
        }
    };

    // CLI overrides take precedence over config.
    if let Some(dir) = args.sessions_dir {
        cfg.sessions_dir = dir;
    }
    if let Some(grace) = args.grace_secs {
        cfg.grace_secs = grace;
    }

    // In coda-sweep the real FsStore does not exist yet (it ships in coda-audit).
    // When the sessions dir exists, we enumerate it; otherwise we use an empty FakeStore.
    let raw_logs = load_logs(&cfg.sessions_dir);

    // active_log: in the future coda-audit will detect it. For now, None.
    let plan = sweep(&raw_logs, None, now_secs(), cfg.grace_secs);

    match args.format {
        OutputFormat::Table => print_table(&plan),
        OutputFormat::Json => print_json(&plan),
    }

    // Exit non-zero if any log is Orphaned.
    if plan.orphaned > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Enumerate `.ndjson` files in `dir` (if it exists) as [`RawLog`]s.
/// Falls back to empty vec on any I/O error.
fn load_logs(dir: &std::path::Path) -> Vec<RawLog> {
    if !dir.is_dir() {
        return vec![];
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut logs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "ndjson") {
            let mtime_secs = entry
                .metadata()
                .ok()
                .and_then(|m| {
                    m.modified().ok().and_then(|t| {
                        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
                    })
                })
                .unwrap_or(0);
            let summary_path = path.with_extension("summary.md");
            let has_summary = summary_path.exists();
            logs.push(RawLog {
                path,
                has_summary,
                mtime_secs,
            });
        }
    }
    // Sort by path for deterministic output.
    logs.sort_by(|a, b| a.path.cmp(&b.path));
    logs
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn print_table(plan: &coda::SweepPlan) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    // Header
    writeln!(
        out,
        "{:<60} {:<10} {:<10} {:<10}",
        "log", "state", "age_sec", "action"
    )
    .ok();
    writeln!(out, "{}", "-".repeat(95)).ok();
    for (log, action) in plan.logs.iter().zip(plan.actions.iter()) {
        let state = match &log.debt {
            coda::DebtClass::Active => "active".to_string(),
            coda::DebtClass::Fresh { .. } => "fresh".to_string(),
            coda::DebtClass::Orphaned { .. } => "orphaned".to_string(),
            coda::DebtClass::Settled => "settled".to_string(),
        };
        let action_str = match action {
            SweepAction::Render { .. } => "render",
            SweepAction::Skip { .. } => "skip",
            SweepAction::NoOp { .. } => "noop",
        };
        let name = log
            .path
            .file_name()
            .map_or_else(|| log.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        writeln!(
            out,
            "{:<60} {:<10} {:<10} {:<10}",
            name, state, log.age_secs, action_str
        )
        .ok();
    }
    writeln!(out, "{}", "-".repeat(95)).ok();
    writeln!(
        out,
        "total={} orphaned={} fresh={} settled={}",
        plan.total, plan.orphaned, plan.fresh, plan.settled
    )
    .ok();
}

#[allow(clippy::print_stdout, clippy::print_stderr)]
fn print_json(plan: &coda::SweepPlan) {
    match serde_json::to_string_pretty(plan) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("coda: JSON serialization error: {e}"),
    }
}
