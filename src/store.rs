//! [`LogStore`] trait and its implementations.
//!
//! The [`LogStore`] trait abstracts the sessions directory so [`crate::sweep`]
//! is pure and testable. The real [`FsStore`] that reads from
//! `~/.cache/ctrace/sessions/` is in coda-audit; this module ships only the
//! trait and [`FakeStore`] for tests.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Raw observation from the store for a single `.ndjson` log file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawLog {
    /// Absolute path to the `.ndjson` log file.
    pub path: PathBuf,
    /// True if a `.summary.md` exists beside the log.
    pub has_summary: bool,
    /// Unix timestamp (seconds) of the log's last modification.
    pub mtime_secs: u64,
}

/// Abstraction over the sessions directory.
///
/// Implementations:
/// - [`FakeStore`] — in-memory fixtures for tests (this crate).
/// - `FsStore` — real filesystem (coda-audit, future crate).
pub trait LogStore {
    /// Error type for store operations.
    type Error: std::fmt::Debug + std::fmt::Display;

    /// Return all `.ndjson` files in the sessions directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    fn logs(&self) -> Result<Vec<RawLog>, Self::Error>;

    /// Render (summarize) a log by shelling out to `scribe render`.
    ///
    /// This is unused in coda-sweep (no `--apply` flag). It exists so
    /// coda-close can implement the same [`LogStore`] contract.
    ///
    /// # Errors
    ///
    /// Returns an error if the render command fails.
    fn render(&self, path: &Path) -> Result<(), Self::Error>;
}

/// In-memory fixture store for tests.
///
/// Construct with a set of [`RawLog`] fixtures; [`LogStore::render`] is a no-op.
#[derive(Debug, Default, Clone)]
pub struct FakeStore {
    logs: Vec<RawLog>,
    /// Track how many times `render` was called (for assertions).
    pub render_calls: std::cell::Cell<usize>,
}

impl FakeStore {
    /// Create a new [`FakeStore`] from a list of raw log fixtures.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec is not const-constructible in MSRV 1.85
    pub fn new(logs: Vec<RawLog>) -> Self {
        Self {
            logs,
            render_calls: std::cell::Cell::new(0),
        }
    }

    /// Return how many times [`LogStore::render`] has been called.
    #[must_use]
    pub fn render_call_count(&self) -> usize {
        self.render_calls.get()
    }
}

impl LogStore for FakeStore {
    type Error = std::convert::Infallible;

    fn logs(&self) -> Result<Vec<RawLog>, Self::Error> {
        Ok(self.logs.clone())
    }

    fn render(&self, _path: &Path) -> Result<(), Self::Error> {
        self.render_calls.set(self.render_calls.get() + 1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_store_returns_all_logs() {
        let logs = vec![
            RawLog {
                path: PathBuf::from("/sessions/a.ndjson"),
                has_summary: true,
                mtime_secs: 1000,
            },
            RawLog {
                path: PathBuf::from("/sessions/b.ndjson"),
                has_summary: false,
                mtime_secs: 900,
            },
        ];
        let store = FakeStore::new(logs.clone());
        let got = store.logs().expect("FakeStore::logs never errors");
        assert_eq!(got, logs);
    }

    #[test]
    fn fake_store_render_tracks_calls() {
        let store = FakeStore::new(vec![]);
        let path = PathBuf::from("/sessions/x.ndjson");
        assert_eq!(store.render_call_count(), 0);
        store.render(&path).expect("FakeStore::render never errors");
        assert_eq!(store.render_call_count(), 1);
    }
}
