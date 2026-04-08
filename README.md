# Agent of Empires

This repository is a fork of [njbrake/agent-of-empires](https://github.com/njbrake/agent-of-empires).

This fork is kept current with upstream and documents every fork-only feature or fix separately. See [Fork Features](docs/fork-features/index.md) for the current delta from upstream and the running history of additions that may later move upstream.

## 1. Test and run locally

Prerequisites:

- Rust toolchain
- `tmux` available on `PATH`

From the repository root:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo test
cargo test --test e2e -- --nocapture   # optional, requires tmux
cargo run --release
```

If you want a faster optimized local build:

```bash
cargo build --profile dev-release
./target/dev-release/aoe
```

## 2. Install locally from source

After cloning this fork, run:

```bash
cargo install --path .
aoe --version
```

To reinstall after local changes:

```bash
cargo install --path . --force
```
