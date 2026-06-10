//! [`LogStore`] trait and its implementations.
//!
//! The [`LogStore`] trait abstracts the sessions directory so [`crate::sweep`]
//! is pure and testable. [`FsStore`] is the real filesystem implementation
//! that walks `~/.cache/ctrace/sessions/`. [`FakeStore`] is the in-memory
//! fixture store for tests.

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
/// - [`FsStore`] — real filesystem walker over `~/.cache/ctrace/sessions/`.
/// - [`FakeStore`] — in-memory fixtures for tests.
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

/// Real filesystem store: walks a sessions directory for `*.ndjson` files.
///
/// For each `.ndjson` found it:
/// - checks whether the sibling `*.summary.md` exists (`has_summary`),
/// - reads the file's `mtime` as seconds since the Unix epoch (`mtime_secs`).
///
/// One directory pass; no per-file content read.
#[derive(Debug, Clone)]
pub struct FsStore {
    /// The sessions directory to walk.
    pub sessions_dir: PathBuf,
    /// Command used to render a log.  Defaults to `"scribe"`.
    render_cmd_inner: String,
}

/// Error type for [`FsStore`] operations.
#[derive(Debug)]
pub struct FsStoreError(pub String);

impl std::fmt::Display for FsStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FsStore error: {}", self.0)
    }
}

impl std::error::Error for FsStoreError {}

impl FsStore {
    /// Create a new [`FsStore`] pointing at `sessions_dir`.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // PathBuf is not const-constructible in MSRV 1.85
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self {
            sessions_dir,
            render_cmd_inner: "scribe".to_string(),
        }
    }

    /// The render command basename (or full path).  Defaults to `"scribe"`.
    /// Override via [`FsStore::with_render_cmd`].
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn render_cmd(&self) -> &str {
        &self.render_cmd_inner
    }

    /// Return a new [`FsStore`] with a custom render command.
    #[must_use]
    pub fn with_render_cmd(mut self, cmd: impl Into<String>) -> Self {
        self.render_cmd_inner = cmd.into();
        self
    }
}

impl LogStore for FsStore {
    type Error = FsStoreError;

    fn logs(&self) -> Result<Vec<RawLog>, Self::Error> {
        if !self.sessions_dir.is_dir() {
            return Ok(vec![]);
        }
        let entries = std::fs::read_dir(&self.sessions_dir)
            .map_err(|e| FsStoreError(format!("read_dir {}: {e}", self.sessions_dir.display())))?;
        let mut logs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ndjson") {
                let mtime_secs = entry
                    .metadata()
                    .ok()
                    .and_then(|m| {
                        m.modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                    })
                    .unwrap_or(0);
                let summary_path = path.with_extension("summary.md");
                let has_summary = summary_path.exists();
                logs.push(RawLog {
                    path,
                    has_summary,
                    mtime_secs,
                });
            }
        }
        // Sort by path for deterministic output.
        logs.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(logs)
    }

    /// Shell out to `<render_cmd> <path>` and return Ok on exit 0.
    ///
    /// Expects `<render_cmd>` to write `<path>.summary.md` beside the log.
    /// Returns `Err` with the stderr output on non-zero exit.
    fn render(&self, path: &Path) -> Result<(), Self::Error> {
        let output = std::process::Command::new(&self.render_cmd_inner)
            .arg("render")
            .arg(path)
            .output()
            .map_err(|e| {
                FsStoreError(format!(
                    "failed to run '{}': {e}",
                    self.render_cmd_inner
                ))
            })?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            Err(FsStoreError(format!(
                "'{}' render failed (exit {:?}): {stderr}",
                self.render_cmd_inner,
                output.status.code()
            )))
        }
    }
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
