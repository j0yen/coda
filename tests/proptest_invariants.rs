//! Property-based invariants for the sweep function.
//! READ-ONLY after scaffold.

use std::path::PathBuf;

use coda::{sweep, DebtClass, RawLog, SweepAction};
use proptest::prelude::*;

fn arb_raw_log(base_path: &str, has_summary: bool, mtime: u64) -> RawLog {
    RawLog {
        path: PathBuf::from(base_path),
        has_summary,
        mtime_secs: mtime,
    }
}

proptest! {
    /// Total count always equals input length.
    #[test]
    fn total_equals_input_length(
        n in 0usize..20usize,
        now in 1000u64..100_000u64,
        grace in 1u64..300u64,
    ) {
        let logs: Vec<RawLog> = (0..n)
            .map(|i| arb_raw_log(&format!("/s/{i}.ndjson"), i % 2 == 0, now.saturating_sub(i as u64 * 50)))
            .collect();
        let plan = sweep(&logs, None, now, grace);
        prop_assert_eq!(plan.total, n);
        prop_assert_eq!(plan.actions.len(), n);
        prop_assert_eq!(plan.logs.len(), n);
    }

    /// orphaned + fresh + settled + active_count == total.
    #[test]
    fn tallies_are_consistent(
        n in 0usize..20usize,
        now in 1000u64..100_000u64,
        grace in 1u64..300u64,
    ) {
        let logs: Vec<RawLog> = (0..n)
            .map(|i| arb_raw_log(&format!("/s/{i}.ndjson"), i % 3 == 0, now.saturating_sub(i as u64 * 60)))
            .collect();
        let plan = sweep(&logs, None, now, grace);
        let active_count = plan.logs.iter().filter(|l| matches!(l.debt, DebtClass::Active)).count();
        prop_assert_eq!(
            plan.orphaned + plan.fresh + plan.settled + active_count,
            plan.total
        );
    }

    /// Orphaned logs always get a Render action.
    #[test]
    fn orphaned_logs_always_render(
        n in 1usize..10usize,
        now in 1000u64..100_000u64,
        grace in 1u64..300u64,
    ) {
        let logs: Vec<RawLog> = (0..n)
            .map(|i| arb_raw_log(&format!("/s/{i}.ndjson"), false, now.saturating_sub(grace + i as u64 + 1)))
            .collect();
        let plan = sweep(&logs, None, now, grace);
        for (log, action) in plan.logs.iter().zip(plan.actions.iter()) {
            if matches!(log.debt, DebtClass::Orphaned { .. }) {
                prop_assert!(matches!(action, SweepAction::Render { .. }),
                    "Orphaned log must get Render, got: {action:?}");
            }
        }
    }

    /// A log with a summary is never Orphaned.
    #[test]
    fn settled_logs_are_never_orphaned(
        n in 1usize..10usize,
        now in 1000u64..100_000u64,
        grace in 1u64..300u64,
        age in 0u64..1_000_000u64,
    ) {
        let logs: Vec<RawLog> = (0..n)
            .map(|i| arb_raw_log(&format!("/s/{i}.ndjson"), true, now.saturating_sub(age + i as u64)))
            .collect();
        let plan = sweep(&logs, None, now, grace);
        for log in &plan.logs {
            prop_assert!(!matches!(log.debt, DebtClass::Orphaned { .. }),
                "log with summary must not be Orphaned, got: {:?}", log.debt);
        }
    }
}
