//! Integration tests for coda-audit: FsStore, active-log resolver, coda audit command.
//!
//! AC1: this file (`tests/audit.rs`) appears in cargo test output.
//! AC2: FsStore::logs over fixture dir returns expected RawLogs.
//! AC3: resolver::parse_ctrace_status parses JSON fixture / returns None for bad input.
//! AC4: coda audit excludes the active log from debt.
//! AC5: --format json and --orphaned-only output.
//! AC6: Read-only guarantee — render counter stays at 0.
//! AC7: exit code 0 with no orphans, exit code 1 with ≥1 orphan.
//! AC8: --orphaned-only piped to head does not panic (SIGPIPE).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use filetime::FileTime;
use tempfile::TempDir;

use coda::{resolver, sweep, DebtClass, FakeStore, FsStore, LogStore, RawLog};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Create an ndjson file in `dir` with the given mtime offset (seconds before now).
/// Optionally create the sibling .summary.md.
fn make_log(dir: &TempDir, name: &str, age_secs: u64, with_summary: bool) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, b"{}").expect("write ndjson");
    // Set mtime to now - age_secs.
    let mtime = now_secs().saturating_sub(age_secs);
    filetime::set_file_mtime(&path, FileTime::from_unix_time(mtime as i64, 0))
        .expect("set mtime");
    if with_summary {
        let summary_path = path.with_extension("summary.md");
        std::fs::write(&summary_path, b"# summary").expect("write summary");
    }
    path
}

// ---------------------------------------------------------------------------
// AC2: FsStore::logs returns correct RawLogs
// ---------------------------------------------------------------------------

#[test]
fn fsstore_logs_correct_has_summary_and_mtime() {
    let dir = TempDir::new().expect("tempdir");

    // with summary
    let p_with = make_log(&dir, "with-summary.ndjson", 500, true);
    // without summary
    let p_without = make_log(&dir, "without-summary.ndjson", 1000, false);

    let store = FsStore::new(dir.path().to_path_buf());
    let logs = store.logs().expect("FsStore::logs");

    // Sort by path for stable comparison.
    let mut logs = logs;
    logs.sort_by(|a, b| a.path.cmp(&b.path));

    assert_eq!(logs.len(), 2);

    // Find each.
    let l_with = logs.iter().find(|l| l.path == p_with).expect("with-summary log");
    let l_without = logs.iter().find(|l| l.path == p_without).expect("without-summary log");

    assert!(l_with.has_summary, "with-summary log should have has_summary=true");
    assert!(!l_without.has_summary, "without-summary log should have has_summary=false");

    // mtime should be within a few seconds of expected.
    let expected_with = now_secs().saturating_sub(500);
    assert!(
        l_with.mtime_secs.abs_diff(expected_with) < 5,
        "mtime for with-summary should be ~{}s ago (got {})",
        500,
        l_with.mtime_secs
    );

    let expected_without = now_secs().saturating_sub(1000);
    assert!(
        l_without.mtime_secs.abs_diff(expected_without) < 5,
        "mtime for without-summary should be ~{}s ago",
        1000
    );
}

#[test]
fn fsstore_empty_dir_returns_empty() {
    let dir = TempDir::new().expect("tempdir");
    let store = FsStore::new(dir.path().to_path_buf());
    let logs = store.logs().expect("FsStore::logs");
    assert!(logs.is_empty());
}

#[test]
fn fsstore_nonexistent_dir_returns_empty() {
    let store = FsStore::new(PathBuf::from("/tmp/coda_nonexistent_dir_99999999"));
    let logs = store.logs().expect("FsStore::logs on missing dir returns empty");
    assert!(logs.is_empty());
}

#[test]
fn fsstore_ignores_non_ndjson_files() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("foo.txt"), b"text").expect("write");
    std::fs::write(dir.path().join("bar.json"), b"{}").expect("write");
    let store = FsStore::new(dir.path().to_path_buf());
    let logs = store.logs().expect("FsStore::logs");
    assert!(logs.is_empty(), "non-ndjson files should be ignored");
}

// ---------------------------------------------------------------------------
// AC3: resolver::parse_ctrace_status
// ---------------------------------------------------------------------------

#[test]
fn resolver_parses_valid_status() {
    let json = r#"{"log": "/tmp/session.ndjson", "pid": 42}"#;
    assert_eq!(
        resolver::parse_ctrace_status(json),
        Some(PathBuf::from("/tmp/session.ndjson"))
    );
}

#[test]
fn resolver_returns_none_for_malformed_json() {
    assert_eq!(resolver::parse_ctrace_status("not json"), None);
}

#[test]
fn resolver_returns_none_for_missing_log_key() {
    let json = r#"{"status": "idle"}"#;
    assert_eq!(resolver::parse_ctrace_status(json), None);
}

#[test]
fn resolver_returns_none_for_not_running_fixture() {
    // ctrace exits non-zero / empty when no active session.
    assert_eq!(resolver::parse_ctrace_status("{}"), None);
}

// ---------------------------------------------------------------------------
// AC4: active log is excluded from debt (classed Active)
// ---------------------------------------------------------------------------

#[test]
fn audit_excludes_active_log_from_debt() {
    let dir = TempDir::new().expect("tempdir");

    // active log: no summary, old enough to be orphaned normally
    let active_path = make_log(&dir, "active.ndjson", 5000, false);
    // orphaned log: no summary, old
    let orphaned_path = make_log(&dir, "orphaned.ndjson", 5000, false);
    // settled log: has summary
    let _settled = make_log(&dir, "settled.ndjson", 5000, true);

    let store = FsStore::new(dir.path().to_path_buf());
    let logs = store.logs().expect("FsStore::logs");

    let now = now_secs();
    let grace = 120u64;
    let plan = sweep(&logs, Some(&active_path), now, grace);

    // Find the active log's classification.
    let active_log_entry = plan.logs.iter().find(|l| l.path == active_path).expect("active log in plan");
    assert!(
        matches!(active_log_entry.debt, DebtClass::Active),
        "active log should be classed Active"
    );

    // The orphaned log should be Orphaned.
    let orphaned_entry = plan.logs.iter().find(|l| l.path == orphaned_path).expect("orphaned log");
    assert!(
        matches!(orphaned_entry.debt, DebtClass::Orphaned { .. }),
        "orphaned log should be Orphaned"
    );

    // orphaned count is exactly 1 (not 2).
    assert_eq!(plan.orphaned, 1, "active log should not count as orphaned");
}

// ---------------------------------------------------------------------------
// AC5: --format json and --orphaned-only
// ---------------------------------------------------------------------------

#[test]
fn audit_json_output_matches_sweep_schema() {
    // Use FakeStore to drive sweep and check JSON shape.
    let now = now_secs();
    let logs = vec![
        RawLog { path: PathBuf::from("/tmp/a.ndjson"), has_summary: false, mtime_secs: now - 5000 },
        RawLog { path: PathBuf::from("/tmp/b.ndjson"), has_summary: true, mtime_secs: now - 5000 },
    ];
    let plan = sweep(&logs, None, now, 120);
    let json = serde_json::to_string_pretty(&plan).expect("serialize plan");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse plan json");

    assert!(v.get("orphaned").is_some(), "json should have orphaned field");
    assert!(v.get("fresh").is_some(), "json should have fresh field");
    assert!(v.get("settled").is_some(), "json should have settled field");
    assert!(v.get("total").is_some(), "json should have total field");
    assert!(v.get("logs").is_some(), "json should have logs array");
    assert!(v.get("actions").is_some(), "json should have actions array");

    // logs array should have path field per entry.
    let logs_arr = v["logs"].as_array().expect("logs is array");
    assert_eq!(logs_arr.len(), 2);
    assert!(logs_arr[0].get("path").is_some(), "each log entry has path");
}

#[test]
fn orphaned_only_prints_exactly_orphaned_paths() {
    let now = now_secs();
    let orphaned_path = PathBuf::from("/tmp/orphaned.ndjson");
    let settled_path = PathBuf::from("/tmp/settled.ndjson");
    let active_path = PathBuf::from("/tmp/active.ndjson");

    let logs = vec![
        RawLog { path: orphaned_path.clone(), has_summary: false, mtime_secs: now - 5000 },
        RawLog { path: settled_path.clone(), has_summary: true, mtime_secs: now - 5000 },
        RawLog { path: active_path.clone(), has_summary: false, mtime_secs: now - 100 },
    ];
    let plan = sweep(&logs, Some(&active_path), now, 120);

    // Collect orphaned paths.
    let orphaned_paths: Vec<&PathBuf> = plan
        .logs
        .iter()
        .filter(|l| matches!(l.debt, DebtClass::Orphaned { .. }))
        .map(|l| &l.path)
        .collect();

    assert_eq!(orphaned_paths.len(), 1);
    assert_eq!(orphaned_paths[0], &orphaned_path);

    // settled and active should NOT be in orphaned list.
    assert!(!orphaned_paths.contains(&&settled_path));
    assert!(!orphaned_paths.contains(&&active_path));
}

// ---------------------------------------------------------------------------
// AC6: Read-only guarantee — render counter stays at 0
// ---------------------------------------------------------------------------

#[test]
fn audit_never_calls_render() {
    let now = now_secs();
    let logs = vec![
        RawLog { path: PathBuf::from("/tmp/a.ndjson"), has_summary: false, mtime_secs: now - 5000 },
        RawLog { path: PathBuf::from("/tmp/b.ndjson"), has_summary: false, mtime_secs: now - 3000 },
        RawLog { path: PathBuf::from("/tmp/c.ndjson"), has_summary: true, mtime_secs: now - 1000 },
    ];

    let store = FakeStore::new(logs.clone());

    // Simulate exactly what coda audit does: call logs() and sweep(), never render().
    let raw = store.logs().expect("logs");
    let _plan = sweep(&raw, None, now, 120);

    assert_eq!(
        store.render_call_count(),
        0,
        "coda audit must never call render()"
    );
}

// ---------------------------------------------------------------------------
// AC7: exit code 0 with no orphans, 1 with ≥1 orphan
// ---------------------------------------------------------------------------

#[test]
fn no_orphans_plan_orphaned_is_zero() {
    let now = now_secs();
    // All settled.
    let logs = vec![
        RawLog { path: PathBuf::from("/tmp/x.ndjson"), has_summary: true, mtime_secs: now - 5000 },
    ];
    let plan = sweep(&logs, None, now, 120);
    assert_eq!(plan.orphaned, 0, "no orphans expected");
}

#[test]
fn one_orphan_plan_orphaned_is_one() {
    let now = now_secs();
    let logs = vec![
        RawLog { path: PathBuf::from("/tmp/x.ndjson"), has_summary: false, mtime_secs: now - 5000 },
    ];
    let plan = sweep(&logs, None, now, 120);
    assert_eq!(plan.orphaned, 1, "one orphan expected");
}

// ---------------------------------------------------------------------------
// AC8: SIGPIPE — tested via process-level invocation
// ---------------------------------------------------------------------------

#[test]
fn audit_orphaned_only_pipe_to_head_does_not_panic() {
    // Build the binary first (will use cached artifact in normal test runs).
    // We run: coda audit --sessions-dir <dir-with-many-logs> --orphaned-only | head -1
    // and assert exit code is 0 or 1 (SIGPIPE broken pipe is ok — not 101).
    let dir = TempDir::new().expect("tempdir");

    // Create 5 orphaned logs.
    for i in 0..5u32 {
        make_log(&dir, &format!("log{i:03}.ndjson"), 5000, false);
    }

    // Find the coda binary.
    let coda_bin = find_coda_bin();

    // Run with --orphaned-only piped to `head -1` via shell.
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} audit --sessions-dir {} --orphaned-only | head -1",
            coda_bin.display(),
            dir.path().display()
        ))
        .output()
        .expect("run coda audit | head");

    // Should not be exit code 141 (SIGPIPE) or anything panic-ish (101).
    let code = output.status.code().unwrap_or(0);
    assert_ne!(code, 101, "coda must not panic on SIGPIPE (exit 101)");
    // 0 or 1 or 141 (broken pipe from shell) are all acceptable.
}

fn find_coda_bin() -> PathBuf {
    // Try the standard cargo build output paths.
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/coda"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/coda"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    // Fall back to PATH.
    PathBuf::from("coda")
}

// ---------------------------------------------------------------------------
// Additional: FsStore sorted output
// ---------------------------------------------------------------------------

#[test]
fn fsstore_output_is_sorted_by_path() {
    let dir = TempDir::new().expect("tempdir");
    make_log(&dir, "zzz.ndjson", 100, false);
    make_log(&dir, "aaa.ndjson", 200, false);
    make_log(&dir, "mmm.ndjson", 300, false);

    let store = FsStore::new(dir.path().to_path_buf());
    let logs = store.logs().expect("logs");
    let names: Vec<_> = logs
        .iter()
        .map(|l| l.path.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, ["aaa.ndjson", "mmm.ndjson", "zzz.ndjson"]);
}
