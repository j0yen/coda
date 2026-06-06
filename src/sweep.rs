//! Pure sweep function and its associated types.
//!
//! [`sweep`] is the heart of coda: given a slice of [`RawLog`] observations,
//! the path of the currently-active ctrace log, the current unix timestamp,
//! and a grace period in seconds, it returns a [`SweepPlan`] classifying
//! every log with zero side effects.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store::RawLog;

/// Whether a `.summary.md` file exists beside the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryState {
    /// A `.summary.md` file exists beside the `.ndjson`.
    Present,
    /// No `.summary.md` file found beside the `.ndjson`.
    Missing,
}

/// Debt classification for a single log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum DebtClass {
    /// The live ctrace log currently being written. Never touch.
    Active,
    /// Summary missing but log is younger than the grace period.
    /// The `SessionEnd` hook may still fire.
    Fresh {
        /// Age of the log in seconds at classification time.
        age_secs: u64,
    },
    /// Summary missing and older than the grace period — real debt.
    Orphaned {
        /// Age of the log in seconds at classification time.
        age_secs: u64,
    },
    /// A `.summary.md` already exists — nothing to do.
    Settled,
}

/// One log as seen through the classification model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLog {
    /// Path to the `.ndjson` log file.
    pub path: PathBuf,
    /// Whether a summary exists.
    pub summary: SummaryState,
    /// Age of the log at classification time (seconds since last write).
    pub age_secs: u64,
    /// True if this is the currently-active ctrace log.
    pub is_active: bool,
    /// The debt classification.
    pub debt: DebtClass,
}

/// What [`sweep`] recommends doing with a single log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SweepAction {
    /// Render this log's summary (coda-close will execute this).
    Render {
        /// Path to the log to render.
        path: PathBuf,
    },
    /// Skip this log for now.
    Skip {
        /// Path to the skipped log.
        path: PathBuf,
        /// Human-readable reason for skipping.
        reason: String,
    },
    /// Nothing to do.
    NoOp {
        /// Path to the log.
        path: PathBuf,
    },
}

/// The result of [`sweep`]: a declarative plan with tallies.
///
/// `Render` actions are what coda-close will execute. This type carries
/// no side effects — it is purely a description of what should be done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepPlan {
    /// Per-log actions, one per input log.
    pub actions: Vec<SweepAction>,
    /// Classified [`SessionLog`]s, parallel to `actions`.
    pub logs: Vec<SessionLog>,
    /// Number of logs classified as [`DebtClass::Orphaned`].
    pub orphaned: usize,
    /// Number of logs classified as [`DebtClass::Fresh`].
    pub fresh: usize,
    /// Number of logs classified as [`DebtClass::Settled`].
    pub settled: usize,
    /// Total number of logs classified.
    pub total: usize,
}

/// Classify a slice of raw log observations into a [`SweepPlan`].
///
/// # Arguments
///
/// * `logs` — raw observations from the store (path, `has_summary`, `mtime_secs`).
/// * `active_log` — path of the log ctrace is currently writing, or `None`.
/// * `now_secs` — current unix timestamp (injected for testability).
/// * `grace_secs` — missing-summary logs younger than this are `Fresh`.
///
/// # Panics
///
/// Never panics. If `mtime_secs > now_secs` (clock skew / future mtime),
/// the age is treated as 0.
#[must_use]
pub fn sweep(
    logs: &[RawLog],
    active_log: Option<&Path>,
    now_secs: u64,
    grace_secs: u64,
) -> SweepPlan {
    let mut actions = Vec::with_capacity(logs.len());
    let mut session_logs = Vec::with_capacity(logs.len());
    let mut orphaned = 0usize;
    let mut fresh = 0usize;
    let mut settled = 0usize;

    for raw in logs {
        let age_secs = now_secs.saturating_sub(raw.mtime_secs);
        let is_active = active_log.is_some_and(|a| a == raw.path);

        let summary = if raw.has_summary {
            SummaryState::Present
        } else {
            SummaryState::Missing
        };

        let debt = classify(is_active, &summary, age_secs, grace_secs);

        let action = match &debt {
            DebtClass::Active => SweepAction::NoOp {
                path: raw.path.clone(),
            },
            DebtClass::Fresh { .. } => SweepAction::Skip {
                path: raw.path.clone(),
                reason: "session may still be active or hook may still fire".to_string(),
            },
            DebtClass::Orphaned { .. } => {
                orphaned += 1;
                SweepAction::Render {
                    path: raw.path.clone(),
                }
            }
            DebtClass::Settled => {
                settled += 1;
                SweepAction::NoOp {
                    path: raw.path.clone(),
                }
            }
        };

        if matches!(debt, DebtClass::Fresh { .. }) {
            fresh += 1;
        }

        session_logs.push(SessionLog {
            path: raw.path.clone(),
            summary,
            age_secs,
            is_active,
            debt,
        });
        actions.push(action);
    }

    let total = logs.len();

    SweepPlan {
        actions,
        logs: session_logs,
        orphaned,
        fresh,
        settled,
        total,
    }
}

/// Classify a single log into a [`DebtClass`].
const fn classify(
    is_active: bool,
    summary: &SummaryState,
    age_secs: u64,
    grace_secs: u64,
) -> DebtClass {
    if is_active {
        return DebtClass::Active;
    }
    match summary {
        SummaryState::Present => DebtClass::Settled,
        SummaryState::Missing => {
            if age_secs < grace_secs {
                DebtClass::Fresh { age_secs }
            } else {
                DebtClass::Orphaned { age_secs }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RawLog;

    fn make_raw(path: &str, has_summary: bool, mtime_secs: u64) -> RawLog {
        RawLog {
            path: PathBuf::from(path),
            has_summary,
            mtime_secs,
        }
    }

    #[test]
    fn active_log_is_noop_even_without_summary() {
        let now = 1000u64;
        let active = PathBuf::from("/sessions/active.ndjson");
        let logs = vec![make_raw("/sessions/active.ndjson", false, now - 50)];
        let plan = sweep(&logs, Some(&active), now, 120);
        assert_eq!(plan.total, 1);
        assert_eq!(plan.orphaned, 0);
        assert!(matches!(plan.logs[0].debt, DebtClass::Active));
        assert!(matches!(plan.actions[0], SweepAction::NoOp { .. }));
    }

    #[test]
    fn fresh_log_is_skipped() {
        let now = 1000u64;
        let logs = vec![make_raw("/sessions/new.ndjson", false, now - 60)];
        let plan = sweep(&logs, None, now, 120);
        assert_eq!(plan.fresh, 1);
        assert_eq!(plan.orphaned, 0);
        assert!(matches!(plan.logs[0].debt, DebtClass::Fresh { age_secs: 60 }));
        assert!(matches!(plan.actions[0], SweepAction::Skip { .. }));
    }

    #[test]
    fn orphaned_log_is_rendered() {
        let now = 1000u64;
        let logs = vec![make_raw("/sessions/old.ndjson", false, now - 300)];
        let plan = sweep(&logs, None, now, 120);
        assert_eq!(plan.orphaned, 1);
        assert!(matches!(plan.logs[0].debt, DebtClass::Orphaned { age_secs: 300 }));
        assert!(matches!(plan.actions[0], SweepAction::Render { .. }));
    }

    #[test]
    fn settled_log_is_noop() {
        let now = 1000u64;
        let logs = vec![make_raw("/sessions/done.ndjson", true, now - 500)];
        let plan = sweep(&logs, None, now, 120);
        assert_eq!(plan.settled, 1);
        assert_eq!(plan.orphaned, 0);
        assert!(matches!(plan.logs[0].debt, DebtClass::Settled));
        assert!(matches!(plan.actions[0], SweepAction::NoOp { .. }));
    }

    #[test]
    fn grace_boundary_exactly_at_grace_is_orphaned() {
        // age == grace → Orphaned (strict less-than for Fresh)
        let now = 1000u64;
        let grace = 120u64;
        let logs = vec![make_raw("/sessions/boundary.ndjson", false, now - grace)];
        let plan = sweep(&logs, None, now, grace);
        assert!(matches!(plan.logs[0].debt, DebtClass::Orphaned { .. }));
    }

    #[test]
    fn future_mtime_age_is_zero() {
        // mtime in the future → saturating_sub → age 0 → Fresh
        let now = 1000u64;
        let logs = vec![make_raw("/sessions/future.ndjson", false, now + 100)];
        let plan = sweep(&logs, None, now, 120);
        assert!(matches!(plan.logs[0].debt, DebtClass::Fresh { age_secs: 0 }));
    }

    #[test]
    fn empty_logs_returns_empty_plan() {
        let plan = sweep(&[], None, 1000, 120);
        assert_eq!(plan.total, 0);
        assert_eq!(plan.orphaned, 0);
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn tallies_are_correct_for_mixed_set() {
        let now = 2000u64;
        let active = PathBuf::from("/sessions/a.ndjson");
        let logs = vec![
            make_raw("/sessions/a.ndjson", false, now - 10), // active
            make_raw("/sessions/b.ndjson", false, now - 60), // fresh
            make_raw("/sessions/c.ndjson", false, now - 300), // orphaned
            make_raw("/sessions/d.ndjson", true, now - 500),  // settled
        ];
        let plan = sweep(&logs, Some(&active), now, 120);
        assert_eq!(plan.total, 4);
        assert_eq!(plan.orphaned, 1);
        assert_eq!(plan.fresh, 1);
        assert_eq!(plan.settled, 1);
    }
}
