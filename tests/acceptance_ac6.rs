//! AC6: coda plan exits non-zero when >=1 log is Orphaned, zero when none.

use std::path::PathBuf;
use std::process::Command;

fn coda_bin() -> PathBuf {
    let mut p = std::env::current_exe().expect("current_exe");
    loop {
        if p.file_name().map_or(false, |n| n == "target") {
            break;
        }
        if !p.pop() {
            panic!("could not find target/ directory");
        }
    }
    p.join("debug").join("coda")
}

/// When no sessions directory exists (empty plan) → exit 0.
#[test]
fn test_exit_code_clean() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let sessions = tmp.path().join("empty_sessions");
    // Don't create the dir — load_logs returns [] for absent dir.

    let status = Command::new(coda_bin())
        .args([
            "plan",
            "--sessions-dir",
            sessions.to_str().expect("utf8"),
            "--grace-secs",
            "120",
        ])
        .status()
        .expect("run coda");

    assert!(
        status.success(),
        "exit code must be 0 with no orphaned logs; got: {status}"
    );
}

/// When sessions dir has only a settled log → exit 0.
#[test]
fn test_exit_code_all_settled() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let sessions = tmp.path().join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions");

    let log = sessions.join("done.ndjson");
    let summary = sessions.join("done.summary.md");
    std::fs::write(&log, b"{}").expect("write log");
    std::fs::write(&summary, b"# summary").expect("write summary");

    let status = Command::new(coda_bin())
        .args([
            "plan",
            "--sessions-dir",
            sessions.to_str().expect("utf8"),
            "--grace-secs",
            "120",
        ])
        .status()
        .expect("run coda");

    assert!(status.success(), "settled-only plan must exit 0");
}

/// When sessions dir has an orphaned log → exit non-zero.
#[test]
fn test_exit_code_orphaned() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let sessions = tmp.path().join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions");

    let log = sessions.join("orphaned.ndjson");
    std::fs::write(&log, b"{}").expect("write log");

    // Set mtime to the past so it's older than grace.
    let past = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    filetime::set_file_mtime(&log, filetime::FileTime::from_system_time(past))
        .unwrap_or(());

    let status = Command::new(coda_bin())
        .args([
            "plan",
            "--sessions-dir",
            sessions.to_str().expect("utf8"),
            "--grace-secs",
            "120",
        ])
        .status()
        .expect("run coda");

    assert!(
        !status.success(),
        "orphaned log must produce exit code 1; got: {status}"
    );
}
