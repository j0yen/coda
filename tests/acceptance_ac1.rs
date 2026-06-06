//! AC1: cargo build and cargo test succeed offline; sweep makes zero store calls.
//!
//! The sweep function operates only on passed &[RawLog] + active-log path +
//! injected now/grace. It does NOT call any LogStore method.

use std::path::PathBuf;

use coda::{sweep, FakeStore, LogStore, RawLog};

/// Sweep does not call FakeStore::logs() — it takes pre-loaded &[RawLog].
#[test]
fn test_sweep_makes_no_store_calls() {
    // Construct a FakeStore so we can track whether any method was called.
    let store = FakeStore::new(vec![RawLog {
        path: PathBuf::from("/sessions/should_not_be_fetched.ndjson"),
        has_summary: false,
        mtime_secs: 0,
    }]);

    // Manually fetch logs (as a caller would), then pass the slice to sweep.
    let raw_logs = store.logs().expect("FakeStore infallible");
    let initial_render_calls = store.render_call_count();

    // Call sweep with the pre-fetched data.
    let active = PathBuf::from("/sessions/active.ndjson");
    let plan = sweep(&raw_logs, Some(&active), 1000, 120);

    // sweep must not have triggered any store calls — render_call_count unchanged.
    assert_eq!(
        store.render_call_count(),
        initial_render_calls,
        "sweep must not call LogStore::render"
    );

    // Plan should reflect the one log.
    assert_eq!(plan.total, 1);
}

/// Additional check: sweep is a pure function — same input always yields same output.
#[test]
fn test_sweep_is_pure() {
    let logs = vec![RawLog {
        path: PathBuf::from("/sessions/a.ndjson"),
        has_summary: false,
        mtime_secs: 500,
    }];
    let plan1 = sweep(&logs, None, 1000, 120);
    let plan2 = sweep(&logs, None, 1000, 120);
    assert_eq!(plan1, plan2, "sweep is deterministic for same inputs");
}
