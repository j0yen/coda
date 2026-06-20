# coda

Finds `ctrace` session logs that never got a summary, and renders the missing ones.

## Why this exists

`ctrace` records every Claude session to `~/.cache/ctrace/sessions/<id>.ndjson`. When a session ends cleanly, a `SessionEnd` hook writes a `<id>.summary.md` next to the log. Headless timer ticks don't end cleanly — cgroup teardown `SIGKILL`s them before the hook fires, so the summary never gets written and never will. The debt is silent and it accumulates. The measurement that prompted this: 623 of 1874 logs (33%) un-summarized as of 2026-06-05.

`coda` models that debt as a classification problem. Each log is `Settled` (summary present), `Fresh` (missing, but young enough that the hook might still fire), `Orphaned` (missing and old enough to be real debt), or `Active` (the live log — never touch it). The classifier is a pure function; everything that touches the filesystem or shells out to a renderer is built around it.

## Install

Requires `cargo` / `rustc` 1.85+.

```sh
git clone https://github.com/j0yen/coda.git
cd coda
cargo install --path . --locked
```

## Commands

```
coda plan    classify all logs, print the table (or --format json)
coda audit   same, but reads the real sessions dir and excludes the live log
coda close   render summaries for orphaned logs (print-only until --apply)
coda boot    install the systemd timer + SessionStart hook
```

Every command exits `1` when any log is `Orphaned` and `0` otherwise — so it composes in a hook: `coda plan || trigger-backfill`.

### plan / audit

```sh
coda audit                       # tally + (with --verbose) a per-log table
coda audit --orphaned-only       # just the orphaned paths, one per line
coda plan --format json          # machine-readable SweepPlan
coda audit --sessions-dir ~/.cache/ctrace/sessions --grace-secs 300
```

`audit` resolves the currently-open log (it shells `ctrace status`) and marks it `Active` so a live session is never mistaken for debt. `plan` is the simpler view and does not do active-log detection.

### close

```sh
coda close                       # print-only: lists orphans, "would render N"
coda close --apply               # render each orphan via `scribe render <path>`
coda close --apply --limit 20    # cap renders per invocation
coda close --json                # {"rendered":N,"skipped":M,"failed":K,"remaining":P}
```

`--apply` renders one summary per orphaned log; a per-log failure is counted, not fatal.

### boot

```sh
coda boot install                # print the SessionStart hook JSON entry
coda boot install --enable       # also copy the systemd units + daemon-reload
```

`--enable` copies `install/coda-close.{service,timer}` into `~/.config/systemd/user/`. It always prints, and never runs, the command to arm the timer: `systemctl --user enable --now coda-close.timer`.

## Configuration

`coda` reads `~/.config/coda/coda.toml`; an absent file yields defaults. CLI flags override config.

```toml
grace_secs = 120                                 # younger missing-summary logs are Fresh
sessions_dir = "/home/jsy/.cache/ctrace/sessions"
```

See [`config/coda.example.toml`](config/coda.example.toml) for the annotated version.

## How it's built

One library crate, exercised through the `coda` binary. The pieces:

- **`sweep(logs, active_log, now, grace_secs) -> SweepPlan`** — the classifier. Pure: no filesystem, no network. You hand it pre-fetched `RawLog`s and the current time.
- **`LogStore` trait** — abstracts the sessions directory so `sweep` stays pure. `FsStore` is the real filesystem implementation; `FakeStore` is the in-memory fixture used in tests.
- **`DebtClass`** — `Active` / `Fresh` / `Orphaned` / `Settled`, the heart of the model.
- **`SweepPlan`** — the result: a `SweepAction` per log (`Render` / `Skip` / `NoOp`) plus the orphaned/fresh/settled/total tallies.

All public types are `serde`-serializable and re-exported from the crate root, which is what makes `--format json` and the test fixtures straightforward.

## Status

`v0.3.0`. All four subcommands work; every acceptance criterion has an integration test. See [CHANGELOG.md](CHANGELOG.md) for what landed when.

## License

MIT OR Apache-2.0, at your option.
