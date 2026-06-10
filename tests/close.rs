//! Integration tests for `coda close`.
//!
//! Uses a real temp filesystem and a fake `scribe` script so the render
//! path is exercised end-to-end without requiring the real `scribe` binary.
//!
//! AC1: print-only (no --apply) — 2 orphans, 0 renders, exit non-zero.
//! AC2: --apply — renders exactly once per orphan, exits 0.
//! AC3: idempotency — after --apply, second run renders 0.
//! AC4: --limit 1 with 2 orphans — renders 1, remaining = 1, non-zero exit.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use filetime::FileTime;
use tempfile::TempDir;

use coda::{CloseArgs, CodaConfig};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Create an orphaned `.ndjson` file (no summary, mtime far in the past).
fn make_orphan(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, b"{}").expect("write ndjson");
    // Make it old enough to be Orphaned (grace default = 120s; use 1h).
    let old_mtime = now_secs().saturating_sub(3600);
    filetime::set_file_mtime(&path, FileTime::from_unix_time(old_mtime as i64, 0))
        .expect("set mtime");
    path
}

/// Create a temporary fake `scribe` shell script in a bin subdirectory.
///
/// The fake scribe simulates `scribe render <path>` by writing a
/// `.summary.md` beside the log.  The script writes to the canonical
/// `<stem>.summary.md` path (i.e. `path.with_extension("summary.md")`).
///
/// Returns the full path to the fake `scribe` binary.
fn make_fake_scribe(tmp: &TempDir) -> PathBuf {
    let bin_dir = tmp.path().join("fake_bin");
    std::fs::create_dir_all(&bin_dir).expect("create fake_bin");
    let script_path = bin_dir.join("scribe");
    // $2 is the path argument; `scribe render <path>`.
    // Write to `${2%.ndjson}.summary.md` → canonical sibling path.
    let script = "#!/usr/bin/env bash\nbase=\"${2%.ndjson}\"\necho rendered > \"${base}.summary.md\"\n";
    std::fs::write(&script_path, script).expect("write scribe script");
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod scribe");
    script_path
}

/// Build a `CodaConfig` pointing at `sessions_dir` with a 0-second grace
/// (everything is immediately Orphaned).
fn make_cfg(sessions_dir: &Path) -> CodaConfig {
    CodaConfig {
        grace_secs: 0,
        sessions_dir: sessions_dir.to_path_buf(),
    }
}

// ---------------------------------------------------------------------------
// AC1: print-only — 2 orphans, 0 renders, exit non-zero
// ---------------------------------------------------------------------------

#[test]
fn close_print_only_no_renders() {
    let dir = TempDir::new().expect("tempdir");

    make_orphan(&dir, "a.ndjson");
    make_orphan(&dir, "b.ndjson");

    let cfg = make_cfg(dir.path());
    let args = CloseArgs {
        apply: false,
        limit: None,
        json: false,
        sessions_dir: Some(dir.path().to_path_buf()),
        render_cmd: None,
    };

    let summary = coda::run_close(&args, &cfg).expect("run_close print-only");

    // No renders because --apply was not passed.
    assert_eq!(summary.rendered, 0, "print-only: no renders");
    // remaining == total orphans (2)
    assert_eq!(summary.remaining, 2, "print-only: 2 orphans remain");

    // .summary.md files must NOT exist.
    assert!(
        !dir.path().join("a.summary.md").exists(),
        "a.summary.md must not exist after print-only"
    );
    assert!(
        !dir.path().join("b.summary.md").exists(),
        "b.summary.md must not exist after print-only"
    );

    // Exit code logic: remaining > 0 → non-zero.
    assert!(summary.remaining > 0, "remaining > 0 signals non-zero exit");
}

// ---------------------------------------------------------------------------
// AC2: --apply — renders exactly once per orphan, exits 0
// ---------------------------------------------------------------------------

#[test]
fn close_apply_renders_all_orphans() {
    let dir = TempDir::new().expect("tempdir");
    let scribe = make_fake_scribe(&dir);

    make_orphan(&dir, "x.ndjson");
    make_orphan(&dir, "y.ndjson");

    let cfg = make_cfg(dir.path());
    let args = CloseArgs {
        apply: true,
        limit: None,
        json: false,
        sessions_dir: Some(dir.path().to_path_buf()),
        render_cmd: Some(scribe.to_string_lossy().into_owned()),
    };

    let summary = coda::run_close(&args, &cfg).expect("run_close --apply");

    assert_eq!(summary.rendered, 2, "--apply: should render both orphans");
    assert_eq!(summary.failed, 0, "--apply: no failures");
    assert_eq!(summary.remaining, 0, "--apply: no orphans remain");

    // .summary.md files must exist (canonical sibling path).
    assert!(
        dir.path().join("x.summary.md").exists(),
        "x.summary.md must exist after --apply"
    );
    assert!(
        dir.path().join("y.summary.md").exists(),
        "y.summary.md must exist after --apply"
    );
}

// ---------------------------------------------------------------------------
// AC3: idempotency — after --apply, second run renders 0
// ---------------------------------------------------------------------------

#[test]
fn close_idempotent_second_run_renders_zero() {
    let dir = TempDir::new().expect("tempdir");
    let scribe = make_fake_scribe(&dir);

    make_orphan(&dir, "idem.ndjson");

    let cfg = make_cfg(dir.path());
    let args = CloseArgs {
        apply: true,
        limit: None,
        json: false,
        sessions_dir: Some(dir.path().to_path_buf()),
        render_cmd: Some(scribe.to_string_lossy().into_owned()),
    };

    // First run.
    let s1 = coda::run_close(&args, &cfg).expect("first run");
    assert_eq!(s1.rendered, 1, "first run should render 1");

    // After the fake scribe runs, idem.summary.md should exist.
    assert!(
        dir.path().join("idem.summary.md").exists(),
        "idem.summary.md must exist after first --apply"
    );

    // Second run — FsStore will now see has_summary=true for idem.ndjson.
    let s2 = coda::run_close(&args, &cfg).expect("second run");

    assert_eq!(s2.rendered, 0, "second run (idempotent): 0 renders");
    assert_eq!(s2.remaining, 0, "second run: no orphans remain");
}

// ---------------------------------------------------------------------------
// AC4: --limit 1 with 2 orphans — renders 1, remaining = 1, non-zero exit
// ---------------------------------------------------------------------------

#[test]
fn close_limit_caps_renders_and_reports_remaining() {
    let dir = TempDir::new().expect("tempdir");
    let scribe = make_fake_scribe(&dir);

    make_orphan(&dir, "p.ndjson");
    make_orphan(&dir, "q.ndjson");

    let cfg = make_cfg(dir.path());
    let args = CloseArgs {
        apply: true,
        limit: Some(1),
        json: false,
        sessions_dir: Some(dir.path().to_path_buf()),
        render_cmd: Some(scribe.to_string_lossy().into_owned()),
    };

    let summary = coda::run_close(&args, &cfg).expect("run_close --limit 1");

    assert_eq!(summary.rendered, 1, "--limit 1: only 1 render");
    assert_eq!(summary.remaining, 1, "--limit 1: 1 orphan remains");

    // Exit code logic: remaining > 0 → non-zero.
    assert!(summary.remaining > 0, "remaining > 0 signals non-zero exit");
}

// ---------------------------------------------------------------------------
// AC5 (bonus): --json emits valid JSON
// ---------------------------------------------------------------------------

#[test]
fn close_json_flag_produces_correct_summary() {
    let dir = TempDir::new().expect("tempdir");

    make_orphan(&dir, "j.ndjson");

    let cfg = make_cfg(dir.path());
    let args = CloseArgs {
        apply: false,
        limit: None,
        json: true,
        sessions_dir: Some(dir.path().to_path_buf()),
        render_cmd: None,
    };

    // run_close with --json writes to stdout; we just verify it returns Ok
    // and the summary fields are correct.
    let summary = coda::run_close(&args, &cfg).expect("run_close --json");
    assert_eq!(summary.rendered, 0);
    assert_eq!(summary.remaining, 1);
}

