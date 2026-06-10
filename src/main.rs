//! coda — summary-debt model for ctrace session logs.
//!
//! ## Usage
//!
//! ```text
//! coda plan [--format table|json] [--sessions-dir <path>] [--grace-secs <n>]
//! coda audit [--format table|json] [--sessions-dir <path>] [--grace-secs <n>] [--verbose] [--orphaned-only]
//! coda close [--apply] [--limit N] [--json] [--sessions-dir <path>]
//! coda boot install [--enable]
//! ```
//!
//! Exit code: 1 when at least one log is `Orphaned`, 0 otherwise.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use coda::{
    resolver, sweep, BootInstallArgs, CloseArgs, CodaConfig, FsStore, LogStore, RawLog,
    SweepAction, run_boot_install, run_close,
};

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
    /// Scan sessions dir, classify via sweep, and report summary debt (read-only).
    Audit(AuditArgs),
    /// Close orphaned log debt: render summaries for orphaned logs.
    Close(CloseCliArgs),
    /// Install systemd timer and SessionStart hook for automated close.
    Boot {
        #[command(subcommand)]
        subcmd: BootSubcommand,
    },
}

#[derive(Subcommand, Debug)]
enum BootSubcommand {
    /// Print the SessionStart hook entry and optionally install systemd units.
    Install(BootInstallCliArgs),
}

#[derive(clap::Args, Debug)]
struct CloseCliArgs {
    /// Actually render orphaned logs (default: print-only).
    #[arg(long)]
    apply: bool,

    /// Cap renders per invocation.
    #[arg(long)]
    limit: Option<usize>,

    /// Emit JSON summary instead of human-readable text.
    #[arg(long)]
    json: bool,

    /// Override the sessions directory from config.
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
struct BootInstallCliArgs {
    /// Copy unit files to ~/.config/systemd/user/ and run daemon-reload.
    #[arg(long)]
    enable: bool,
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

#[derive(clap::Args, Debug)]
struct AuditArgs {
    /// Output format (ignored when --orphaned-only is set).
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,

    /// Override the sessions directory from config.
    #[arg(long)]
    sessions_dir: Option<PathBuf>,

    /// Override the grace period (seconds) from config.
    #[arg(long)]
    grace_secs: Option<u64>,

    /// Print per-log details in table output.
    #[arg(long)]
    verbose: bool,

    /// Print only orphaned log paths, one per line (for piping to coda-close).
    #[arg(long)]
    orphaned_only: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Table,
    Json,
}

fn main() -> ExitCode {
    // SIGPIPE reset must be first: `coda plan | head` and `coda audit --orphaned-only | head`
    // must not panic.
    // Safety: this crate sets deny(unsafe_code); sigpipe::reset() is safe.
    #[allow(unused_must_use)]
    {
        sigpipe::reset();
    }

    let cli = Cli::parse();
    match cli.command {
        Commands::Plan(args) => run_plan(args),
        Commands::Audit(args) => run_audit(args),
        Commands::Close(args) => dispatch_close(args),
        Commands::Boot { subcmd } => dispatch_boot(subcmd),
    }
}

fn run_plan(args: PlanArgs) -> ExitCode {
    // Load config (absent config yields defaults — no error).
    let mut cfg = match CodaConfig::load_default() {
        Ok(c) => c,
        Err(e) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("coda: config error: {e}");
            }
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

fn run_audit(args: AuditArgs) -> ExitCode {
    let mut cfg = match CodaConfig::load_default() {
        Ok(c) => c,
        Err(e) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("coda: config error: {e}");
            }
            return ExitCode::from(2);
        }
    };

    if let Some(dir) = args.sessions_dir {
        cfg.sessions_dir = dir;
    }
    if let Some(grace) = args.grace_secs {
        cfg.grace_secs = grace;
    }

    // Use FsStore for the real sessions directory.
    let store = FsStore::new(cfg.sessions_dir.clone());
    let raw_logs = match store.logs() {
        Ok(l) => l,
        Err(e) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("coda audit: store error: {e}");
            }
            return ExitCode::from(2);
        }
    };

    // Resolve the active log (shells ctrace status; returns None if not running).
    let active_log = resolver::resolve_active_log();
    let active_log_ref = active_log.as_deref();

    let now = now_secs();
    let plan = sweep(&raw_logs, active_log_ref, now, cfg.grace_secs);

    if args.orphaned_only {
        print_orphaned_only(&plan);
    } else {
        match args.format {
            OutputFormat::Table => print_audit_table(&plan, args.verbose),
            OutputFormat::Json => print_json(&plan),
        }
    }

    if plan.orphaned > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Enumerate `.ndjson` files in `dir` (if it exists) as [`RawLog`]s.
/// Falls back to empty vec on any I/O error.
///
/// Used by `coda plan` which predates `FsStore`; kept for backwards compat.
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

/// Print a compact tally line plus, with `verbose`, the per-log table.
#[allow(clippy::print_stdout)]
fn print_audit_table(plan: &coda::SweepPlan, verbose: bool) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // Find the oldest orphan age.
    let oldest_orphan = plan
        .logs
        .iter()
        .filter_map(|l| {
            if let coda::DebtClass::Orphaned { age_secs } = l.debt {
                Some(age_secs)
            } else {
                None
            }
        })
        .max();

    writeln!(
        out,
        "{} orphaned, {} fresh, {} settled / {} total",
        plan.orphaned, plan.fresh, plan.settled, plan.total
    )
    .ok();

    if let Some(age) = oldest_orphan {
        writeln!(out, "oldest orphan: {age}s ago").ok();
    }

    if verbose {
        writeln!(
            out,
            "\n{:<60} {:<10} {:<10} {:<10}",
            "log", "state", "age_sec", "action"
        )
        .ok();
        writeln!(out, "{}", "-".repeat(95)).ok();
        for (log, action) in plan.logs.iter().zip(plan.actions.iter()) {
            let state = match &log.debt {
                coda::DebtClass::Active => "active",
                coda::DebtClass::Fresh { .. } => "fresh",
                coda::DebtClass::Orphaned { .. } => "orphaned",
                coda::DebtClass::Settled => "settled",
            };
            let action_str = match action {
                SweepAction::Render { .. } => "render",
                SweepAction::Skip { .. } => "skip",
                SweepAction::NoOp { .. } => "noop",
            };
            let name = log.path.file_name().map_or_else(
                || log.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            writeln!(
                out,
                "{:<60} {:<10} {:<10} {:<10}",
                name, state, log.age_secs, action_str
            )
            .ok();
        }
        writeln!(out, "{}", "-".repeat(95)).ok();
    }
}

/// Print only orphaned log paths, one per line.
#[allow(clippy::print_stdout)]
fn print_orphaned_only(plan: &coda::SweepPlan) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for log in &plan.logs {
        if matches!(log.debt, coda::DebtClass::Orphaned { .. }) {
            writeln!(out, "{}", log.path.display()).ok();
        }
    }
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

fn dispatch_close(args: CloseCliArgs) -> ExitCode {
    let cfg = match CodaConfig::load_default() {
        Ok(c) => c,
        Err(e) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("coda: config error: {e}");
            }
            return ExitCode::from(2);
        }
    };

    let close_args = CloseArgs {
        apply: args.apply,
        limit: args.limit,
        json: args.json,
        sessions_dir: args.sessions_dir,
        render_cmd: None,
    };

    match run_close(&close_args, &cfg) {
        Ok(summary) => {
            if summary.remaining > 0 || summary.failed > 0 {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("coda close: {e}");
            }
            ExitCode::from(2)
        }
    }
}

fn dispatch_boot(subcmd: BootSubcommand) -> ExitCode {
    match subcmd {
        BootSubcommand::Install(args) => {
            let boot_args = BootInstallArgs { enable: args.enable };
            match run_boot_install(&boot_args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    #[allow(clippy::print_stderr)]
                    {
                        eprintln!("coda boot: {e}");
                    }
                    ExitCode::from(2)
                }
            }
        }
    }
}
