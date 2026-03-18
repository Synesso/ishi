# 石 ishi

A terminal UI client for [Linear](https://linear.app), built with Rust.

## Features

- **Vim-style keybindings** — `j`/`k` to navigate, `gg`/`G` to jump, `/` to search, `Enter` to select, `Esc` to go back, `q` to quit
- **Read-only views** — My Issues, Project, and Issue Detail
- **Thread run modes** — from issue detail, choose foreground interactive continuation or non-blocking background `amp threads continue`; background run lifecycle metadata is persisted and reconciled (`running`/`completed`/`failed`/`stale`) in state
- **Run management actions** — in issue detail thread list, open latest run log (`l`), retry latest run (`R`), or mark latest run stale (`x`) for the selected thread; actions no-op safely when run metadata is unavailable
- **Async** — background API fetches with in-memory caching for snappy navigation
- **Simple auth** — reads `LINEAR_API_KEY` from env, `~/.config/ishi/config.toml`, or prompts on first run

## Getting started

```sh
# Set your Linear API key
export LINEAR_API_KEY="lin_api_..."

# Run
cargo run
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
