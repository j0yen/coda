//! `coda boot` subcommand — systemd timer and SessionStart hook installer.
//!
//! ## Subcommands
//!
//! ```text
//! coda boot install [--enable]
//! ```
//!
//! `install` (without `--enable`): prints the settings.json hook entry to
//! stdout so the user can paste it manually.  Never modifies any file.
//!
//! `install --enable`: additionally copies `install/*.{service,timer}` to
//! `~/.config/systemd/user/` and runs `systemctl --user daemon-reload`.
//! Always prints (never runs) the `enable --now` command.

use std::io::Write as _;
use std::path::PathBuf;

/// The SessionStart hook JSON snippet printed by `coda boot install`.
const HOOK_JSON: &str = r#"{"SessionStart": ["/bin/bash", "-c", "/home/jsy/.local/bin/coda close --apply --limit 50 >/dev/null 2>&1 &"]}"#;

/// The `enable --now` command we always print but never execute.
const ENABLE_CMD: &str = "systemctl --user enable --now coda-close.timer";

/// CLI arguments for `coda boot install`.
#[derive(Debug, Clone)]
pub struct BootInstallArgs {
    /// If true, copy unit files and run daemon-reload.
    pub enable: bool,
}

/// Run `coda boot install`.
///
/// # Errors
///
/// Returns an error string if unit-file copy or `daemon-reload` fails
/// (only when `--enable` is passed).
#[allow(clippy::print_stdout)]
pub fn run_boot_install(args: &BootInstallArgs) -> Result<(), String> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // Always print the settings.json hook entry.
    writeln!(out, "Add this entry to your ~/.claude/settings.json hooks:").ok();
    writeln!(out, "{HOOK_JSON}").ok();
    writeln!(out).ok();

    if args.enable {
        install_unit_files()?;
        writeln!(out, "Unit files installed. Run:").ok();
    } else {
        writeln!(out, "To install systemd units, re-run with --enable. Then run:").ok();
    }

    // Always print (never run) the enable command.
    writeln!(out, "  run: {ENABLE_CMD}").ok();

    Ok(())
}

/// Copy the bundled install/*.{service,timer} to `~/.config/systemd/user/`
/// and run `systemctl --user daemon-reload`.
fn install_unit_files() -> Result<(), String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let dest_dir = PathBuf::from(&home).join(".config/systemd/user");

    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("failed to create {}: {e}", dest_dir.display()))?;

    // Locate the install/ directory relative to the manifest (dev) or the
    // binary's sibling directory (installed).
    let install_dir = locate_install_dir()?;

    for unit in &["coda-close.service", "coda-close.timer"] {
        let src = install_dir.join(unit);
        let dst = dest_dir.join(unit);
        std::fs::copy(&src, &dst).map_err(|e| {
            format!(
                "failed to copy {} → {}: {e}",
                src.display(),
                dst.display()
            )
        })?;
    }

    // Run daemon-reload.
    let status = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .map_err(|e| format!("failed to run systemctl: {e}"))?;

    if !status.success() {
        return Err(format!(
            "systemctl --user daemon-reload failed with {}",
            status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string())
        ));
    }

    Ok(())
}

/// Find the `install/` directory: first look relative to `CARGO_MANIFEST_DIR`
/// (dev/test builds), then relative to the binary's parent.
fn locate_install_dir() -> Result<PathBuf, String> {
    // In dev/test: CARGO_MANIFEST_DIR is set.
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(manifest).join("install");
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    // Installed: look beside the binary.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("install");
            if candidate.is_dir() {
                return Ok(candidate);
            }
            // One level up (e.g. ~/.local/bin/../share/coda/install/).
            let candidate2 = parent.join("../share/coda/install");
            if candidate2.is_dir() {
                return Ok(candidate2);
            }
        }
    }

    Err("cannot locate install/ directory; try running from the coda source tree".to_string())
}
