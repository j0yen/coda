//! coda — summary-debt model and sweep planner for ctrace session logs.
//!
//! ## Overview
//!
//! ctrace records every Claude session to `~/.cache/ctrace/sessions/<id>.ndjson`.
//! When the session ends cleanly, the `SessionEnd` hook renders a
//! `<id>.summary.md` alongside the log. Headless timer ticks are `SIGKILLed`
//! by cgroup teardown before that hook fires, leaving the log permanently
//! un-summarized. As of 2026-06-05 that's 623 out of 1874 logs (33%).
//!
//! This crate provides:
//!
//! - **Types** — [`SessionLog`], [`SummaryState`], [`DebtClass`],
//!   [`SweepAction`], [`SweepPlan`], [`RawLog`].
//! - **[`LogStore`] trait** — abstracts the sessions directory so
//!   [`sweep`] is pure and testable, and so sibling crates share one contract.
//! - **[`FsStore`]** — real filesystem store over `~/.cache/ctrace/sessions/`.
//! - **[`FakeStore`]** — in-memory fixture store for tests.
//! - **[`sweep`]** — pure function that classifies a `&[RawLog]` into a
//!   [`SweepPlan`]. Zero side effects; no filesystem or network access.
//! - **[`CodaConfig`]** — loads `~/.config/coda/coda.toml` with sane defaults.
//! - **[`resolver`]** — active-log resolver: shells `ctrace status` to find the
//!   currently-open log so it is excluded from debt classification.

use std::path::PathBuf;

pub mod config;
pub mod resolver;
pub mod store;
pub mod sweep;

pub use config::CodaConfig;
pub use store::{FakeStore, FsStore, FsStoreError, LogStore, RawLog};
pub use sweep::{sweep, DebtClass, SessionLog, SummaryState, SweepAction, SweepPlan};

/// Default grace period in seconds: 120s (2 minutes).
///
/// A missing-summary log younger than this is considered [`DebtClass::Fresh`] —
/// the `SessionEnd` hook may still fire. Older than this is [`DebtClass::Orphaned`].
pub const DEFAULT_GRACE_SECS: u64 = 120;

/// Default sessions directory.
pub const DEFAULT_SESSIONS_DIR: &str = "/home/jsy/.cache/ctrace/sessions";

// Re-export for convenience in tests
pub use std::path::PathBuf as _PathBuf;

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()))
}

/// Resolve `~` prefix to the actual home directory.
#[must_use]
pub fn expand_tilde(path: &str) -> PathBuf {
    path.strip_prefix("~/")
        .map_or_else(
            || {
                if path == "~" {
                    home_dir()
                } else {
                    PathBuf::from(path)
                }
            },
            |rest| home_dir().join(rest),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_home() {
        let expanded = expand_tilde("~/foo/bar");
        assert!(expanded.to_str().map_or(false, |s| s.ends_with("/foo/bar")));
        assert!(!expanded.to_str().map_or(true, |s| s.starts_with('~')));
    }
}
