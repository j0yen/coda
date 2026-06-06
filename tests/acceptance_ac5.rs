//! AC5: coda plan --format json emits one entry per log plus SweepPlan tallies.

use std::path::PathBuf;
use std::process::Command;

fn coda_bin() -> PathBuf {
    // Use the built binary from the cargo test environment.
    let mut p = std::env::current_exe().expect("current_exe");
    // Walk up to find the target directory.
    loop {
        if p.file_name().map_or(false, |n| n == "target") {
            break;
        }
        if !p.pop() {
            panic!("could not find target/ directory from {}", std::env::current_exe().unwrap().display());
        }
    }
    p.join("debug").join("coda")
}

#[test]
fn test_plan_json_output() {
    // Use a temp sessions dir with some fixture files.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let sessions = tmp.path().join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions dir");

    // Create one orphaned log (old mtime) and one with a summary.
    let old_log = sessions.join("old.ndjson");
    let settled_log = sessions.join("settled.ndjson");
    let settled_summary = sessions.join("settled.summary.md");

    std::fs::write(&old_log, b"{}").expect("write old log");
    std::fs::write(&settled_log, b"{}").expect("write settled log");
    std::fs::write(&settled_summary, b"# summary").expect("write summary");

    // Set old_log's mtime to way in the past.
    let past = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_secs(1_000_000); // very old
    filetime::set_file_mtime(&old_log, filetime::FileTime::from_system_time(past))
        .unwrap_or(()); // best-effort; may not work in all envs

    let output = Command::new(coda_bin())
        .args([
            "plan",
            "--format", "json",
            "--sessions-dir", sessions.to_str().expect("utf8"),
            "--grace-secs", "120",
        ])
        .output()
        .expect("run coda");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Must be valid JSON.
    let plan: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("output is not valid JSON: {e}\nstdout: {stdout}"));

    // Must have the SweepPlan shape.
    assert!(plan.get("total").is_some(), "missing 'total' field");
    assert!(plan.get("orphaned").is_some(), "missing 'orphaned' field");
    assert!(plan.get("fresh").is_some(), "missing 'fresh' field");
    assert!(plan.get("settled").is_some(), "missing 'settled' field");
    assert!(plan.get("actions").is_some(), "missing 'actions' field");
    assert!(plan.get("logs").is_some(), "missing 'logs' field");

    let total = plan["total"].as_u64().expect("total is u64");
    assert_eq!(total, 2, "expected 2 total logs");

    let actions = plan["actions"].as_array().expect("actions is array");
    assert_eq!(actions.len(), 2, "actions length must match total");
}
