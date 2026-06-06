//! AC7: coda plan | head -1 does not panic (SIGPIPE reset verified).
//!
//! `sigpipe::reset()` restores SIGPIPE to `SIG_DFL`, which means:
//! - When the pipe is closed mid-write, the OS sends SIGPIPE.
//! - The process exits cleanly via signal (no Rust panic/unwind).
//!
//! The test verifies the binary is NOT panicking (no panic output on stderr,
//! no abort/SIGABRT). Being terminated by SIGPIPE (signal 13 / exit code 141)
//! is the *expected* clean behavior — it is distinct from panic.

use std::path::PathBuf;
use std::process::{Command, Stdio};

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

/// Simulates `coda plan | head -1`:
/// - spawns coda with stdout piped
/// - reads exactly one line, then drops the pipe (closes read end)
/// - coda must NOT panic (no "panicked at" on stderr, no SIGABRT)
#[test]
fn test_sigpipe_no_panic() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let sessions = tmp.path().join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions");

    // Create several orphaned logs so there's output to write.
    for i in 0..20 {
        let log = sessions.join(format!("log_{i:03}.ndjson"));
        std::fs::write(&log, b"{}").expect("write log");
        let past = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        filetime::set_file_mtime(&log, filetime::FileTime::from_system_time(past))
            .unwrap_or(());
    }

    let mut child = Command::new(coda_bin())
        .args([
            "plan",
            "--sessions-dir",
            sessions.to_str().expect("utf8"),
            "--grace-secs",
            "120",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn coda");

    // Read only the first line then drop the pipe.
    {
        let stdout = child.stdout.take().expect("piped stdout");
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        let _ = std::io::BufRead::read_line(&mut reader, &mut line);
        // reader (and the pipe) drops here → SIGPIPE sent to coda
    }

    let output = child.wait_with_output().expect("wait on child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The process must NOT have panicked.
    // A panic writes "thread 'main' panicked at ..." to stderr.
    assert!(
        !stderr.contains("panicked at"),
        "coda panicked on SIGPIPE. stderr: {stderr}"
    );

    // Must not be killed by SIGABRT (signal 6) which happens on double-panic or abort().
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert!(
            output.status.signal().map_or(true, |sig| sig != 6),
            "coda aborted (SIGABRT / signal 6) — this indicates a panic or abort, not clean SIGPIPE handling"
        );
    }

    // Being terminated by SIGPIPE (signal 13) or exiting with code 1 (orphaned logs)
    // are both acceptable outcomes — the key is no panic.
}
