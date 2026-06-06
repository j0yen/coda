//! Configuration loader for coda.
//!
//! Reads `~/.config/coda/coda.toml` (or a path you supply) and merges with
//! built-in defaults. Loading is **pure** — no filesystem scan, no `scribe`
//! invocation.
//!
//! ## Config format
//!
//! ```toml
//! # ~/.config/coda/coda.toml
//! grace_secs = 120        # logs younger than this (seconds) are Fresh, not Orphaned
//! sessions_dir = "/home/jsy/.cache/ctrace/sessions"
//! ```
//!
//! Both keys are optional; absent file yields the same defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_GRACE_SECS, DEFAULT_SESSIONS_DIR};

/// Coda configuration (merged from file + defaults).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodaConfig {
    /// Grace period in seconds. Missing-summary logs younger than this are
    /// [`DebtClass::Fresh`]; older are [`DebtClass::Orphaned`].
    pub grace_secs: u64,
    /// Path to the ctrace sessions directory.
    pub sessions_dir: PathBuf,
}

/// Raw TOML representation (all fields optional for partial overrides).
#[derive(Debug, Deserialize)]
struct RawConfig {
    grace_secs: Option<u64>,
    sessions_dir: Option<String>,
}

impl Default for CodaConfig {
    fn default() -> Self {
        Self {
            grace_secs: DEFAULT_GRACE_SECS,
            sessions_dir: crate::expand_tilde(DEFAULT_SESSIONS_DIR),
        }
    }
}

impl CodaConfig {
    /// Load configuration from `path`.
    ///
    /// If the file does not exist, returns [`CodaConfig::default`].
    /// If the file exists but is malformed, returns an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed as TOML.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let text =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e, path.to_path_buf()))?;
        let raw: RawConfig =
            toml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string()))?;

        let defaults = Self::default();
        Ok(Self {
            grace_secs: raw.grace_secs.unwrap_or(defaults.grace_secs),
            sessions_dir: raw
                .sessions_dir
                .map(|s| crate::expand_tilde(&s))
                .unwrap_or(defaults.sessions_dir),
        })
    }

    /// Load from the default config path `~/.config/coda/coda.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load_default() -> Result<Self, ConfigError> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let path = PathBuf::from(home).join(".config/coda/coda.toml");
        Self::load(&path)
    }
}

/// Errors that can occur while loading config.
#[derive(Debug)]
pub enum ConfigError {
    /// I/O error reading the config file.
    Io(std::io::Error, PathBuf),
    /// TOML parse error.
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e, path) => write!(f, "failed to read {}: {e}", path.display()),
            Self::Parse(msg) => write!(f, "config parse error: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = CodaConfig::default();
        assert_eq!(cfg.grace_secs, 120);
        assert!(cfg
            .sessions_dir
            .to_str()
            .map_or(false, |s| s.contains("ctrace/sessions")));
    }

    #[test]
    fn absent_file_yields_defaults() {
        let tmp = std::env::temp_dir().join("coda_nonexistent_config_12345.toml");
        let cfg = CodaConfig::load(&tmp).expect("absent file should yield defaults");
        assert_eq!(cfg, CodaConfig::default());
    }

    #[test]
    fn partial_override_merges_with_defaults() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        writeln!(f, "grace_secs = 300").expect("write");
        let cfg = CodaConfig::load(f.path()).expect("load partial config");
        assert_eq!(cfg.grace_secs, 300);
        // sessions_dir falls back to default
        assert_eq!(cfg.sessions_dir, CodaConfig::default().sessions_dir);
    }
}
