# Changelog

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
