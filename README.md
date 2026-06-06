# coda

Summary-debt model and sweep planner for `ctrace` session logs.

## Why this exists

`ctrace` records every Claude session to `~/.cache/ctrace/sessions/<id>.ndjson`.
When a session ends cleanly, the `SessionEnd` hook renders a `.summary.md`
alongside the log. Headless timer ticks are `SIGKILLed` by cgroup teardown
before that hook fires — leaving 623 of 1874 logs (33%) without summaries as
of 2026-06-05.

`coda plan` shows the debt at a glance. It classifies every log and prints
which ones need rendering.

## Usage

```sh
# Show summary-debt table
coda plan

# Machine-readable JSON output
coda plan --format json

# Override sessions directory and grace period
coda plan --sessions-dir ~/.cache/ctrace/sessions --grace-secs 300
```

**Exit code:** `1` when at least one log is `Orphaned` (debt exists), `0`
otherwise. Useful in hooks: `coda plan || trigger-backfill`.

## Configuration

`coda` reads `~/.config/coda/coda.toml` (absent file yields defaults):

```toml
# ~/.config/coda/coda.toml
grace_secs = 120        # younger-than-this missing-summary logs are Fresh
sessions_dir = "/home/jsy/.cache/ctrace/sessions"
```

See [`config/coda.example.toml`](config/coda.example.toml) for the full
annotated example.

## Type surface

All types are `pub`, `serde`-(de)serializable, and re-exported from the
crate root.

### `RawLog`

Raw observation from the store:

```rust
pub struct RawLog {
    pub path: PathBuf,       // path to the .ndjson file
    pub has_summary: bool,   // true if .summary.md exists beside it
    pub mtime_secs: u64,     // last-write unix timestamp
}
```

### `SummaryState`

```rust
pub enum SummaryState { Present, Missing }
```

### `DebtClass`

```rust
pub enum DebtClass {
    Active,                      // live ctrace log — never touch
    Fresh    { age_secs: u64 },  // missing summary, within grace period
    Orphaned { age_secs: u64 },  // missing summary, older than grace → render it
    Settled,                     // summary present — nothing to do
}
```

### `SessionLog`

One log with its classification:

```rust
pub struct SessionLog {
    pub path: PathBuf,
    pub summary: SummaryState,
    pub age_secs: u64,
    pub is_active: bool,
    pub debt: DebtClass,
}
```

### `SweepAction`

What `sweep` recommends for one log:

```rust
pub enum SweepAction {
    Render { path: PathBuf },                  // render this log (coda-close executes)
    Skip   { path: PathBuf, reason: String },  // skip; hook may still fire
    NoOp   { path: PathBuf },                  // nothing to do
}
```

### `SweepPlan`

The output of `sweep()` — declarative, no side effects:

```rust
pub struct SweepPlan {
    pub actions:  Vec<SweepAction>,
    pub logs:     Vec<SessionLog>,
    pub orphaned: usize,
    pub fresh:    usize,
    pub settled:  usize,
    pub total:    usize,
}
```

## `LogStore` trait

Abstracts the sessions directory so `sweep` is pure and testable. Sibling
crates (`coda-audit`, `coda-close`) implement this trait against the real
filesystem:

```rust
pub trait LogStore {
    type Error: std::fmt::Debug + std::fmt::Display;

    /// Return all .ndjson files in the sessions directory.
    fn logs(&self) -> Result<Vec<RawLog>, Self::Error>;

    /// Shell out to `scribe render` (apply-only; unused in coda-sweep).
    fn render(&self, path: &Path) -> Result<(), Self::Error>;
}
```

### `FakeStore`

In-memory fixture store for tests:

```rust
let store = FakeStore::new(vec![
    RawLog { path: PathBuf::from("/sessions/old.ndjson"), has_summary: false, mtime_secs: 1000 },
]);
let raw = store.logs()?;
let plan = sweep(&raw, None, now_secs, grace_secs);
```

## `sweep` function

```rust
pub fn sweep(
    logs: &[RawLog],
    active_log: Option<&Path>,
    now_secs: u64,
    grace_secs: u64,
) -> SweepPlan
```

Pure function — zero side effects, no filesystem or network access. Pass
pre-fetched `&[RawLog]` from `LogStore::logs()`.

## Sibling crates

| Crate | Purpose |
|-------|---------|
| `coda-audit` | `FsStore` + live active-log detection + audit report |
| `coda-close` | `--apply` flag: executes `Render` actions via `scribe render` |
| `coda-boot`  | boot-time sweep + cron integration |

## MSRV

Rust 1.85.

## License

MIT OR Apache-2.0
