//! `coda close` subcommand — idempotent actuator for orphaned log debt.
//!
//! Drives [`sweep`] to identify [`SweepAction::Render`] candidates, then
//! optionally invokes [`LogStore::render`] for each one.  Print-only by
//! default; `--apply` triggers the actual render.
//!
//! ## Exit codes
//!
//! - `0`: all orphans were rendered (or none existed), or print-only and
//!        zero orphans.
//! - `1`: at least one orphan remains after the run (print-only with orphans,
//!        failed renders, or capped with remaining debt).
//! - `2`: configuration or store error.

use std::io::Write as _;
use std::path::PathBuf;

use crate::{CodaConfig, FsStore, LogStore, SweepAction, resolver, sweep};

/// CLI arguments for `coda close`.
#[derive(Debug, Clone)]
pub struct CloseArgs {
    /// If false, print-only (no render). If true, invoke render for each orphan.
    pub apply: bool,
    /// Cap the number of renders per invocation.
    pub limit: Option<usize>,
    /// Emit JSON summary instead of human-readable text.
    pub json: bool,
    /// Override the sessions directory from config.
    pub sessions_dir: Option<PathBuf>,
    /// Override the render command (defaults to `"scribe"`). Useful in tests.
    pub render_cmd: Option<String>,
}

/// Summary tallies returned from [`run_close`] (and emitted in `--json` mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseSummary {
    /// Number of logs successfully rendered in this run.
    pub rendered: usize,
    /// Number of logs skipped (fresh / settled / active / already done).
    pub skipped: usize,
    /// Number of logs that failed to render.
    pub failed: usize,
    /// Number of orphans not touched (due to limit or no --apply).
    pub remaining: usize,
}

/// Run the `close` subcommand.
///
/// Returns `Ok(summary)` on success (even if some renders failed; errors are
/// counted in `summary.failed`).  Returns `Err(String)` for configuration or
/// store errors that prevent any work.
///
/// # Errors
///
/// Returns an error string if the config or store cannot be initialised.
pub fn run_close(args: &CloseArgs, config: &CodaConfig) -> Result<CloseSummary, String> {
    // Apply CLI override for sessions_dir.
    let mut cfg = config.clone();
    if let Some(ref dir) = args.sessions_dir {
        cfg.sessions_dir = dir.clone();
    }

    let store = {
        let base = FsStore::new(cfg.sessions_dir.clone());
        if let Some(ref cmd) = args.render_cmd {
            base.with_render_cmd(cmd.clone())
        } else {
            base
        }
    };
    let raw_logs = store
        .logs()
        .map_err(|e| format!("coda close: store error: {e}"))?;

    let active_log = resolver::resolve_active_log();
    let active_log_ref = active_log.as_deref();
    let now = now_secs();
    let plan = sweep(&raw_logs, active_log_ref, now, cfg.grace_secs);

    // Collect Render actions.
    let render_paths: Vec<PathBuf> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let SweepAction::Render { path } = a {
                Some(path.clone())
            } else {
                None
            }
        })
        .collect();

    let total_orphans = render_paths.len();
    let already_settled = plan.settled;

    // Apply limit.
    let (to_render, capped_remaining) = match args.limit {
        Some(n) if n < total_orphans => (&render_paths[..n], total_orphans - n),
        _ => (&render_paths[..], 0),
    };

    let mut rendered = 0usize;
    let mut failed = 0usize;

    if args.apply {
        for path in to_render {
            match store.render(path) {
                Ok(()) => rendered += 1,
                Err(e) => {
                    failed += 1;
                    #[allow(clippy::print_stderr)]
                    {
                        let stderr = std::io::stderr();
                        let mut err = stderr.lock();
                        writeln!(err, "coda close: render failed for {}: {e}", path.display())
                            .ok();
                    }
                }
            }
        }
    }

    let remaining = if args.apply {
        capped_remaining + failed
    } else {
        total_orphans
    };

    let skipped = already_settled;

    let summary = CloseSummary {
        rendered,
        skipped,
        failed,
        remaining,
    };

    emit_output(args, &summary, total_orphans, capped_remaining);

    Ok(summary)
}

/// Emit either JSON or human-readable output.
#[allow(clippy::print_stdout)]
fn emit_output(
    args: &CloseArgs,
    summary: &CloseSummary,
    total_orphans: usize,
    capped_remaining: usize,
) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if args.json {
        writeln!(
            out,
            "{{\"rendered\": {}, \"skipped\": {}, \"failed\": {}, \"remaining\": {}}}",
            summary.rendered, summary.skipped, summary.failed, summary.remaining
        )
        .ok();
        return;
    }

    if !args.apply {
        // Print-only mode.
        writeln!(
            out,
            "would render {}, {} already settled",
            total_orphans, summary.skipped
        )
        .ok();
        return;
    }

    // --apply mode.
    if let Some(n) = args.limit {
        if capped_remaining > 0 {
            writeln!(out, "capped at {n}, {capped_remaining} orphans remain").ok();
            return;
        }
    }

    writeln!(
        out,
        "rendered {}, {} failed, {} remaining",
        summary.rendered, summary.failed, summary.remaining
    )
    .ok();
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
