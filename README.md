# 石 ishi

A terminal UI client for [Linear](https://linear.app), built with Rust.

## Features

- **Vim-style keybindings** — `j`/`k` to navigate, `gg`/`G` to jump, `/` to search, `Enter` to select, `Esc` to go back, `q` to quit
- **Read-only views** — My Issues, Project, and Issue Detail
- **Thread run modes** — from issue detail, choose foreground interactive continuation or non-blocking background `amp threads continue`; background run lifecycle metadata is persisted in state
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
