//! AC4: sweep classifies all four cases correctly.

use std::path::PathBuf;

use coda::{sweep, DebtClass, RawLog, SweepAction};

fn raw(path: &str, has_summary: bool, mtime_secs: u64) -> RawLog {
    RawLog {
        path: PathBuf::from(path),
        has_summary,
        mtime_secs,
    }
}

/// Active-log path → Active + NoOp (even if summary missing).
#[test]
fn test_sweep_active_log() {
    let now = 1000u64;
    let active = PathBuf::from("/sessions/active.ndjson");
    let logs = vec![raw("/sessions/active.ndjson", false, now - 10)];
    let plan = sweep(&logs, Some(&active), now, 120);

    assert_eq!(plan.total, 1);
    let log = &plan.logs[0];
    let action = &plan.actions[0];
    assert!(log.is_active, "log must be marked active");
    assert!(
        matches!(log.debt, DebtClass::Active),
        "debt must be Active, got: {:?}",
        log.debt
    );
    assert!(
        matches!(action, SweepAction::NoOp { .. }),
        "action must be NoOp, got: {action:?}"
    );
}

/// Missing summary, younger than grace → Fresh + Skip.
#[test]
fn test_sweep_fresh_log() {
    let now = 1000u64;
    let logs = vec![raw("/sessions/fresh.ndjson", false, now - 60)];
    let plan = sweep(&logs, None, now, 120);

    let log = &plan.logs[0];
    assert!(
        matches!(log.debt, DebtClass::Fresh { age_secs: 60 }),
        "debt must be Fresh(60), got: {:?}",
        log.debt
    );
    assert!(
        matches!(&plan.actions[0], SweepAction::Skip { .. }),
        "action must be Skip"
    );
    assert_eq!(plan.fresh, 1);
    assert_eq!(plan.orphaned, 0);
}

/// Missing summary, older than grace → Orphaned + Render.
#[test]
fn test_sweep_orphaned_log() {
    let now = 1000u64;
    let logs = vec![raw("/sessions/orphaned.ndjson", false, now - 300)];
    let plan = sweep(&logs, None, now, 120);

    let log = &plan.logs[0];
    assert!(
        matches!(log.debt, DebtClass::Orphaned { age_secs: 300 }),
        "debt must be Orphaned(300), got: {:?}",
        log.debt
    );
    let action = &plan.actions[0];
    assert!(
        matches!(action, SweepAction::Render { path } if path == &PathBuf::from("/sessions/orphaned.ndjson")),
        "action must be Render with correct path, got: {action:?}"
    );
    assert_eq!(plan.orphaned, 1);
}

/// Log with summary → Settled + NoOp.
#[test]
fn test_sweep_settled_log() {
    let now = 1000u64;
    let logs = vec![raw("/sessions/settled.ndjson", true, now - 500)];
    let plan = sweep(&logs, None, now, 120);

    let log = &plan.logs[0];
    assert!(
        matches!(log.debt, DebtClass::Settled),
        "debt must be Settled, got: {:?}",
        log.debt
    );
    assert!(
        matches!(&plan.actions[0], SweepAction::NoOp { .. }),
        "action must be NoOp"
    );
    assert_eq!(plan.settled, 1);
    assert_eq!(plan.orphaned, 0);
}

/// All four classes in one sweep.
#[test]
fn test_sweep_classification_all_cases() {
    let now = 2000u64;
    let active = PathBuf::from("/sessions/active.ndjson");
    let logs = vec![
        raw("/sessions/active.ndjson", false, now - 5),   // Active
        raw("/sessions/fresh.ndjson", false, now - 60),   // Fresh
        raw("/sessions/orphaned.ndjson", false, now - 300), // Orphaned
        raw("/sessions/settled.ndjson", true, now - 1000), // Settled
    ];
    let plan = sweep(&logs, Some(&active), now, 120);

    assert_eq!(plan.total, 4);
    assert_eq!(plan.orphaned, 1);
    assert_eq!(plan.fresh, 1);
    assert_eq!(plan.settled, 1);

    // Active entry
    assert!(matches!(plan.logs[0].debt, DebtClass::Active));
    assert!(matches!(plan.actions[0], SweepAction::NoOp { .. }));

    // Fresh entry
    assert!(matches!(plan.logs[1].debt, DebtClass::Fresh { .. }));
    assert!(matches!(plan.actions[1], SweepAction::Skip { .. }));

    // Orphaned entry
    assert!(matches!(plan.logs[2].debt, DebtClass::Orphaned { .. }));
    assert!(matches!(plan.actions[2], SweepAction::Render { .. }));

    // Settled entry
    assert!(matches!(plan.logs[3].debt, DebtClass::Settled));
    assert!(matches!(plan.actions[3], SweepAction::NoOp { .. }));
}
