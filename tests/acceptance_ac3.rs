//! AC3: CodaConfig::load parses config/coda.example.toml yielding expected
//! grace_secs + sessions_dir; absent file yields documented defaults.

use coda::CodaConfig;
use std::path::PathBuf;

#[test]
fn test_config_load_example() {
    let example_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("config")
        .join("coda.example.toml");
    let cfg = CodaConfig::load(&example_path).expect("load example config");
    assert_eq!(cfg.grace_secs, 120, "example grace_secs must be 120");
    assert!(
        cfg.sessions_dir
            .to_str()
            .map_or(false, |s| s.contains("ctrace/sessions")),
        "example sessions_dir must contain 'ctrace/sessions', got: {}",
        cfg.sessions_dir.display()
    );
}

#[test]
fn test_config_defaults() {
    // Use a path that is guaranteed not to exist.
    let absent = PathBuf::from("/tmp/coda_nonexistent_config_test_98765.toml");
    assert!(!absent.exists(), "test precondition: path must not exist");
    let cfg = CodaConfig::load(&absent).expect("absent file yields defaults");
    assert_eq!(cfg.grace_secs, 120, "default grace_secs");
    assert!(
        cfg.sessions_dir
            .to_str()
            .map_or(false, |s| s.contains("ctrace/sessions")),
        "default sessions_dir must contain 'ctrace/sessions', got: {}",
        cfg.sessions_dir.display()
    );
}

#[test]
fn test_config_custom_grace() {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().expect("tempfile");
    writeln!(f, "grace_secs = 600\nsessions_dir = \"/tmp/my_sessions\"")
        .expect("write");
    let cfg = CodaConfig::load(f.path()).expect("load custom config");
    assert_eq!(cfg.grace_secs, 600);
    assert_eq!(cfg.sessions_dir, PathBuf::from("/tmp/my_sessions"));
}
