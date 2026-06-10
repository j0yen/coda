# Changelog

## v0.3.0 — 2026-06-10

coda-close + coda-boot: idempotent actuator and timer/hook installer

### coda-close (`coda close`)

Adds the `close` subcommand: drives `sweep` to identify `Render` candidates
then shells `scribe render <path>` for each orphaned log.

- **Default (print-only):** lists orphans + "would render N, M already settled"; exits non-zero if ≥1 orphan remains
- **`--apply`:** renders each orphan; per-log failures are counted but don't abort; exits 0 if all rendered
- **`--limit N`:** caps renders per invocation; prints "capped at N, P orphans remain" when work is dropped
- **`--json`:** emits `{"rendered": N, "skipped": M, "failed": K, "remaining": P}`
- **`--sessions-dir <path>`:** overrides config sessions dir
- `FsStore::render` now shells `<render_cmd> render <path>` (configurable for tests via `with_render_cmd`)
- 5 integration tests in `tests/close.rs` covering all 4 ACs + JSON flag

### coda-boot (`coda boot install`)

Adds the `boot` subcommand for one-time timer and hook setup.

- Prints the exact `settings.json` `SessionStart` hook JSON entry to stdout
- **`--enable`:** copies `install/coda-close.{service,timer}` to `~/.config/systemd/user/` and runs `systemctl --user daemon-reload`
- Always prints (never runs): `systemctl --user enable --now coda-close.timer`
- Ships `install/coda-close.service`, `install/coda-close.timer`, `install/coda-session-start.sh` in the repo

---

## v0.2.0 — 2026-06-06

coda-audit: live read of summary debt

Adds FsStore (real LogStore over ~/.cache/ctrace/sessions/), active-log
resolver (ctrace status JSON parser), and `coda audit` command with
--format table|json, --verbose, --orphaned-only flags. 16 integration
tests; all 8 ACs green; strictly read-only (render call count = 0).

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Initial scaffold: `SessionLog`, `SummaryState`, `DebtClass`, `SweepAction`, `SweepPlan`, `RawLog` types (all `pub`, serde-serializable)
- `LogStore` trait with `FakeStore` in-memory implementation
- Pure `sweep()` function classifying `&[RawLog]` into `SweepPlan` with zero side effects
- `CodaConfig::load` reading `~/.config/coda/coda.toml` with graceful defaults
- `coda plan` subcommand: table and `--format json` output
- Exit code 1 when any log is `Orphaned`, 0 otherwise
- `sigpipe::reset()` at main entry — no panic on `coda plan | head`
- All 8 acceptance criteria green (iter-0)
