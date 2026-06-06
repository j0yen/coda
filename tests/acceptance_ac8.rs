//! AC8: README documents config format, type surface, and LogStore trait.

use std::path::PathBuf;

#[test]
fn test_readme_sections_present() {
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("Could not read README.md at {}: {e}", readme_path.display()));

    let required_sections = [
        // Config format
        "grace_secs",
        "sessions_dir",
        "coda.toml",
        // Type surface
        "SessionLog",
        "SummaryState",
        "DebtClass",
        "SweepAction",
        "SweepPlan",
        "RawLog",
        // LogStore trait
        "LogStore",
        "FakeStore",
        // Usage
        "coda plan",
    ];

    let missing: Vec<&str> = required_sections
        .iter()
        .filter(|&&s| !readme.contains(s))
        .copied()
        .collect();

    assert!(
        missing.is_empty(),
        "README.md is missing required content. Missing: {missing:?}"
    );
}
