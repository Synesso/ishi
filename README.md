# 石 ishi

A terminal UI client for [Linear](https://linear.app), built with Rust.

## Features

- **Vim-style keybindings** — `j`/`k` to navigate, `gg`/`G` to jump, `/` to search, `Enter` to select, `Esc` to go back, `q` to quit
- **Read-only views** — My Issues, Project, and Issue Detail
- **Amp thread integration** — create, continue, and manage [Amp](https://ampcode.com) threads linked to Linear issues
- **Background sessions** — run `amp threads continue` in the background with full lifecycle tracking
- **Run management** — open logs, retry failed runs, or mark stale runs from the TUI
- **Async** — background API fetches with in-memory caching for snappy navigation
- **Simple auth** — reads `LINEAR_API_KEY` from env, `~/.config/ishi/config.toml`, or prompts on first run

## Getting started

```sh
# Set your Linear API key
export LINEAR_API_KEY="lin_api_..."

# Run
cargo run
```

## Background sessions

Ishi can launch Amp threads in the background, allowing you to continue using the TUI while work runs in a separate process. This section covers the session lifecycle, how to manage runs, and how to troubleshoot common problems.

### Starting a background run

1. Navigate to an issue and press `Enter` to open the detail view.
2. Press `Tab` to focus the threads panel.
3. Select a thread and press `Enter` to choose a run mode:
   - **`f` — Foreground**: suspends the TUI and opens an interactive `amp threads continue` session.
   - **`b` — Background**: spawns `amp threads continue --stream-json` in a detached process. The TUI remains usable.
4. To create a new thread, press `a` from the detail view, select a workspace directory, and compose a prompt.

### Session lifecycle statuses

Each background run tracks a lifecycle status:

| Status      | Meaning |
|-------------|---------|
| **Pending** | Run record created but process has not started yet. |
| **Running** | Process is alive (PID confirmed via `ps`). |
| **Completed** | Process exited with code 0. |
| **Failed**  | Process exited with a non-zero exit code. |
| **Stale**   | Process is gone but no exit marker was found in the log, or the run was manually marked stale. |

Statuses are reconciled automatically on each refresh (`r`). Reconciliation checks whether each run's PID is still alive and reads exit-code markers from log files to determine terminal status.

### Run management keybindings

When the threads panel is focused in the detail view:

| Key   | Action |
|-------|--------|
| `l`   | Open the latest run log for the selected thread in your default viewer. |
| `R`   | Retry the latest run for the selected thread (launches a new background run). |
| `x`   | Mark the latest run as stale (clears PID, sets status to stale). |

### State and log files

- **State file**: `~/.config/ishi/state.toml` — persists thread links (thread → issue + workspace), workspace history, and session run metadata.
- **Run logs**: `~/.local/state/ishi/runs/` (or `~/Library/Caches/ishi/runs/` on macOS) — each run writes stdout/stderr to `<run-id>.log`. An exit-code marker (`__ISHI_EXIT_CODE__=<code>`) is appended when the process exits.

### Using parallel workspaces with jj

You can run background sessions in separate `jj` workspaces to avoid conflicts with your working copy:

```sh
# Create a workspace for background work
jj workspace add ../ishi-bg

# In ishi, press `a` on an issue and select ../ishi-bg as the workspace
# Background runs in that workspace won't interfere with your main working copy
```

When starting a new thread, the workspace picker shows your history of previously used directories. Press `/` to type a new path, or use `Tab` to autocomplete.

### Troubleshooting

**Run stuck in "running" but process is gone**
The reconciler checks PID liveness on refresh. If the process exited without writing an exit marker (e.g. killed with `SIGKILL`), the run transitions to **stale**. Press `r` to trigger reconciliation, or `x` to manually mark it stale.

**Run stuck in "pending"**
A pending run has no PID yet. This can happen if the spawn failed silently. Check the run log with `l` for errors. Mark the run stale with `x` and retry with `R`.

**Log file is missing or empty**
Run logs are created at spawn time. If a log is missing, the `amp` binary may not have been found on `$PATH`. Verify `amp` is installed and accessible from the workspace directory.

**PID belongs to a different process**
Process IDs are reused by the OS. The reconciler trusts `ps -p <pid>` output. If a long-dead run shows as "running" because the PID was recycled, mark it stale with `x`. On the next refresh, the exit marker (if present) will determine the true terminal status.

**State file is corrupted**
Delete `~/.config/ishi/state.toml` to reset. Thread links and run history will be lost, but ishi will recreate the file on next launch. Existing Amp threads are unaffected (they live in `~/.local/share/amp/threads/`).

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
