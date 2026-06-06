//! Active-log resolver: shells `ctrace status` to find the currently-open log.
//!
//! The active session log must never be counted as summary debt — it is still
//! being written. This module resolves it from `ctrace status` so the
//! classification is deterministic rather than heuristic (mtime-based).
//!
//! If `ctrace status` is unavailable, exits non-zero, or produces malformed
//! JSON, the resolver returns `None` without error — every closed log is then
//! eligible for classification.

use std::path::PathBuf;
use std::process::Command;

/// Parse a `ctrace status` JSON string and return the `.log` path if present.
///
/// Expected shape: `{"log": "/path/to/session.ndjson", ...}`
///
/// Returns `None` if the JSON is missing the `log` key or is malformed.
#[must_use]
pub fn parse_ctrace_status(json: &str) -> Option<PathBuf> {
    // Minimal parse: look for "log": "<path>" without pulling in serde_json
    // for a one-field extraction. We do use serde_json since it's already
    // in our dep tree.
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let log = v.get("log")?.as_str()?;
    if log.is_empty() {
        return None;
    }
    Some(PathBuf::from(log))
}

/// Resolve the active ctrace log by running `ctrace status`.
///
/// Returns `Some(path)` if ctrace is running and reported a log path,
/// `None` if ctrace is not running, unavailable, or its output cannot be
/// parsed.
///
/// # Errors
///
/// This function never returns an error — it captures failures as `None`
/// so callers can always proceed safely in cron / headless contexts.
#[must_use]
pub fn resolve_active_log() -> Option<PathBuf> {
    let output = Command::new("ctrace").arg("status").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = std::str::from_utf8(&output.stdout).ok()?;
    parse_ctrace_status(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_ctrace_status() {
        let json =
            r#"{"log": "/home/jsy/.cache/ctrace/sessions/claude-20260605T230000.ndjson", "pid": 1234}"#;
        let result = parse_ctrace_status(json);
        assert_eq!(
            result,
            Some(PathBuf::from(
                "/home/jsy/.cache/ctrace/sessions/claude-20260605T230000.ndjson"
            ))
        );
    }

    #[test]
    fn returns_none_for_malformed_json() {
        assert_eq!(parse_ctrace_status("not json at all"), None);
    }

    #[test]
    fn returns_none_for_missing_log_key() {
        let json = r#"{"status": "idle", "pid": 0}"#;
        assert_eq!(parse_ctrace_status(json), None);
    }

    #[test]
    fn returns_none_for_empty_log_value() {
        let json = r#"{"log": ""}"#;
        assert_eq!(parse_ctrace_status(json), None);
    }

    #[test]
    fn returns_none_for_empty_json_object() {
        assert_eq!(parse_ctrace_status("{}"), None);
    }

    #[test]
    fn returns_none_for_null_log() {
        let json = r#"{"log": null}"#;
        assert_eq!(parse_ctrace_status(json), None);
    }
}
